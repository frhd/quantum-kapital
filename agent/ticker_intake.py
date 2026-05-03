"""Baseline-note intake loop for newly-added tickers — Phase 2.

Polling loop. Every `poll_interval_secs` (default 60s), pulls the
watchlist via `get_watchlist`, filters to symbols that Phase 1 has
already primed (`last_primed_at IS NOT NULL`) and that don't yet have a
recent baseline `research_notes` row, gathers fundamentals + news +
bars, asks the LLM for a one-paragraph baseline thesis, and persists it
via `write_research_note` with `written_by = "agent.ticker_intake"`.

Mirrors `alert_dive.py`'s polling shape but is event-agnostic — it
reacts to "watchlist symbol with no recent baseline note" rather than
to alert fires. No new MCP tool is introduced; reuse-window dedup uses
an in-memory cache scoped to the daemon's runtime plus an optional
`list_research_notes` hook the MCP adapter exposes when available
(see `loop/plan/QUESTIONS.md::Phase 2`).

Entry point: `python -m ticker_intake` (from `agent/`) or
`uv run qk-ticker-intake` (via the script alias in pyproject.toml).
"""

from __future__ import annotations

import argparse
import asyncio
import logging
import os
import sys
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Mapping, Sequence

import budget_guard as bg
import data_summary as ds
from config import AgentConfig, load as load_config
from llm import AnthropicLlmClient, LlmClient
from mcp_client import McpClient, McpToolError


log = logging.getLogger("ticker_intake")


# Hardcode the writer string ONCE here so eval / UI / audits can
# distinguish baseline notes from `morning_sweep` and `alert_dive`
# writes. A grep test in `tests/test_ticker_intake.py` asserts no other
# writer string appears in this file.
WRITER = "agent.ticker_intake"


# ---- Configuration knobs (with defaults) -------------------------------------

DEFAULT_POLL_INTERVAL_SECS = 60
DEFAULT_PER_SYMBOL_USD = 0.10
DEFAULT_MAX_CONCURRENT = 2
DEFAULT_REUSE_NOTE_WINDOW_DAYS = 7
# Stop intake once 90% of the daily LLM budget is gone, mirroring
# `alert_dive`'s `GLOBAL_RESERVE_FRAC`. Three concurrent agents
# (morning_sweep + alert_dive + ticker_intake) share the same global
# cap — keeping a 10% reserve gives the schedulers headroom.
GLOBAL_RESERVE_FRAC = 0.10


# ---- Per-tick result ---------------------------------------------------------


@dataclass
class IntakeResult:
    symbol: str
    research_note_id: int | None = None
    skipped_reason: str | None = None
    spent_usd: float = 0.0


@dataclass
class TickResult:
    polled: int = 0
    eligible: int = 0
    written: int = 0
    skipped: int = 0
    failed: int = 0
    spent_usd: float = 0.0
    intakes: list[IntakeResult] = field(default_factory=list)


# ---- LLM tool schema ---------------------------------------------------------


WRITE_NOTE_TOOL: dict[str, Any] = {
    "name": "write_research_note",
    "description": (
        "Emit one baseline research note for the symbol the user just "
        "added to their watchlist. The body_md is a 200-400 word "
        "starting-point thesis: why this is interesting now, what the "
        "fundamentals / news / chart say, and the single thing that "
        "would invalidate the case. Cite news ids and concrete levels "
        "rather than adjectives. Conviction is a best-effort grade A/B/C "
        "— most baseline notes are B or C."
    ),
    "input_schema": {
        "type": "object",
        "properties": {
            "body_md": {"type": "string", "minLength": 50},
            "conviction": {"type": "string", "enum": ["A", "B", "C"]},
            "evidence_refs": {
                "type": "array",
                "items": {"type": "object"},
                "default": [],
            },
        },
        "required": ["body_md", "conviction"],
        "additionalProperties": False,
    },
}


# ---- Public entry points -----------------------------------------------------


