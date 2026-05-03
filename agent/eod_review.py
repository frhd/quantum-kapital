"""EOD review — after-close calibration agent loop (Phase 7).

Runs once per weekday (cron at 17:00 ET). Resolves yesterday's
agent-authored morning pack, scores its predictions against realized
bars via `get_outcomes`, asks the LLM for a markdown commentary, and
persists it to the journal via `append_journal_entry`. The
daily-journal skill renders the resulting `journal_entries` row into
`journal/YYYY-MM-DD.md`.

Entry point: `python -m eod_review` (from `agent/`) or
`uv run qk-eod-review` (via the script alias in pyproject.toml).
"""

from __future__ import annotations

import argparse
import asyncio
import json
import logging
import os
import sys
from dataclasses import dataclass
from datetime import date, datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Mapping, Sequence

import budget_guard as bg
from config import AgentConfig, load as load_config
from llm import LlmClient, make_llm_client
from mcp_client import McpClient, McpToolError
from morning_sweep import is_trading_day


log = logging.getLogger("eod_review")


DEFAULT_SECTION = "EOD Review (Agent)"
DEFAULT_PER_LOOP_USD = 0.10


@dataclass
class EodReviewResult:
    journal_date: str
    pack_date: str | None
    predictions_considered: int
    outcomes_scored: int
    body_len: int
    skipped_reason: str | None = None
    spent_usd: float = 0.0


# ---- Public entry point ------------------------------------------------------


async def run_eod_review(
    *,
    mcp: "EodReviewMcp",
    llm: LlmClient,
    cfg: AgentConfig,
    today: date,
    section: str = DEFAULT_SECTION,
    eval_window_days: int = 1,
    per_loop_usd: float = DEFAULT_PER_LOOP_USD,
    dry_run: bool = False,
    system_prompt: str | None = None,
) -> EodReviewResult:
    """Drive one EOD review against an already-connected MCP client.

    Pure orchestration — no I/O setup. Tests inject fakes for `mcp`
    and `llm` and call this directly.
    """
    journal_iso = today.isoformat()
    pack_date = previous_trading_day(today)
    pack_iso = pack_date.isoformat()

    guard = bg.BudgetGuard(
        per_loop_usd=per_loop_usd,
        abort_if_global_spend_above=cfg.budget.abort_if_global_spend_above,
    )

    # 1. Global budget gate.
    raw_status = await mcp.get_llm_budget_status()
    status = bg.parse_global_status(raw_status if isinstance(raw_status, dict) else {})
    try:
        guard.check_global(status)
    except bg.GlobalBudgetExhausted as e:
        log.warning("eod_review aborting: %s", e)
        return EodReviewResult(
            journal_date=journal_iso,
            pack_date=pack_iso,
            predictions_considered=0,
            outcomes_scored=0,
            body_len=0,
            skipped_reason=str(e),
        )

    # 2. Pack + outcomes.
    pack = await mcp.get_morning_pack(date_iso=pack_iso)
    pack_ideas = _ideas_from_pack(pack)
    if not pack_ideas:
        body = _no_pack_body(pack_date)
        if not dry_run:
            await mcp.append_journal_entry(
                date_iso=journal_iso,
                section=section,
                body_md=body,
            )
        return EodReviewResult(
            journal_date=journal_iso,
            pack_date=pack_iso,
            predictions_considered=0,
            outcomes_scored=0,
            body_len=len(body),
            skipped_reason="no morning pack to score",
        )

    raw_outcomes = await mcp.get_outcomes(
        since_iso=pack_iso,
        eval_window_days=eval_window_days,
    )
    outcomes = _items_from_envelope(raw_outcomes)
    # Filter to outcomes that match the target pack date — get_outcomes
    # may return earlier dates if they exist in the table.
    same_day_outcomes = [
        o for o in outcomes if isinstance(o, Mapping) and o.get("pack_date") == pack_iso
    ]

    # 3. LLM synthesis.
    sys_prompt = system_prompt or _read_system_prompt()
    user_msg = _format_user_message(
        pack_date=pack_date,
        journal_date=today,
        ideas=pack_ideas,
        outcomes=same_day_outcomes,
    )

    try:
        guard.ensure_can_spend()
    except bg.BudgetExceeded as e:
        return EodReviewResult(
            journal_date=journal_iso,
            pack_date=pack_iso,
            predictions_considered=len(pack_ideas),
            outcomes_scored=len(same_day_outcomes),
            body_len=0,
            skipped_reason=f"per-loop budget: {e}",
        )

    try:
        resp = await llm.call(
            model=cfg.models.smart,
            system=sys_prompt,
            messages=[{"role": "user", "content": user_msg}],
            tools=None,
            tool_choice=None,
            max_tokens=2048,
        )
    except Exception as e:  # noqa: BLE001
        log.exception("eod_review LLM call failed")
        return EodReviewResult(
            journal_date=journal_iso,
            pack_date=pack_iso,
            predictions_considered=len(pack_ideas),
            outcomes_scored=len(same_day_outcomes),
            body_len=0,
            skipped_reason=f"llm error: {e}",
        )

    cost = guard.record(
        cfg.models.smart,
        resp.input_tokens,
        resp.output_tokens,
        envelope_cost_usd=resp.cost_usd,
    )
    body_md = (resp.text or "").strip()
    if not body_md:
        return EodReviewResult(
            journal_date=journal_iso,
            pack_date=pack_iso,
            predictions_considered=len(pack_ideas),
            outcomes_scored=len(same_day_outcomes),
            body_len=0,
            skipped_reason="empty LLM body",
            spent_usd=cost,
        )

    if dry_run:
        log.info(
            "dry-run: would write journal entry for %s (%d chars)",
            journal_iso,
            len(body_md),
        )
        return EodReviewResult(
            journal_date=journal_iso,
            pack_date=pack_iso,
            predictions_considered=len(pack_ideas),
            outcomes_scored=len(same_day_outcomes),
            body_len=len(body_md),
            spent_usd=cost,
        )

    try:
        await mcp.append_journal_entry(
            date_iso=journal_iso,
            section=section,
            body_md=body_md,
        )
    except McpToolError as e:
        log.exception("append_journal_entry failed")
        return EodReviewResult(
            journal_date=journal_iso,
            pack_date=pack_iso,
            predictions_considered=len(pack_ideas),
            outcomes_scored=len(same_day_outcomes),
            body_len=len(body_md),
            skipped_reason=f"append_journal_entry failed: {e}",
            spent_usd=cost,
        )

    log.info(
        "eod_review wrote %s/%s (%d ideas, %d outcomes, $%.4f)",
        journal_iso,
        section,
        len(pack_ideas),
        len(same_day_outcomes),
        cost,
    )
    return EodReviewResult(
        journal_date=journal_iso,
        pack_date=pack_iso,
        predictions_considered=len(pack_ideas),
        outcomes_scored=len(same_day_outcomes),
        body_len=len(body_md),
        spent_usd=cost,
    )


