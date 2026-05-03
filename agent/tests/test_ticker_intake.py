"""Ticker-intake loop tests with fakes for MCP + Anthropic.

Verifies the orchestration shape: poll watchlist → filter to primed +
note-less → gather → synthesise → write_research_note. No real network
or subprocess.
"""

from __future__ import annotations

import re
from dataclasses import dataclass, field
from datetime import datetime, timedelta, timezone
from pathlib import Path
from typing import Any, Iterable, Mapping

import pytest

from config import (
    AgentConfig,
    BudgetConfig,
    McpConfig,
    ModelsConfig,
    OutputConfig,
    UniverseConfig,
)
from ticker_intake import (
    GLOBAL_RESERVE_FRAC,
    SeenCache,
    WRITER,
    run_tick,
)

from tests.conftest import FakeLlmClient, make_tool_response


def _cfg(*, abort_above: float = 0.50) -> AgentConfig:
    return AgentConfig(
        budget=BudgetConfig(per_loop_usd=0.50, abort_if_global_spend_above=abort_above),
        universe=UniverseConfig(top_k=5, candidate_min_score=0.0, setups_lookback_days=30),
        output=OutputConfig(min_ideas=3, max_ideas=5),
        models=ModelsConfig(fast="claude-haiku-4-5", smart="claude-sonnet-4-6"),
        mcp=McpConfig(server_bin="./not-used-in-tests", socket_path=None),
    )


@dataclass
class FakeIntakeMcp:
    """Mirrors the protocol the ticker-intake loop expects on its MCP client."""

    watchlist: list[Mapping[str, Any]] = field(default_factory=list)
    budget_status: Mapping[str, Any] = field(
        default_factory=lambda: {"daily_usd_cap": 5.0, "daily_usd_spent": 0.10}
    )
    bars: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)
    fundamentals: dict[str, Mapping[str, Any]] = field(default_factory=dict)
    news: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)
    setups: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)

    next_note_id: int = 200
    write_research_note_calls: list[dict[str, Any]] = field(default_factory=list)

    async def get_watchlist(self) -> Mapping[str, Any]:
        return {"items": list(self.watchlist), "count": len(self.watchlist)}

    async def get_llm_budget_status(self) -> Mapping[str, Any]:
        return dict(self.budget_status)

    async def get_bars(self, symbol: str, bar_size: str, lookback_days: int):
        return list(self.bars.get(symbol, []))

    async def get_fundamentals(self, symbol: str):
        return dict(self.fundamentals.get(symbol, {}))

    async def get_news(self, symbol: str, max_age_secs: int | None = None):
        return list(self.news.get(symbol, []))

    async def get_setups(
        self,
        *,
        symbol: str | None = None,
        since: datetime | None = None,
    ):
        return list(self.setups.get(symbol or "", []))

    async def write_research_note(
        self,
        *,
        symbol: str,
        body_md: str,
        conviction: str,
        evidence_refs: Iterable[Mapping[str, Any]],
        written_by: str,
    ):
        nid = self.next_note_id
        self.next_note_id += 1
        call = {
            "id": nid,
            "symbol": symbol,
            "body_md": body_md,
            "conviction": conviction,
            "evidence_refs": list(evidence_refs),
            "written_by": written_by,
        }
        self.write_research_note_calls.append(call)
        return {"id": nid, "symbol": symbol}


@pytest.fixture
def fake_intake_mcp() -> FakeIntakeMcp:
    return FakeIntakeMcp()


def _row(
    symbol: str,
    *,
    primed: bool = True,
    primed_at: datetime | None = None,
) -> dict[str, Any]:
    """Watchlist row shape compatible with the agent's `_is_primed` /
    `_symbol_of` predicates."""
    if primed_at is None and primed:
        primed_at = datetime(2026, 5, 3, 12, 0, tzinfo=timezone.utc)
    return {
        "symbol": symbol,
        "source": "manual",
        "status": "watching",
        "tags": [],
        "notes": "added by user",
        "added_at": "2026-05-03T11:55:00Z",
        "last_primed_at": primed_at.isoformat() if primed and primed_at else None,
    }


def _seed_market(mcp: FakeIntakeMcp, symbol: str) -> None:
    mcp.bars[symbol] = [
        {"close": 100 + i * 0.1, "high": 101, "low": 99} for i in range(252)
    ]
    mcp.fundamentals[symbol] = {"Sector": "Tech"}
    mcp.news[symbol] = [{"id": 1, "verdict": "bullish", "title": "Earnings beat"}]
    mcp.setups[symbol] = []