async def run_tick(
    *,
    mcp: "TickerIntakeMcp",
    llm: LlmClient,
    cfg: AgentConfig,
    system_prompt: str,
    seen: "SeenCache",
    per_symbol_usd: float = DEFAULT_PER_SYMBOL_USD,
    max_concurrent: int = DEFAULT_MAX_CONCURRENT,
    reuse_note_window_days: int = DEFAULT_REUSE_NOTE_WINDOW_DAYS,
    now_fn: "Callable[[], datetime] | None" = None,  # noqa: F821
) -> TickResult:
    """One pass of the polling loop. Pure orchestration; no I/O setup.

    Returns a `TickResult` for tests / structured logging. Tests inject
    fakes via `mcp` and `llm`; production wiring lives in `_async_main`.
    """
    now_fn = now_fn or _utc_now
    out = TickResult()

    # 1. Pull watchlist.
    raw_watch = await mcp.get_watchlist()
    rows = _items_from_envelope(raw_watch)
    out.polled = len(rows)
    if not rows:
        return out

    # 2. Filter to "primed and no recent baseline note" candidates.
    reuse_window = timedelta(days=reuse_note_window_days)
    now = now_fn()
    candidates: list[Mapping[str, Any]] = []
    for row in rows:
        symbol = _symbol_of(row)
        if not symbol:
            continue
        if not _is_primed(row):
            continue
        if seen.recent(symbol, within=reuse_window, now=now):
            continue
        recent_id = await _recent_note_for_symbol(
            mcp,
            symbol,
            within=reuse_window,
            now=now,
        )
        if recent_id is not None:
            seen.mark(symbol, now)
            continue
        candidates.append(row)

    out.eligible = len(candidates)
    if not candidates:
        return out

    # 3. Global-budget gate before any LLM spend.
    raw_status = await mcp.get_llm_budget_status()
    status = bg.parse_global_status(raw_status if isinstance(raw_status, dict) else {})
    fraction_used = status.fraction_used
    cutoff = max(cfg.budget.abort_if_global_spend_above, 1.0 - GLOBAL_RESERVE_FRAC)
    if fraction_used >= cutoff:
        reason = (
            f"global daily budget {fraction_used:.0%} used "
            f">= cutoff {cutoff:.0%}"
        )
        log.warning("ticker_intake aborting: %s", reason)
        for row in candidates:
            sym = _symbol_of(row) or ""
            out.intakes.append(IntakeResult(symbol=sym, skipped_reason=reason))
            out.skipped += 1
        return out

    # 4. Fan out, throttled.
    sem = asyncio.Semaphore(max_concurrent)

    async def _run(row: Mapping[str, Any]) -> IntakeResult:
        async with sem:
            return await _intake_one(
                mcp=mcp,
                llm=llm,
                cfg=cfg,
                system_prompt=system_prompt,
                seen=seen,
                row=row,
                per_symbol_usd=per_symbol_usd,
                now_fn=now_fn,
            )

    intakes = await asyncio.gather(*(_run(c) for c in candidates), return_exceptions=False)
    for it in intakes:
        out.intakes.append(it)
        out.spent_usd += it.spent_usd
        if it.skipped_reason:
            out.skipped += 1
        elif it.research_note_id is not None:
            out.written += 1
        else:
            out.failed += 1
    return out