# ---- Helpers ----------------------------------------------------------------


def previous_trading_day(d: date) -> date:
    """Most recent trading day strictly before `d`. Falls back to one
    calendar day if the holiday calendar runs out, so the function can
    never loop forever."""
    cur = d - timedelta(days=1)
    for _ in range(10):
        if is_trading_day(cur):
            return cur
        cur -= timedelta(days=1)
    return d - timedelta(days=1)


def _items_from_envelope(raw: Any) -> list[Any]:
    if isinstance(raw, Mapping) and "items" in raw:
        items = raw["items"]
        return list(items) if isinstance(items, list) else []
    if isinstance(raw, list):
        return list(raw)
    return []


def _ideas_from_pack(pack: Any) -> list[Mapping[str, Any]]:
    if not isinstance(pack, Mapping):
        return []
    ideas = pack.get("ranked_ideas")
    if not isinstance(ideas, list):
        return []
    return [i for i in ideas if isinstance(i, Mapping)]


def _no_pack_body(pack_date: date) -> str:
    return (
        f"_No morning pack found for {pack_date.isoformat()} — nothing to score._\n\n"
        "Possible reasons: pre-market sweep skipped (budget / non-trading day), "
        "first run before any pack was authored, or DB out of sync."
    )


def _format_user_message(
    *,
    pack_date: date,
    journal_date: date,
    ideas: Sequence[Mapping[str, Any]],
    outcomes: Sequence[Mapping[str, Any]],
) -> str:
    """Render a compact prompt body. Outcomes joined on (pack_date,
    symbol) so the LLM can speak to "what played out vs. what didn't"
    without re-doing arithmetic."""
    by_symbol = {
        str(o.get("symbol")): o for o in outcomes if isinstance(o, Mapping)
    }
    lines: list[str] = []
    lines.append(f"PACK DATE: {pack_date.isoformat()}")
    lines.append(f"JOURNAL DATE: {journal_date.isoformat()}")
    lines.append("")
    lines.append("PREDICTIONS")
    lines.append("-----------")
    for i, idea in enumerate(ideas, start=1):
        sym = str(idea.get("symbol", "?")).upper()
        outcome = by_symbol.get(sym, {})
        oc = outcome.get("outcome_class", "(not scored)")
        realized_high = outcome.get("realized_high")
        realized_low = outcome.get("realized_low")
        realized_close = outcome.get("realized_close")
        conviction = idea.get("conviction") or "-"
        entry_zone = idea.get("entry_zone") or "(unspecified)"
        invalidation = idea.get("invalidation") or "(unspecified)"
        thesis = (idea.get("thesis_md") or "").strip()
        thesis_excerpt = thesis if len(thesis) <= 280 else thesis[:280] + "..."
        lines.append(
            f"{i}. {sym} ({conviction}) — outcome={oc}"
        )
        lines.append(f"   entry_zone: {entry_zone}")
        lines.append(f"   invalidation: {invalidation}")
        if realized_high is not None and realized_low is not None:
            lines.append(
                f"   realized: high={realized_high} low={realized_low} close={realized_close}"
            )
        else:
            lines.append("   realized: (no bars in eval window)")
        if thesis_excerpt:
            lines.append(f"   thesis: {thesis_excerpt}")
        lines.append("")

    counts: dict[str, int] = {}
    for o in outcomes:
        if not isinstance(o, Mapping):
            continue
        cls = str(o.get("outcome_class") or "unknown")
        counts[cls] = counts.get(cls, 0) + 1
    if counts:
        lines.append("OUTCOME COUNTS: " + json.dumps(counts, sort_keys=True))
    lines.append("")
    lines.append(
        "TASK: Write a markdown commentary scoring yesterday's calls. Cover: "
        "(a) which predictions played out vs. which didn't and why, "
        "(b) any miscalibration patterns visible in the conviction grades, "
        "(c) one or two notes on what to watch next session. "
        "Be terse — 200-400 words total. Output markdown only; do not "
        "include front-matter, fenced wrappers, or section headers above "
        "level 3 (### or smaller)."
    )
    return "\n".join(lines)


