"""Morning sweep — pre-market research agent loop.

Runs once per weekday (cron at 07:00 ET). Pulls candidates + watchlist, gathers
data via MCP, scores with the LLM, synthesizes 3-5 ranked ideas, and writes
the morning pack via `write_morning_pack`.

Entry point: `python -m morning_sweep` (from `agent/`) or
`uv run morning_sweep` (via the script alias in pyproject.toml).
"""

from __future__ import annotations

import argparse
import asyncio
import logging
import os
import sys
from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Mapping, Sequence

import budget_guard as bg
import data_summary as ds
import playbook as pb
from config import AgentConfig, load as load_config
from llm import LlmClient, make_llm_client
from mcp_client import McpClient, McpToolError, hours_ago_unix
from ranker import rank_candidates
from synthesizer import synthesize_pack


log = logging.getLogger("morning_sweep")


@dataclass
class PlaybookOutcome:
    """Result of the structured-playbook extension."""

    pack_date: str
    n_setups: int = 0
    n_skip: int = 0
    generation_id: int | None = None
    skipped_reason: str | None = None
    spent_usd: float = 0.0


@dataclass
class SweepResult:
    date: str
    candidates_considered: int
    ideas_written: int
    spent_usd: float
    skipped_reason: str | None = None
    shadow: bool = False
    playbook: PlaybookOutcome | None = None


# ---- Loop ---------------------------------------------------------------------