async def run_forever(
    *,
    mcp: "TickerIntakeMcp",
    llm: LlmClient,
    cfg: AgentConfig,
    system_prompt: str,
    poll_interval_secs: int = DEFAULT_POLL_INTERVAL_SECS,
    per_symbol_usd: float = DEFAULT_PER_SYMBOL_USD,
    max_concurrent: int = DEFAULT_MAX_CONCURRENT,
    reuse_note_window_days: int = DEFAULT_REUSE_NOTE_WINDOW_DAYS,
    stop_event: asyncio.Event | None = None,
) -> None:
    """Continuous polling loop. Cancels cleanly on `stop_event` or task cancel."""
    seen = SeenCache()
    log.info(
        "ticker_intake starting (interval=%ss, per_symbol=$%.2f, concurrent=%d, reuse=%dd)",
        poll_interval_secs,
        per_symbol_usd,
        max_concurrent,
        reuse_note_window_days,
    )
    while True:
        if stop_event and stop_event.is_set():
            return
        try:
            tick = await run_tick(
                mcp=mcp,
                llm=llm,
                cfg=cfg,
                system_prompt=system_prompt,
                seen=seen,
                per_symbol_usd=per_symbol_usd,
                max_concurrent=max_concurrent,
                reuse_note_window_days=reuse_note_window_days,
            )
            if tick.polled or tick.eligible or tick.written:
                log.info(
                    "tick: polled=%d eligible=%d written=%d skipped=%d failed=%d spent=$%.4f",
                    tick.polled,
                    tick.eligible,
                    tick.written,
                    tick.skipped,
                    tick.failed,
                    tick.spent_usd,
                )
        except Exception:  # noqa: BLE001
            log.exception("ticker_intake tick failed; sleeping then retrying")
        if stop_event:
            try:
                await asyncio.wait_for(stop_event.wait(), timeout=poll_interval_secs)
                return
            except asyncio.TimeoutError:
                pass
        else:
            await asyncio.sleep(poll_interval_secs)


# ---- Per-symbol intake -------------------------------------------------------


async def _intake_one(
    *,
    mcp: "TickerIntakeMcp",
    llm: LlmClient,
    cfg: AgentConfig,
    system_prompt: str,
    seen: "SeenCache",
    row: Mapping[str, Any],
    per_symbol_usd: float,
    now_fn,
) -> IntakeResult:
    symbol = _symbol_of(row) or ""
    if not symbol:
        return IntakeResult(symbol="", skipped_reason="watchlist row missing symbol")

    # Per-symbol budget guard. The global one already passed at tick start.
    guard = bg.BudgetGuard(
        per_loop_usd=per_symbol_usd,
        abort_if_global_spend_above=cfg.budget.abort_if_global_spend_above,
    )

    bundle, news = await _gather_for_symbol(mcp, symbol)

    body = _format_intake_block(symbol, row, bundle)

    try:
        guard.ensure_can_spend()
    except bg.BudgetExceeded as e:
        return IntakeResult(symbol=symbol, skipped_reason=f"per-symbol budget: {e}")

    user_msg = (
        f"The user just added {symbol} to their watchlist; write the "
        f"baseline research note. Use the `write_research_note` tool "
        f"exactly once. The note's evidence_refs may cite any news / "
        f"setup ids you reference. Be honest about C-grade setups — do "
        f"not inflate conviction.\n\n"
        f"{body}"
    )

    try:
        resp = await llm.call(
            model=cfg.models.smart,
            system=system_prompt,
            messages=[{"role": "user", "content": user_msg}],
            tools=[WRITE_NOTE_TOOL],
            tool_choice={"type": "tool", "name": "write_research_note"},
            max_tokens=2048,
        )
    except Exception as e:  # noqa: BLE001
        log.exception("LLM call failed for %s", symbol)
        return IntakeResult(symbol=symbol, skipped_reason=f"llm error: {e}")

    cost = guard.record(cfg.models.smart, resp.input_tokens, resp.output_tokens)

    if not resp.tool_uses:
        return IntakeResult(
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="LLM produced no tool_use",
        )

    payload = resp.tool_uses[0].input
    body_md = (payload.get("body_md") or "").strip()
    conviction = payload.get("conviction") or "C"
    refs = payload.get("evidence_refs") or []
    if not body_md:
        return IntakeResult(
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="empty body_md",
        )

    try:
        note = await mcp.write_research_note(
            symbol=symbol,
            body_md=body_md,
            conviction=conviction,
            evidence_refs=list(refs),
            written_by=WRITER,
        )
    except McpToolError:
        log.exception("write_research_note failed for %s", symbol)
        return IntakeResult(
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="write_research_note failed",
        )

    note_id = note.get("id") if isinstance(note, Mapping) else None
    if not isinstance(note_id, int):
        return IntakeResult(
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="note returned without id",
        )

    seen.mark(symbol, now_fn())
    log.info(
        "%s: wrote baseline note#%s conviction=%s spent=$%.4f",
        symbol,
        note_id,
        conviction,
        cost,
    )
    return IntakeResult(symbol=symbol, research_note_id=note_id, spent_usd=cost)