def _read_system_prompt() -> str:
    p = Path(__file__).resolve().parent / "prompts" / "eod_review.md"
    if p.exists():
        return p.read_text(encoding="utf-8")
    return (
        "You are an equity research analyst writing terse, evidence-grounded "
        "after-close calibration notes for a single trader. Your job is to "
        "honestly score predictions against realized price action — credit "
        "winners, name misses, and call out conviction miscalibration when "
        "it shows. Do not give financial advice. This is a journal entry, "
        "not a thesis: keep it tight."
    )


# ---- MCP protocol ------------------------------------------------------------


class EodReviewMcp:  # pragma: no cover — typing-only marker
    """Methods the EOD review loop expects on its MCP client."""

    async def get_llm_budget_status(self) -> Any: ...
    async def get_morning_pack(self, *, date_iso: str) -> Any: ...
    async def get_outcomes(
        self, *, since_iso: str, eval_window_days: int = 1
    ) -> Any: ...
    async def append_journal_entry(
        self, *, date_iso: str, section: str, body_md: str
    ) -> Any: ...


class _ProdAdapter:
    """Adapts production `McpClient` to the EodReviewMcp protocol."""

    def __init__(self, mcp: McpClient) -> None:
        self._mcp = mcp

    async def get_llm_budget_status(self) -> Any:
        return await self._mcp.get_llm_budget_status()

    async def get_morning_pack(self, *, date_iso: str) -> Any:
        return await self._mcp.call_tool("get_morning_pack", {"date": date_iso})

    async def get_outcomes(
        self, *, since_iso: str, eval_window_days: int = 1
    ) -> Any:
        return await self._mcp.call_tool(
            "get_outcomes",
            {"since": since_iso, "eval_window_days": eval_window_days},
        )

    async def append_journal_entry(
        self, *, date_iso: str, section: str, body_md: str
    ) -> Any:
        return await self._mcp.call_tool(
            "append_journal_entry",
            {"date": date_iso, "section": section, "body_md": body_md},
        )


# ---- CLI --------------------------------------------------------------------


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
        adapter = _ProdAdapter(mcp)
        llm: LlmClient = make_llm_client(cfg.llm_backend)
        result = await run_eod_review(
            mcp=adapter,
            llm=llm,
            cfg=cfg,
            today=today,
            section=args.section,
            eval_window_days=args.eval_window_days,
            per_loop_usd=args.per_loop_usd,
            dry_run=args.dry_run,
        )
    log.info("eod_review result: %s", result)
    return 0


def main() -> int:
    parser = argparse.ArgumentParser(description="Quantum Kapital EOD review agent")
    parser.add_argument("--config", help="Path to config.toml")
    parser.add_argument("--date", help="Override today's date (YYYY-MM-DD)")
    parser.add_argument(
        "--section",
        default=DEFAULT_SECTION,
        help=f"Journal section heading (default {DEFAULT_SECTION!r})",
    )
    parser.add_argument(
        "--eval-window-days",
        type=int,
        default=1,
        help="Number of trading days the outcome extractor evaluates (default 1)",
    )
    parser.add_argument(
        "--per-loop-usd",
        type=float,
        default=DEFAULT_PER_LOOP_USD,
        help=f"Per-loop USD cap (default ${DEFAULT_PER_LOOP_USD:.2f})",
    )
    parser.add_argument("--dry-run", action="store_true", help="Skip the journal write")
    parser.add_argument("--force", action="store_true", help="Run on weekends/holidays")
    parser.add_argument("--log-level", default=os.environ.get("LOG_LEVEL", "INFO"))
    args = parser.parse_args()

    logging.basicConfig(
        level=args.log_level.upper(),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )
    return asyncio.run(_async_main(args))


if __name__ == "__main__":
    sys.exit(main())


# Avoid an unused-imports complaint in `from __future__` mode for these.
_ = datetime, timezone