async def run_sweep(
    *,
    mcp: McpClient,
    llm: LlmClient,
    cfg: AgentConfig,
    today: date,
    shadow: bool = False,
    dry_run: bool = False,
    system_prompt: str | None = None,
    skip_playbook: bool = False,
    playbook_system_prompt: str | None = None,
) -> SweepResult:
    """Drive one sweep against an already-connected MCP client and LLM seam.

    Pure orchestration — no I/O setup. Tests use this directly with fakes.
    """
    iso_today = today.isoformat()
    guard = bg.BudgetGuard(
        per_loop_usd=cfg.budget.per_loop_usd,
        abort_if_global_spend_above=cfg.budget.abort_if_global_spend_above,
    )

    # 1. Global budget check.
    raw_status = await mcp.get_llm_budget_status()
    status = bg.parse_global_status(raw_status if isinstance(raw_status, dict) else {})
    try:
        guard.check_global(status)
    except bg.GlobalBudgetExhausted as e:
        log.warning("aborting: %s", e)
        return SweepResult(
            date=iso_today,
            candidates_considered=0,
            ideas_written=0,
            spent_usd=0.0,
            skipped_reason=str(e),
            shadow=shadow,
        )

    # 2. Build the universe.
    candidates = await mcp.get_candidates(
        min_score=cfg.universe.candidate_min_score,
        include_promoted=False,
    )
    watchlist = await mcp.get_watchlist()
    symbols = ds.candidate_set(
        candidates,
        watchlist,
        min_score=cfg.universe.candidate_min_score,
        top_k=cfg.universe.top_k,
    )
    log.info("universe size=%d (watchlist=%d, candidates=%d)", len(symbols), len(watchlist), len(candidates))

    if not symbols:
        return SweepResult(
            date=iso_today,
            candidates_considered=0,
            ideas_written=0,
            spent_usd=0.0,
            skipped_reason="empty universe",
            shadow=shadow,
        )

    # 3. Gather data per symbol — concurrent inside each symbol, serial across
    # symbols to keep IBKR / AV rate-limit-friendly.
    bundles = await _gather_bundles(mcp, symbols, cfg)

    # Mid-flight global budget re-check before LLM steps.
    raw_status = await mcp.get_llm_budget_status()
    status = bg.parse_global_status(raw_status if isinstance(raw_status, dict) else {})
    try:
        guard.check_global(status)
    except bg.GlobalBudgetExhausted as e:
        log.warning("aborting between gather and rank: %s", e)
        return SweepResult(
            date=iso_today,
            candidates_considered=len(bundles),
            ideas_written=0,
            spent_usd=guard.spent_usd,
            skipped_reason=str(e),
            shadow=shadow,
        )

    sys_prompt = system_prompt or _read_system_prompt()

    # 4. Rank.
    try:
        scores = await rank_candidates(
            llm=llm,
            model=cfg.models.smart,
            system_prompt=sys_prompt,
            bundles=bundles,
            guard=guard,
        )
    except bg.BudgetExceeded as e:
        return SweepResult(
            date=iso_today,
            candidates_considered=len(bundles),
            ideas_written=0,
            spent_usd=guard.spent_usd,
            skipped_reason=f"budget exhausted in ranking: {e}",
            shadow=shadow,
        )
    log.info("ranked %d candidates", len(scores))

    # 5. Synthesize.
    try:
        ideas = await synthesize_pack(
            llm=llm,
            model=cfg.models.smart,
            system_prompt=sys_prompt,
            scores=scores,
            bundles=bundles,
            guard=guard,
            min_ideas=cfg.output.min_ideas,
            max_ideas=cfg.output.max_ideas,
            shadow=shadow,
        )
    except bg.BudgetExceeded as e:
        return SweepResult(
            date=iso_today,
            candidates_considered=len(bundles),
            ideas_written=0,
            spent_usd=guard.spent_usd,
            skipped_reason=f"budget exhausted in synthesis: {e}",
            shadow=shadow,
        )

    # 6. Persist.
    morning_pack_written = False
    if dry_run:
        log.info("dry-run: would write %d ideas to morning pack %s", len(ideas), iso_today)
    elif ideas:
        await mcp.write_morning_pack(date=iso_today, ranked_ideas=ideas)
        morning_pack_written = True
        log.info("wrote morning pack %s with %d ideas", iso_today, len(ideas))

        # 7. Optional: promote any non-watchlist symbol into the watchlist.
        watch_syms = {(w.get("symbol") or "").upper() for w in watchlist}
        for idea in ideas:
            sym = idea["symbol"]
            if sym not in watch_syms:
                try:
                    await mcp.add_ticker(sym, reason=f"morning_sweep {iso_today}")
                except Exception:  # noqa: BLE001
                    log.exception("add_ticker failed for %s; continuing", sym)
    else:
        log.info("no ideas met the bar; not writing pack for %s", iso_today)

    # 8. Playbook extension — second LLM call, structured tool output.
    # Independent of the morning_pack so a playbook can ship even if the
    # pack is empty (no high-conviction ideas != no setups to skip).
    playbook_outcome: PlaybookOutcome | None = None
    if not skip_playbook and bundles:
        playbook_outcome = await _run_playbook(
            mcp=mcp,
            llm=llm,
            cfg=cfg,
            guard=guard,
            iso_today=iso_today,
            bundles=bundles,
            system_prompt=playbook_system_prompt,
            dry_run=dry_run,
        )
    elif skip_playbook:
        log.info("playbook step skipped (--no-playbook)")

    # 9. Final budget log.
    final = await mcp.get_llm_budget_status()
    log.info("final spend: loop=$%.4f global=%s", guard.spent_usd, final)

    _ = morning_pack_written  # currently unused but documents intent
    return SweepResult(
        date=iso_today,
        candidates_considered=len(bundles),
        ideas_written=len(ideas),
        spent_usd=guard.spent_usd,
        shadow=shadow,
        playbook=playbook_outcome,
    )