# ---- In-memory dedup ---------------------------------------------------------


class SeenCache:
    """Per-daemon-runtime "we already wrote a baseline note for this
    symbol" memory. Falls back to this when the MCP adapter doesn't
    expose `list_research_notes` (the production case as of Phase 2)."""

    def __init__(self) -> None:
        self._seen_at: dict[str, datetime] = {}

    def mark(self, symbol: str, when: datetime) -> None:
        self._seen_at[symbol.upper()] = when

    def recent(self, symbol: str, *, within: timedelta, now: datetime) -> bool:
        ts = self._seen_at.get(symbol.upper())
        if ts is None:
            return False
        return (now - ts) <= within


# ---- Helpers -----------------------------------------------------------------


def _utc_now() -> datetime:
    return datetime.now(tz=timezone.utc)


def _items_from_envelope(raw: Any) -> list[Mapping[str, Any]]:
    """The Rust read tools wrap top-level arrays as `{ items, count }`. Fakes
    sometimes return a bare list; accept both."""
    if isinstance(raw, Mapping) and "items" in raw:
        items = raw["items"]
        return list(items) if isinstance(items, list) else []
    if isinstance(raw, list):
        return list(raw)
    return []


def _symbol_of(row: Mapping[str, Any]) -> str | None:
    sym = row.get("symbol")
    if isinstance(sym, str) and sym.strip():
        return sym.upper()
    return None


def _is_primed(row: Mapping[str, Any]) -> bool:
    """`last_primed_at` may serialize as an RFC3339 string, a unix epoch
    int, or `None`. Treat any non-null value as "primed"; the column is
    set strictly by `TickerPrimerService::prime` after a successful run.
    """
    return row.get("last_primed_at") not in (None, "", 0)


async def _gather_for_symbol(
    mcp: "TickerIntakeMcp",
    symbol: str,
) -> tuple[ds.CandidateBundle, list[Mapping[str, Any]]]:
    since_setups = _utc_now() - timedelta(days=30)

    daily, intraday, fund, news, setups = await asyncio.gather(
        mcp.get_bars(symbol, "1d", 252),
        mcp.get_bars(symbol, "5m", 78),
        mcp.get_fundamentals(symbol),
        mcp.get_news(symbol, max_age_secs=24 * 3600),
        mcp.get_setups(symbol=symbol, since=since_setups),
        return_exceptions=True,
    )

    def _ok_list(v: Any) -> list[Mapping[str, Any]]:
        if isinstance(v, list):
            return v
        if isinstance(v, Mapping) and "items" in v:
            items = v["items"]
            return list(items) if isinstance(items, list) else []
        return []

    def _ok_dict(v: Any) -> Mapping[str, Any]:
        return v if isinstance(v, Mapping) else {}

    bundle = ds.CandidateBundle(
        symbol=symbol,
        daily_summary=ds.summarize_daily_bars(_ok_list(daily)),
        intraday_summary=ds.summarize_intraday_bars(_ok_list(intraday)),
        fundamentals_summary=ds.summarize_fundamentals(_ok_dict(fund)),
        news_summary=ds.summarize_news(_ok_list(news)),
        sentiment_summary="not consulted at intake",
        setups_summary=ds.summarize_setups(_ok_list(setups)),
    )
    return bundle, _ok_list(news)


def _format_intake_block(
    symbol: str,
    row: Mapping[str, Any],
    bundle: ds.CandidateBundle,
) -> str:
    primed_at = row.get("last_primed_at")
    source = row.get("source") or "?"
    notes = row.get("notes")
    primed_bits = (
        f"primed_at={primed_at}" if primed_at is not None else "primed_at=(missing)"
    )
    notes_bit = f"reason: {notes}" if notes else "reason: (none)"
    return (
        f"WATCHLIST METADATA\n"
        f"- symbol: {symbol}\n"
        f"- source: {source}\n"
        f"- {primed_bits}\n"
        f"- {notes_bit}\n\n"
        f"CONTEXT\n"
        f"{bundle.as_prompt_block()}"
    )


