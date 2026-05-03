"""Per-alert deep-dive agent — Phase 6.

Polling loop. Every `poll_interval_secs` (default 30s), pulls every alert
the tracker has fired but the dive hasn't yet enriched, gathers context
for each via MCP read tools, asks the LLM to synthesise a research note,
persists it via `write_research_note`, and stamps `mark_alert_enriched`
to close the loop. Budget guardrails at two layers: a per-alert USD cap
(`per_alert_usd`) and the global daily ceiling enforced by `LlmService`.

The watermark in this loop is a perf hint only — `enriched_at IS NULL`
on the row remains the source of truth, so a crash mid-flight is safe
to retry on the next tick.

Entry point: `python -m alert_dive` (from `agent/`) or
`uv run qk-alert-dive` (via the script alias in pyproject.toml).
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
from llm import LlmClient, make_llm_client
from mcp_client import McpClient, McpToolError


log = logging.getLogger("alert_dive")


# ---- Configuration knobs (with defaults) -------------------------------------
#
# These would migrate to config.toml the moment we want to vary them per
# environment. For Phase 6 we keep the defaults inline so a fresh checkout
# has the loop usable without editing config.

DEFAULT_POLL_INTERVAL_SECS = 30
DEFAULT_PER_ALERT_USD = 0.05
DEFAULT_MAX_CONCURRENT = 2
DEFAULT_BATCH_LIMIT = 10
# How recently a research_note for the same symbol can short-circuit a new
# dive. Two alerts for the same symbol within this window reuse the most
# recent note instead of re-running the full synth.
DEFAULT_REUSE_NOTE_WITHIN_MINS = 30
# Skip enrichment when the global daily budget has less than this fraction
# remaining (i.e. we're past 1 - GLOBAL_RESERVE_FRAC of the cap). 0.10 ⇒
# stop diving once 90% of the daily budget has been consumed.
GLOBAL_RESERVE_FRAC = 0.10


# ---- Per-tick result ---------------------------------------------------------


@dataclass
class DiveResult:
    """Outcome of enriching a single alert."""

    alert_id: int
    symbol: str | None
    research_note_id: int | None = None
    skipped_reason: str | None = None
    spent_usd: float = 0.0


@dataclass
class TickResult:
    polled: int = 0
    enriched: int = 0
    skipped: int = 0
    failed: int = 0
    spent_usd: float = 0.0
    dives: list[DiveResult] = field(default_factory=list)


# ---- LLM tool schema ---------------------------------------------------------


WRITE_NOTE_TOOL: dict[str, Any] = {
    "name": "write_research_note",
    "description": (
        "Emit one deep-dive research note for the alert in question. The "
        "body_md must cite specific evidence from the inputs (alert id, "
        "news ids, sentiment counts, recent setup ids). Conviction is a "
        "best-effort grade A/B/C — A reserved for high-confidence asymmetric "
        "setups. Keep body_md under ~500 words; this is a per-alert note, "
        "not a full thesis."
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
    mcp: AlertDiveMcp,
    llm: LlmClient,
    cfg: AgentConfig,
    system_prompt: str,
    per_alert_usd: float = DEFAULT_PER_ALERT_USD,
    max_concurrent: int = DEFAULT_MAX_CONCURRENT,
    batch_limit: int = DEFAULT_BATCH_LIMIT,
    reuse_note_within_mins: int = DEFAULT_REUSE_NOTE_WITHIN_MINS,
    now_fn: "Callable[[], datetime] | None" = None,  # noqa: F821
) -> TickResult:
    """One pass of the polling loop. Pure orchestration; no I/O setup.

    Returns a `TickResult` for tests / structured logging. Tests inject
    fakes via `mcp` and `llm`; the production wiring lives in
    `_async_main`.
    """
    now_fn = now_fn or _utc_now
    out = TickResult()

    # 1. Pull pending alerts. We rely on `enriched_at IS NULL` (server-side
    #    filter), not the watermark — see master plan gotcha.
    raw_alerts = await mcp.get_alerts(
        unenriched_only=True,
        limit=batch_limit,
    )
    items = _items_from_envelope(raw_alerts)
    out.polled = len(items)
    if not items:
        return out

    # 2. Global-budget gate before any LLM spend. We treat the configured
    #    `abort_if_global_spend_above` as the alert-dive cutoff too —
    #    consistent with the morning sweep — and additionally enforce the
    #    `GLOBAL_RESERVE_FRAC` floor so a slow-drip exhaustion doesn't
    #    silently push past 90%.
    raw_status = await mcp.get_llm_budget_status()
    status = bg.parse_global_status(raw_status if isinstance(raw_status, dict) else {})
    fraction_used = status.fraction_used
    cutoff = max(cfg.budget.abort_if_global_spend_above, 1.0 - GLOBAL_RESERVE_FRAC)
    if fraction_used >= cutoff:
        # Stamp every pending alert as "skipped" so the loop doesn't keep
        # picking them up next tick — the agent already noted the budget
        # situation, no point burning DB writes on the same rows.
        reason = (
            f"global daily budget {fraction_used:.0%} used "
            f">= cutoff {cutoff:.0%}"
        )
        log.warning("alert_dive aborting: %s", reason)
        for a in items:
            alert_id = int(a.get("id") or 0)
            if alert_id <= 0:
                continue
            try:
                await mcp.mark_alert_enriched(alert_id=alert_id, research_note_id=None)
            except Exception:  # noqa: BLE001
                log.exception("mark_alert_enriched(skip) failed for alert#%s", alert_id)
            out.dives.append(
                DiveResult(
                    alert_id=alert_id,
                    symbol=_alert_symbol(a),
                    skipped_reason=reason,
                )
            )
            out.skipped += 1
        return out

    # 3. Fan out, throttled.
    sem = asyncio.Semaphore(max_concurrent)

    async def _run(alert: Mapping[str, Any]) -> DiveResult:
        async with sem:
            return await _dive_one(
                mcp=mcp,
                llm=llm,
                cfg=cfg,
                system_prompt=system_prompt,
                alert=alert,
                per_alert_usd=per_alert_usd,
                reuse_note_within_mins=reuse_note_within_mins,
                now_fn=now_fn,
            )

    dives = await asyncio.gather(*(_run(a) for a in items), return_exceptions=False)
    for d in dives:
        out.dives.append(d)
        out.spent_usd += d.spent_usd
        if d.skipped_reason:
            out.skipped += 1
        elif d.research_note_id is not None:
            out.enriched += 1
        else:
            out.failed += 1
    return out


async def run_forever(
    *,
    mcp: AlertDiveMcp,
    llm: LlmClient,
    cfg: AgentConfig,
    system_prompt: str,
    poll_interval_secs: int = DEFAULT_POLL_INTERVAL_SECS,
    per_alert_usd: float = DEFAULT_PER_ALERT_USD,
    max_concurrent: int = DEFAULT_MAX_CONCURRENT,
    batch_limit: int = DEFAULT_BATCH_LIMIT,
    stop_event: asyncio.Event | None = None,
) -> None:
    """Continuous polling loop. Cancels cleanly on `stop_event` or task cancel."""
    log.info(
        "alert_dive starting (interval=%ss, per_alert=$%.2f, concurrent=%d)",
        poll_interval_secs,
        per_alert_usd,
        max_concurrent,
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
                per_alert_usd=per_alert_usd,
                max_concurrent=max_concurrent,
                batch_limit=batch_limit,
            )
            if tick.polled:
                log.info(
                    "tick: polled=%d enriched=%d skipped=%d failed=%d spent=$%.4f",
                    tick.polled,
                    tick.enriched,
                    tick.skipped,
                    tick.failed,
                    tick.spent_usd,
                )
        except Exception:  # noqa: BLE001
            log.exception("alert_dive tick failed; sleeping then retrying")
        # Sleep with cancellation responsiveness.
        if stop_event:
            try:
                await asyncio.wait_for(stop_event.wait(), timeout=poll_interval_secs)
                return
            except asyncio.TimeoutError:
                pass
        else:
            await asyncio.sleep(poll_interval_secs)


# ---- Per-alert dive ----------------------------------------------------------


async def _dive_one(
    *,
    mcp: AlertDiveMcp,
    llm: LlmClient,
    cfg: AgentConfig,
    system_prompt: str,
    alert: Mapping[str, Any],
    per_alert_usd: float,
    reuse_note_within_mins: int,
    now_fn,
) -> DiveResult:
    alert_id = int(alert.get("id") or 0)
    symbol = _alert_symbol(alert)
    if alert_id <= 0 or not symbol:
        return DiveResult(
            alert_id=alert_id,
            symbol=symbol,
            skipped_reason="alert missing id or symbol",
        )

    # Symbol-correlated alerts: if a fresh research_note already exists
    # for this symbol, reuse it instead of redoing the work. Best-effort —
    # if the lookup tool isn't on the server, fall through to a full dive.
    reuse_id = await _recent_note_for_symbol(
        mcp,
        symbol,
        within=timedelta(minutes=reuse_note_within_mins),
        now=now_fn(),
    )
    if reuse_id is not None:
        try:
            await mcp.mark_alert_enriched(alert_id=alert_id, research_note_id=reuse_id)
        except Exception:  # noqa: BLE001
            log.exception("mark_alert_enriched(reuse) failed for alert#%s", alert_id)
            return DiveResult(alert_id=alert_id, symbol=symbol)
        log.info(
            "alert#%s (%s): reused recent note#%s within %dm window",
            alert_id,
            symbol,
            reuse_id,
            reuse_note_within_mins,
        )
        return DiveResult(alert_id=alert_id, symbol=symbol, research_note_id=reuse_id)

    # Per-alert budget guard. The global one already passed at tick start.
    guard = bg.BudgetGuard(
        per_loop_usd=per_alert_usd,
        abort_if_global_spend_above=cfg.budget.abort_if_global_spend_above,
    )

    # Gather inputs concurrently.
    bundle, news, sentiment, setups = await _gather_for_alert(mcp, symbol)

    body = _format_alert_block(alert, symbol, bundle, news, sentiment, setups)

    try:
        guard.ensure_can_spend()
    except bg.BudgetExceeded as e:
        return DiveResult(
            alert_id=alert_id,
            symbol=symbol,
            skipped_reason=f"per-alert budget: {e}",
        )

    user_msg = (
        f"Alert #{alert_id} ({alert.get('kind')}): synthesise a deep-dive "
        f"research note for {symbol}. Use the `write_research_note` tool "
        f"exactly once. The note's evidence_refs MUST include "
        f"`{{\"type\": \"alert\", \"id\": {alert_id}}}` plus citations of "
        f"any news / setup ids you reference. Be honest about C-grade "
        f"setups — do not inflate conviction.\n\n"
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
        log.exception("LLM call failed for alert#%s", alert_id)
        return DiveResult(alert_id=alert_id, symbol=symbol, skipped_reason=f"llm error: {e}")

    cost = guard.record(
        cfg.models.smart,
        resp.input_tokens,
        resp.output_tokens,
        envelope_cost_usd=resp.cost_usd,
    )

    if not resp.tool_uses:
        return DiveResult(
            alert_id=alert_id,
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="LLM produced no tool_use",
        )

    payload = resp.tool_uses[0].input
    body_md = (payload.get("body_md") or "").strip()
    conviction = payload.get("conviction") or "C"
    refs = payload.get("evidence_refs") or []
    if not body_md:
        return DiveResult(
            alert_id=alert_id,
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="empty body_md",
        )

    setup_id = alert.get("setup_id")
    refs = _ensure_alert_ref(refs, alert_id)

    try:
        note = await mcp.write_research_note(
            symbol=symbol,
            body_md=body_md,
            conviction=conviction,
            evidence_refs=refs,
            alert_id=alert_id,
            setup_id=int(setup_id) if isinstance(setup_id, int) else None,
        )
    except McpToolError:
        log.exception("write_research_note failed for alert#%s", alert_id)
        return DiveResult(
            alert_id=alert_id,
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="write_research_note failed",
        )

    note_id = note.get("id") if isinstance(note, Mapping) else None
    if not isinstance(note_id, int):
        return DiveResult(
            alert_id=alert_id,
            symbol=symbol,
            spent_usd=cost,
            skipped_reason="note returned without id",
        )

    try:
        await mcp.mark_alert_enriched(alert_id=alert_id, research_note_id=note_id)
    except Exception:  # noqa: BLE001
        log.exception("mark_alert_enriched failed for alert#%s", alert_id)
        # Note still landed; surface as enriched but flag the marker miss.
        return DiveResult(
            alert_id=alert_id,
            symbol=symbol,
            research_note_id=note_id,
            spent_usd=cost,
            skipped_reason="mark_alert_enriched failed (will retry next tick)",
        )

    log.info(
        "alert#%s (%s): wrote note#%s conviction=%s spent=$%.4f",
        alert_id,
        symbol,
        note_id,
        conviction,
        cost,
    )
    return DiveResult(
        alert_id=alert_id,
        symbol=symbol,
        research_note_id=note_id,
        spent_usd=cost,
    )


# ---- Helpers -----------------------------------------------------------------


def _utc_now() -> datetime:
    return datetime.now(tz=timezone.utc)


def _items_from_envelope(raw: Any) -> list[Mapping[str, Any]]:
    """The Rust read tools wrap top-level arrays as `{ items, count }`. Some
    fake clients return a bare list; accept both shapes.
    """
    if isinstance(raw, Mapping) and "items" in raw:
        items = raw["items"]
        return list(items) if isinstance(items, list) else []
    if isinstance(raw, list):
        return list(raw)
    return []


def _alert_symbol(alert: Mapping[str, Any]) -> str | None:
    payload = alert.get("payload") or {}
    if isinstance(payload, Mapping):
        sym = payload.get("symbol")
        if isinstance(sym, str) and sym.strip():
            return sym.upper()
    sym = alert.get("symbol")
    if isinstance(sym, str) and sym.strip():
        return sym.upper()
    return None


def _ensure_alert_ref(refs: Sequence[Any], alert_id: int) -> list[dict[str, Any]]:
    out: list[dict[str, Any]] = []
    found = False
    for r in refs:
        if not isinstance(r, Mapping):
            continue
        out.append(dict(r))
        if r.get("type") == "alert" and r.get("id") == alert_id:
            found = True
    if not found:
        out.append({"type": "alert", "id": alert_id})
    return out


async def _gather_for_alert(mcp: AlertDiveMcp, symbol: str):
    since_setups = _utc_now() - timedelta(days=90)
    since_24h = _utc_now() - timedelta(hours=24 * 7)

    # Each call may fail independently — gather with return_exceptions and
    # treat misses as empty rather than aborting the whole dive.
    daily, intraday, fund, news, sentiment, setups = await asyncio.gather(
        mcp.get_bars(symbol, "1d", 252),
        mcp.get_bars(symbol, "5m", 78),
        mcp.get_fundamentals(symbol),
        mcp.get_news(symbol, max_age_secs=7 * 24 * 3600),
        mcp.get_sentiment(symbol, since=since_24h),
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
        sentiment_summary=ds.summarize_sentiment(_ok_list(sentiment)),
        setups_summary=ds.summarize_setups(_ok_list(setups)),
    )
    return bundle, _ok_list(news), _ok_list(sentiment), _ok_list(setups)


def _format_alert_block(
    alert: Mapping[str, Any],
    symbol: str,
    bundle: ds.CandidateBundle,
    news: Sequence[Mapping[str, Any]],
    sentiment: Sequence[Mapping[str, Any]],
    setups: Sequence[Mapping[str, Any]],
) -> str:
    payload = alert.get("payload") or {}
    payload_bits = ", ".join(
        f"{k}={v}" for k, v in (payload.items() if isinstance(payload, Mapping) else [])
    )
    return (
        f"ALERT METADATA\n"
        f"- alert_id: {alert.get('id')}\n"
        f"- kind: {alert.get('kind')}\n"
        f"- fired_at: {alert.get('fired_at')}\n"
        f"- payload: {payload_bits or '(empty)'}\n\n"
        f"CONTEXT\n"
        f"{bundle.as_prompt_block()}"
    )


async def _recent_note_for_symbol(
    mcp: AlertDiveMcp,
    symbol: str,
    *,
    within: timedelta,
    now: datetime,
) -> int | None:
    """Look up the most recent research_note for `symbol` written within
    `within`. Returns the note id, or None if no recent note exists or the
    server doesn't expose the lookup yet (then a full dive runs).
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
#
# Defined as `Any` for runtime — Python typing isn't enforced. Documented
# here so test fakes know which methods to implement.