def _ok_tool_response() -> Any:
    return make_tool_response(
        tool_name="write_research_note",
        tool_input={
            "body_md": (
                "Baseline thesis for the symbol. Earnings beat; tight base; "
                "volume profile constructive. More than fifty characters of body."
            ),
            "conviction": "B",
            "evidence_refs": [{"type": "news", "cache_id": 1}],
        },
        input_tokens=1500,
        output_tokens=400,
    )


@pytest.mark.asyncio
async def test_happy_path_writes_baseline_note(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    fake_intake_mcp.watchlist = [_row("AAPL")]
    _seed_market(fake_intake_mcp, "AAPL")
    fake_llm.responses = [_ok_tool_response()]

    seen = SeenCache()
    tick = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=seen,
        per_symbol_usd=0.10,
        max_concurrent=1,
    )

    assert tick.polled == 1
    assert tick.eligible == 1
    assert tick.written == 1
    assert tick.skipped == 0
    assert tick.failed == 0

    assert len(fake_intake_mcp.write_research_note_calls) == 1
    call = fake_intake_mcp.write_research_note_calls[0]
    assert call["symbol"] == "AAPL"
    assert call["written_by"] == "agent.ticker_intake"
    assert call["written_by"] == WRITER
    assert call["conviction"] == "B"


@pytest.mark.asyncio
async def test_unprimed_rows_are_skipped_without_llm_call(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    fake_intake_mcp.watchlist = [_row("AAPL", primed=False)]

    tick = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=SeenCache(),
    )

    assert tick.polled == 1
    assert tick.eligible == 0
    assert tick.written == 0
    assert len(fake_llm.recorded) == 0
    assert len(fake_intake_mcp.write_research_note_calls) == 0


@pytest.mark.asyncio
async def test_seen_cache_short_circuits_within_window(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    fake_intake_mcp.watchlist = [_row("AAPL")]
    _seed_market(fake_intake_mcp, "AAPL")
    fake_llm.responses = [_ok_tool_response()]

    seen = SeenCache()
    # First tick writes the note and stamps the cache.
    tick1 = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=seen,
    )
    assert tick1.written == 1

    # Second tick within the reuse window should be a complete no-op.
    tick2 = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=seen,
    )
    assert tick2.polled == 1
    assert tick2.eligible == 0
    assert tick2.written == 0
    assert len(fake_intake_mcp.write_research_note_calls) == 1


@pytest.mark.asyncio
async def test_recent_note_lookup_short_circuits_when_available(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    """When the MCP adapter exposes `list_research_notes`, the loop uses
    it to skip symbols a previous daemon (or another writer) already
    covered. Mirrors the alert_dive optional-hook pattern."""
    fake_intake_mcp.watchlist = [_row("AAPL")]
    _seed_market(fake_intake_mcp, "AAPL")

    now = datetime.now(tz=timezone.utc)
    recent_ts = int(now.timestamp()) - 3 * 24 * 3600  # 3 days ago — within 7d window.

    async def _list_research_notes(
        *, symbol: str, limit: int = 1
    ) -> list[Mapping[str, Any]]:
        if symbol == "AAPL":
            return [{"id": 999, "written_at": recent_ts}]
        return []

    setattr(fake_intake_mcp, "list_research_notes", _list_research_notes)

    tick = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=SeenCache(),
    )

    assert tick.polled == 1
    assert tick.eligible == 0
    assert tick.written == 0
    assert len(fake_llm.recorded) == 0
    assert len(fake_intake_mcp.write_research_note_calls) == 0


@pytest.mark.asyncio
async def test_recent_note_lookup_outside_window_does_not_short_circuit(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    """A note older than the reuse window must NOT block the intake."""
    fake_intake_mcp.watchlist = [_row("AAPL")]
    _seed_market(fake_intake_mcp, "AAPL")
    fake_llm.responses = [_ok_tool_response()]

    now = datetime.now(tz=timezone.utc)
    stale_ts = int(now.timestamp()) - 30 * 24 * 3600  # 30 days ago.

    async def _list_research_notes(
        *, symbol: str, limit: int = 1
    ) -> list[Mapping[str, Any]]:
        return [{"id": 12, "written_at": stale_ts}]

    setattr(fake_intake_mcp, "list_research_notes", _list_research_notes)

    tick = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=SeenCache(),
    )
    assert tick.written == 1