async def _recent_note_for_symbol(
    mcp: "TickerIntakeMcp",
    symbol: str,
    *,
    within: timedelta,
    now: datetime,
) -> int | None:
    """Best-effort lookup of any baseline-or-richer note for `symbol`
    written within `within`. Returns the note id, or None if no recent
    note exists or the MCP adapter doesn't expose `list_research_notes`
    (the production default — see `loop/plan/QUESTIONS.md::Phase 2`).
    """
    if not hasattr(mcp, "list_research_notes"):
        return None
    try:
        rows = await mcp.list_research_notes(  # type: ignore[attr-defined]
            symbol=symbol,
            limit=1,
        )
    except Exception:  # noqa: BLE001
        return None
    items = _items_from_envelope(rows)
    if not items:
        return None
    head = items[0]
    written_at = head.get("written_at")
    written_dt: datetime | None = None
    if isinstance(written_at, (int, float)):
        written_dt = datetime.fromtimestamp(float(written_at), tz=timezone.utc)
    elif isinstance(written_at, str):
        try:
            written_dt = datetime.fromisoformat(written_at.replace("Z", "+00:00"))
        except ValueError:
            written_dt = None
    if written_dt is None or now - written_dt > within:
        return None
    nid = head.get("id")
    if isinstance(nid, int):
        return nid
    return None


# ---- Protocol the loop expects from the MCP client ---------------------------


class TickerIntakeMcp:  # pragma: no cover — typing-only marker
    """Methods the ticker-intake loop expects on its MCP client.

    Production: `mcp_client.McpClient` wrapped in `_ProdAdapter`.
    Tests pass a fake mirroring this surface.
    """

    async def get_watchlist(self) -> Any: ...
    async def get_llm_budget_status(self) -> Any: ...
    async def get_bars(self, symbol: str, bar_size: str, lookback_days: int) -> Any: ...
    async def get_fundamentals(self, symbol: str) -> Any: ...
    async def get_news(self, symbol: str, max_age_secs: int | None = None) -> Any: ...
    async def get_setups(self, *, symbol: str | None = None, since: datetime | None = None) -> Any: ...
    async def write_research_note(
        self,
        *,
        symbol: str,
        body_md: str,
        conviction: str,
        evidence_refs: Sequence[Mapping[str, Any]],
        written_by: str,
    ) -> Mapping[str, Any]: ...


# ---- CLI ---------------------------------------------------------------------


def _read_system_prompt() -> str:
    p = Path(__file__).resolve().parent / "prompts" / "ticker_intake.md"
    if p.exists():
        return p.read_text(encoding="utf-8")
    return (
        "You are an equity research analyst writing baseline research "
        "notes for symbols a single trader has just added to their "
        "watchlist. Be specific, cite evidence, and grade conviction "
        "A/B/C honestly. Most baseline notes are B or C. Do not give "
        "financial advice."
    )


def _resolve_server_bin(cfg: AgentConfig) -> str:
    raw = cfg.mcp.server_bin
    if os.path.isabs(raw):
        return raw
    return str((Path(__file__).resolve().parent / raw).resolve())


async def _async_main(args: argparse.Namespace) -> int:
    cfg = load_config(args.config) if args.config else load_config()
    server_bin = _resolve_server_bin(cfg)
    socket_path = cfg.mcp.socket_path

    sys_prompt = _read_system_prompt()

    async with McpClient.connect(server_bin, socket_path=socket_path) as mcp:
        adapter = _ProdAdapter(mcp)
        llm: LlmClient = AnthropicLlmClient()

        if args.once or args.dry_run:
            seen = SeenCache()
            tick = await run_tick(
                mcp=adapter,
                llm=llm if not args.dry_run else _DryRunLlm(),
                cfg=cfg,
                system_prompt=sys_prompt,
                seen=seen,
                per_symbol_usd=args.per_symbol_usd,
                max_concurrent=args.concurrent,
                reuse_note_window_days=args.reuse_window_days,
            )
            log.info("tick result: %s", tick)
            return 0

        await run_forever(
            mcp=adapter,
            llm=llm,
            cfg=cfg,
            system_prompt=sys_prompt,
            poll_interval_secs=args.interval,
            per_symbol_usd=args.per_symbol_usd,
            max_concurrent=args.concurrent,
            reuse_note_window_days=args.reuse_window_days,
        )
    return 0