class AlertDiveMcp:  # pragma: no cover — typing-only marker
    """Methods the alert-dive loop expects on its MCP client.

    Production: `mcp_client.McpClient` extended with the two Phase-6
    tool wrappers below. Tests pass a fake mirroring this surface.
    """

    async def get_alerts(self, *, unenriched_only: bool, limit: int) -> Any: ...
    async def get_llm_budget_status(self) -> Any: ...
    async def get_bars(self, symbol: str, bar_size: str, lookback_days: int) -> Any: ...
    async def get_fundamentals(self, symbol: str) -> Any: ...
    async def get_news(self, symbol: str, max_age_secs: int | None = None) -> Any: ...
    async def get_sentiment(self, symbol: str, *, since: datetime | None = None) -> Any: ...
    async def get_setups(self, *, symbol: str | None = None, since: datetime | None = None) -> Any: ...
    async def write_research_note(
        self,
        *,
        symbol: str,
        body_md: str,
        conviction: str,
        evidence_refs: Sequence[Mapping[str, Any]],
        alert_id: int | None = None,
        setup_id: int | None = None,
    ) -> Mapping[str, Any]: ...
    async def mark_alert_enriched(
        self,
        *,
        alert_id: int,
        research_note_id: int | None,
    ) -> Mapping[str, Any]: ...