async def _run_playbook(
    *,
    mcp: McpClient,
    llm: LlmClient,
    cfg: AgentConfig,
    guard: bg.BudgetGuard,
    iso_today: str,
    bundles: list[ds.CandidateBundle],
    system_prompt: str | None,
    dry_run: bool,
) -> PlaybookOutcome:
    """Generate + persist the structured pre-market playbook.

    Sibling to the morning_pack write above. v1 passes
    `trader_profile = None`; Phase 6 wires in the real profile fetched
    from `get_trader_profile`. The prompt template already includes a
    placeholder, so wiring the profile is a one-line change here."""
    try:
        guard.ensure_can_spend()
    except bg.BudgetExceeded as e:
        return PlaybookOutcome(
            pack_date=iso_today,
            skipped_reason=f"per-loop budget: {e}",
        )

    sys_prompt = system_prompt or _read_playbook_system_prompt()
    user_msg = pb.format_playbook_prompt(
        pack_date=iso_today,
        bundles=bundles,
        trader_profile=None,  # Phase 6 wires the real one.
    )

    try:
        resp = await llm.call(
            model=cfg.models.smart,
            system=sys_prompt,
            messages=[{"role": "user", "content": user_msg}],
            tools=[pb.RANKED_SETUPS_TOOL_SCHEMA],
            tool_choice={"type": "tool", "name": pb.PLAYBOOK_TOOL_NAME},
            max_tokens=3000,
        )
    except Exception as e:  # noqa: BLE001
        log.exception("playbook LLM call failed")
        return PlaybookOutcome(
            pack_date=iso_today,
            skipped_reason=f"llm error: {e}",
        )

    cost = guard.record(
        cfg.models.smart,
        resp.input_tokens,
        resp.output_tokens,
        envelope_cost_usd=resp.cost_usd,
    )

    tool_use = next(
        (u for u in resp.tool_uses if u.name == pb.PLAYBOOK_TOOL_NAME),
        None,
    )
    if tool_use is None:
        return PlaybookOutcome(
            pack_date=iso_today,
            skipped_reason="LLM did not call submit_playbook",
            spent_usd=cost,
        )

    parsed = pb.parse_tool_response(tool_use.input)
    n_setups = len(parsed["ranked_setups"])
    n_skip = len(parsed["skip_list"])

    if dry_run:
        log.info(
            "dry-run: would write playbook for %s (%d setups, %d skip)",
            iso_today,
            n_setups,
            n_skip,
        )
        return PlaybookOutcome(
            pack_date=iso_today,
            n_setups=n_setups,
            n_skip=n_skip,
            skipped_reason="dry-run",
            spent_usd=cost,
        )

    try:
        result = await mcp.write_playbook(
            date_iso=iso_today,
            ranked_setups=parsed["ranked_setups"],
            skip_list=parsed["skip_list"],
        )
    except McpToolError as e:
        log.exception("write_playbook failed")
        return PlaybookOutcome(
            pack_date=iso_today,
            n_setups=n_setups,
            n_skip=n_skip,
            skipped_reason=f"write_playbook failed: {e}",
            spent_usd=cost,
        )

    generation_id: int | None = None
    if isinstance(result, Mapping):
        g = result.get("generation_id")
        if isinstance(g, int):
            generation_id = g

    log.info(
        "wrote playbook %s gen=%s setups=%d skip=%d ($%.4f)",
        iso_today,
        generation_id,
        n_setups,
        n_skip,
        cost,
    )
    return PlaybookOutcome(
        pack_date=iso_today,
        n_setups=n_setups,
        n_skip=n_skip,
        generation_id=generation_id,
        spent_usd=cost,
    )


async def _gather_bundles(
    mcp: McpClient,
    symbols: Sequence[str],
    cfg: AgentConfig,
) -> list[ds.CandidateBundle]:
    out: list[ds.CandidateBundle] = []
    today_utc = datetime.now(tz=timezone.utc).replace(hour=0, minute=0, second=0, microsecond=0)
    since_setups = today_utc - timedelta(days=cfg.universe.setups_lookback_days)
    since_unix_24h = hours_ago_unix(24)

    for sym in symbols:
        try:
            daily, intraday, fund, news, sentiment, setups = await asyncio.gather(
                mcp.get_bars(sym, "1d", 252),
                mcp.get_bars(sym, "5m", 78),
                mcp.get_fundamentals(sym),
                mcp.get_news(sym, max_age_secs=24 * 3600),
                mcp.get_sentiment(sym, since=_unix_to_dt(since_unix_24h)),
                mcp.get_setups(symbol=sym, since=since_setups),
            )
        except Exception:  # noqa: BLE001
            log.exception("gather failed for %s; skipping", sym)
            continue

        out.append(
            ds.CandidateBundle(
                symbol=sym,
                daily_summary=ds.summarize_daily_bars(daily or []),
                intraday_summary=ds.summarize_intraday_bars(intraday or []),
                fundamentals_summary=ds.summarize_fundamentals(fund),
                news_summary=ds.summarize_news(news),
                sentiment_summary=ds.summarize_sentiment(sentiment),
                setups_summary=ds.summarize_setups(setups),
            )
        )
    return out