class _DryRunLlm:
    """Stub LLM that records the request and returns no tool_use, so the
    loop short-circuits before any write_research_note call. Used by
    `--dry-run` for cost-free smoke testing against a live MCP socket.
    """

    async def call(self, **kwargs: Any) -> Any:
        from llm import LlmResponse

        log.info("dry-run: skipping LLM call (model=%s)", kwargs.get("model"))
        return LlmResponse(
            text="",
            tool_uses=[],
            input_tokens=0,
            output_tokens=0,
            stop_reason="end_turn",
            raw=None,
        )


class _ProdAdapter:
    """Wraps `McpClient` for the ticker-intake protocol."""

    def __init__(self, mcp: McpClient) -> None:
        self._mcp = mcp

    async def get_watchlist(self) -> Any:
        return await self._mcp.get_watchlist()

    async def get_llm_budget_status(self) -> Any:
        return await self._mcp.get_llm_budget_status()

    async def get_bars(self, symbol: str, bar_size: str, lookback_days: int) -> Any:
        return await self._mcp.get_bars(symbol, bar_size, lookback_days)

    async def get_fundamentals(self, symbol: str) -> Any:
        return await self._mcp.get_fundamentals(symbol)

    async def get_news(self, symbol: str, max_age_secs: int | None = None) -> Any:
        return await self._mcp.get_news(symbol, max_age_secs=max_age_secs)

    async def get_setups(
        self,
        *,
        symbol: str | None = None,
        since: datetime | None = None,
    ) -> Any:
        return await self._mcp.get_setups(symbol=symbol, since=since)

    async def write_research_note(
        self,
        *,
        symbol: str,
        body_md: str,
        conviction: str,
        evidence_refs: Sequence[Mapping[str, Any]],
        written_by: str,
    ) -> Mapping[str, Any]:
        args: dict[str, Any] = {
            "symbol": symbol,
            "body_md": body_md,
            "conviction": conviction,
            "evidence_refs": list(evidence_refs),
            "written_by": written_by,
        }
        return await self._mcp.call_tool("write_research_note", args)


def main() -> int:
    parser = argparse.ArgumentParser(description="Quantum Kapital ticker-intake agent")
    parser.add_argument("--config", help="Path to config.toml (defaults to ./config.toml)")
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run a single tick and exit (vs. continuous polling)",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Run a single tick using a stub LLM that emits no tool_use; "
        "exercises the orchestration path without spending budget.",
    )
    parser.add_argument(
        "--interval",
        type=int,
        default=DEFAULT_POLL_INTERVAL_SECS,
        help=f"Poll interval seconds (default {DEFAULT_POLL_INTERVAL_SECS})",
    )
    parser.add_argument(
        "--per-symbol-usd",
        type=float,
        default=DEFAULT_PER_SYMBOL_USD,
        help=f"Per-symbol USD cap (default ${DEFAULT_PER_SYMBOL_USD:.2f})",
    )
    parser.add_argument(
        "--concurrent",
        type=int,
        default=DEFAULT_MAX_CONCURRENT,
        help=f"Max concurrent intakes (default {DEFAULT_MAX_CONCURRENT})",
    )
    parser.add_argument(
        "--reuse-window-days",
        type=int,
        default=DEFAULT_REUSE_NOTE_WINDOW_DAYS,
        help=(
            f"Skip a symbol whose baseline note was written within this "
            f"window (default {DEFAULT_REUSE_NOTE_WINDOW_DAYS})"
        ),
    )
    parser.add_argument("--log-level", default=os.environ.get("LOG_LEVEL", "INFO"))
    args = parser.parse_args()

    logging.basicConfig(
        level=args.log_level.upper(),
        format="%(asctime)s %(levelname)s %(name)s: %(message)s",
    )
    return asyncio.run(_async_main(args))


if __name__ == "__main__":
    sys.exit(main())