@pytest.mark.asyncio
async def test_global_budget_above_cutoff_skips_all_candidates(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    fake_intake_mcp.budget_status = {"daily_usd_cap": 5.0, "daily_usd_spent": 4.75}
    fake_intake_mcp.watchlist = [_row("AAPL"), _row("MSFT")]
    _seed_market(fake_intake_mcp, "AAPL")
    _seed_market(fake_intake_mcp, "MSFT")

    tick = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(abort_above=0.50),
        system_prompt="SYSTEM",
        seen=SeenCache(),
    )

    assert tick.polled == 2
    assert tick.eligible == 2
    assert tick.skipped == 2
    assert tick.written == 0
    assert len(fake_llm.recorded) == 0
    assert len(fake_intake_mcp.write_research_note_calls) == 0
    assert GLOBAL_RESERVE_FRAC == pytest.approx(0.10)


@pytest.mark.asyncio
async def test_per_symbol_budget_records_spend(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    """Each successful intake records the call cost into the per-symbol
    `BudgetGuard` so a follow-up call within the same intake (none today)
    or a downstream eval can read non-zero spend. The global cutoff
    test covers the loop-level short-circuit; here we only need to
    confirm spend is materialised so the cap could ever bind."""
    fake_intake_mcp.watchlist = [_row("AAPL")]
    _seed_market(fake_intake_mcp, "AAPL")
    fake_llm.responses = [_ok_tool_response()]

    tick = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=SeenCache(),
        per_symbol_usd=0.10,
    )
    assert tick.written == 1
    assert tick.spent_usd > 0.0
    assert tick.intakes[0].spent_usd > 0.0


@pytest.mark.asyncio
async def test_no_tool_use_response_is_recorded_as_failed(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    """A malformed LLM response (no tool_use) must be skipped without
    crashing the tick or persisting a half-written note."""
    fake_intake_mcp.watchlist = [_row("AAPL")]
    _seed_market(fake_intake_mcp, "AAPL")
    from llm import LlmResponse

    fake_llm.responses = [
        LlmResponse(
            text="prose instead of a tool call",
            tool_uses=[],
            input_tokens=200,
            output_tokens=50,
            stop_reason="end_turn",
            raw=None,
        )
    ]

    tick = await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=SeenCache(),
    )
    assert tick.polled == 1
    assert tick.eligible == 1
    assert tick.skipped == 1
    assert tick.written == 0
    assert len(fake_intake_mcp.write_research_note_calls) == 0


@pytest.mark.asyncio
async def test_write_call_uses_writer_constant(
    fake_intake_mcp: FakeIntakeMcp, fake_llm: FakeLlmClient
) -> None:
    fake_intake_mcp.watchlist = [_row("AAPL")]
    _seed_market(fake_intake_mcp, "AAPL")
    fake_llm.responses = [_ok_tool_response()]

    await run_tick(
        mcp=fake_intake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        seen=SeenCache(),
    )
    call = fake_intake_mcp.write_research_note_calls[0]
    assert call["written_by"] == "agent.ticker_intake"


def test_writer_string_is_unique_in_module() -> None:
    """Grep discipline: `agent/ticker_intake.py` must contain the
    literal `"agent.ticker_intake"` and NO other agent writer string.
    Catches a copy-paste from `alert_dive.py` that forgot to rename."""
    src = (Path(__file__).resolve().parent.parent / "ticker_intake.py").read_text()
    assert "agent.ticker_intake" in src
    forbidden = re.findall(
        r'"agent[._](alert_dive|morning_sweep|eod_review)"', src
    )
    assert forbidden == [], f"unexpected writer strings in ticker_intake.py: {forbidden}"


def test_seen_cache_recent_predicate() -> None:
    cache = SeenCache()
    now = datetime(2026, 5, 3, 12, 0, tzinfo=timezone.utc)
    cache.mark("AAPL", now - timedelta(days=2))
    assert cache.recent("AAPL", within=timedelta(days=7), now=now) is True
    assert cache.recent("AAPL", within=timedelta(days=1), now=now) is False
    assert cache.recent("MSFT", within=timedelta(days=7), now=now) is False
    # Case-insensitive on symbol so a lowercase row from the watchlist
    # still hits the cache.
    cache.mark("tsla", now)
    assert cache.recent("TSLA", within=timedelta(days=1), now=now) is True