# ---- CLI ---------------------------------------------------------------------


def _read_system_prompt() -> str:
    p = Path(__file__).resolve().parent / "prompts" / "alert_dive.md"
    if p.exists():
        return p.read_text(encoding="utf-8")
    # Fall back to a minimal inline prompt rather than crashing when the
    # repo is shipped without the optional prompt file.
    return (
        "You are an equity research analyst writing per-alert deep-dive "
        "notes for a single trader. Be specific, cite evidence, and grade "
        "conviction A/B/C honestly. Do not give financial advice."
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
        # Adapt the production McpClient to the AlertDiveMcp protocol by
        # wiring the two Phase-6 tools as inline closures. We could push
        # them into mcp_client.py, but keeping them here means the
        # morning-sweep client API stays minimal.
        adapter = _ProdAdapter(mcp)
        llm: LlmClient = make_llm_client(cfg.llm_backend)

        if args.once:
            tick = await run_tick(
                mcp=adapter,
                llm=llm,
                cfg=cfg,
                system_prompt=sys_prompt,
                per_alert_usd=args.per_alert_usd,
                max_concurrent=args.concurrent,
                batch_limit=args.batch_limit,
            )
            log.info("tick result: %s", tick)
            return 0

        await run_forever(
            mcp=adapter,
            llm=llm,
            cfg=cfg,
            system_prompt=sys_prompt,
            poll_interval_secs=args.interval,
            per_alert_usd=args.per_alert_usd,
            max_concurrent=args.concurrent,
            batch_limit=args.batch_limit,
        )
    return 0


class _ProdAdapter:
    """Wraps `McpClient` with the two Phase-6 tool calls + delegated reads."""

    def __init__(self, mcp: McpClient) -> None:
        self._mcp = mcp

    async def get_alerts(self, *, unenriched_only: bool, limit: int) -> Any:
        return await self._mcp.call_tool(
            "get_alerts",
            {"unenriched_only": unenriched_only, "limit": limit},
        )

    async def get_llm_budget_status(self) -> Any:
        return await self._mcp.get_llm_budget_status()

    async def get_bars(self, symbol: str, bar_size: str, lookback_days: int) -> Any:
        return await self._mcp.get_bars(symbol, bar_size, lookback_days)

    async def get_fundamentals(self, symbol: str) -> Any:
        return await self._mcp.get_fundamentals(symbol)

    async def get_news(self, symbol: str, max_age_secs: int | None = None) -> Any:
        return await self._mcp.get_news(symbol, max_age_secs=max_age_secs)

    async def get_sentiment(self, symbol: str, *, since: datetime | None = None) -> Any:
        return await self._mcp.get_sentiment(symbol, since=since)

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
        alert_id: int | None = None,
        setup_id: int | None = None,
    ) -> Mapping[str, Any]:
        args: dict[str, Any] = {
            "symbol": symbol,
            "body_md": body_md,
            "conviction": conviction,
            "evidence_refs": list(evidence_refs),
        }
        if alert_id is not None:
            args["alert_id"] = alert_id
        if setup_id is not None:
            args["setup_id"] = setup_id
        return await self._mcp.call_tool("write_research_note", args)

    async def mark_alert_enriched(
        self,
        *,
        alert_id: int,
        research_note_id: int | None,
    ) -> Mapping[str, Any]:
        args: dict[str, Any] = {"alert_id": alert_id}
        if research_note_id is not None:
            args["research_note_id"] = research_note_id
        return await self._mcp.call_tool("mark_alert_enriched", args)


def main() -> int:
    parser = argparse.ArgumentParser(description="Quantum Kapital alert-dive agent")
    parser.add_argument("--config", help="Path to config.toml (defaults to ./config.toml)")
    parser.add_argument(
        "--once",
        action="store_true",
        help="Run a single tick and exit (vs. continuous polling)",
    )
    parser.add_argument(
        "--interval",
        type=int,
        default=DEFAULT_POLL_INTERVAL_SECS,
        help=f"Poll interval seconds (default {DEFAULT_POLL_INTERVAL_SECS})",
    )
    parser.add_argument(
        "--per-alert-usd",
        type=float,
        default=DEFAULT_PER_ALERT_USD,
        help=f"Per-alert USD cap (default ${DEFAULT_PER_ALERT_USD:.2f})",
    )
    parser.add_argument(
        "--concurrent",
        type=int,
        default=DEFAULT_MAX_CONCURRENT,
        help=f"Max concurrent dives (default {DEFAULT_MAX_CONCURRENT})",
    )
    parser.add_argument(
        "--batch-limit",
        type=int,
        default=DEFAULT_BATCH_LIMIT,
        help=f"Max alerts pulled per tick (default {DEFAULT_BATCH_LIMIT})",
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