def _unix_to_dt(ts: int) -> datetime:
    return datetime.fromtimestamp(ts, tz=timezone.utc)


# ---- Trading-calendar / CLI ---------------------------------------------------


# Hardcoded for 2024-2026; Rust side has the canonical list. Keep in sync if
# years are added there. (See src-tauri/src/utils/market_calendar/holidays.rs.)
_US_HOLIDAYS_2024_2026: frozenset[date] = frozenset(
    {
        # 2024
        date(2024, 1, 1), date(2024, 1, 15), date(2024, 2, 19),
        date(2024, 3, 29), date(2024, 5, 27), date(2024, 6, 19),
        date(2024, 7, 4), date(2024, 9, 2), date(2024, 11, 28),
        date(2024, 12, 25),
        # 2025
        date(2025, 1, 1), date(2025, 1, 9), date(2025, 1, 20),
        date(2025, 2, 17), date(2025, 4, 18), date(2025, 5, 26),
        date(2025, 6, 19), date(2025, 7, 4), date(2025, 9, 1),
        date(2025, 11, 27), date(2025, 12, 25),
        # 2026
        date(2026, 1, 1), date(2026, 1, 19), date(2026, 2, 16),
        date(2026, 4, 3), date(2026, 5, 25), date(2026, 6, 19),
        date(2026, 7, 3), date(2026, 9, 7), date(2026, 11, 26),
        date(2026, 12, 25),
    }
)


def is_trading_day(d: date) -> bool:
    return d.weekday() < 5 and d not in _US_HOLIDAYS_2024_2026


def _read_system_prompt() -> str:
    p = Path(__file__).resolve().parent / "prompts" / "morning_sweep.md"
    return p.read_text(encoding="utf-8")


def _read_playbook_system_prompt() -> str:
    p = Path(__file__).resolve().parent / "prompts" / "playbook.md"
    if p.exists():
        return p.read_text(encoding="utf-8")
    return (
        "You are an equity desk strategist writing a tight, actionable "
        "pre-market playbook. Always call `submit_playbook` with "
        "`ranked_setups` and `skip_list` — never reply with bare text."
    )


def _resolve_server_bin(cfg: AgentConfig) -> str:
    raw = cfg.mcp.server_bin
    if os.path.isabs(raw):
        return raw
    return str((Path(__file__).resolve().parent / raw).resolve())


async def _async_main(args: argparse.Namespace) -> int:
    cfg = load_config(args.config) if args.config else load_config()
    today = date.fromisoformat(args.date) if args.date else date.today()

    if not args.force and not is_trading_day(today):
        log.info("non-trading day %s; nothing to do", today.isoformat())
        return 0

    server_bin = _resolve_server_bin(cfg)
    socket_path = cfg.mcp.socket_path

    async with McpClient.connect(server_bin, socket_path=socket_path) as mcp:
        llm: LlmClient = make_llm_client(cfg.llm_backend)
        result = await run_sweep(
            mcp=mcp,
            llm=llm,
            cfg=cfg,
            today=today,
            shadow=args.shadow,
            dry_run=args.dry_run,
            skip_playbook=args.no_playbook,
        )
    log.info("sweep result: %s", result)
    if result.skipped_reason:
        # Still exit 0 — graceful skips (holidays, budget) aren't failures.
        return 0
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Quantum Kapital morning sweep agent")
    parser.add_argument("--config", help="Path to config.toml (defaults to ./config.toml)")
    parser.add_argument("--date", help="Override today's date (YYYY-MM-DD)")
    parser.add_argument("--shadow", action="store_true", help="Tag pack as shadow output")
    parser.add_argument("--dry-run", action="store_true", help="Run loop without writing morning pack")
    parser.add_argument(
        "--no-playbook",
        action="store_true",
        help="Skip the structured playbook LLM call (smoke tests only).",
    )
    parser.add_argument("--force", action="store_true", help="Run even on weekends/holidays")
    parser.add_argument("--log-level", default=os.environ.get("LOG_LEVEL", "INFO"))
    args = parser.parse_args()

    logging.basicConfig(
        level=args.log_level.upper(),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )
    return asyncio.run(_async_main(args))


if __name__ == "__main__":
    sys.exit(main())
