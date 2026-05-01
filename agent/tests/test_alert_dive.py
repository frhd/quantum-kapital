"""Alert-dive loop integration tests with fakes for MCP + Anthropic.

Verifies the orchestration shape: poll unenriched → gather → synthesise →
write_research_note → mark_alert_enriched. No real network or subprocess.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import datetime, timezone
from typing import Any, Iterable, Mapping

import pytest

from alert_dive import GLOBAL_RESERVE_FRAC, run_tick
from config import (
    AgentConfig,
    BudgetConfig,
    McpConfig,
    ModelsConfig,
    OutputConfig,
    UniverseConfig,
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
class FakeAlertDiveMcp:
    """Mirrors the protocol the alert-dive loop expects on its MCP client."""

    alerts: list[Mapping[str, Any]] = field(default_factory=list)
    budget_status: Mapping[str, Any] = field(
        default_factory=lambda: {"daily_usd_cap": 5.0, "daily_usd_spent": 0.10}
    )
    bars: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)
    fundamentals: dict[str, Mapping[str, Any]] = field(default_factory=dict)
    news: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)
    sentiment: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)
    setups: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)
    notes_by_symbol: dict[str, list[Mapping[str, Any]]] = field(default_factory=dict)

    next_note_id: int = 100
    write_research_note_calls: list[dict[str, Any]] = field(default_factory=list)
    mark_alert_enriched_calls: list[dict[str, Any]] = field(default_factory=list)
    enriched_ids: set[int] = field(default_factory=set)

    async def get_alerts(self, *, unenriched_only: bool, limit: int) -> Mapping[str, Any]:
        items = [a for a in self.alerts if int(a["id"]) not in self.enriched_ids]
        return {"items": items[:limit], "count": len(items[:limit])}

    async def get_llm_budget_status(self) -> Mapping[str, Any]:
        return dict(self.budget_status)

    async def get_bars(self, symbol: str, bar_size: str, lookback_days: int):
        return list(self.bars.get(symbol, []))

    async def get_fundamentals(self, symbol: str):
        return dict(self.fundamentals.get(symbol, {}))

    async def get_news(self, symbol: str, max_age_secs: int | None = None):
        return list(self.news.get(symbol, []))

    async def get_sentiment(self, symbol: str, *, since: datetime | None = None):
        return list(self.sentiment.get(symbol, []))

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
        alert_id: int | None = None,
        setup_id: int | None = None,
    ):
        nid = self.next_note_id
        self.next_note_id += 1
        call = {
            "id": nid,
            "symbol": symbol,
            "body_md": body_md,
            "conviction": conviction,
            "evidence_refs": list(evidence_refs),
            "alert_id": alert_id,
            "setup_id": setup_id,
        }
        self.write_research_note_calls.append(call)
        return {"id": nid, "symbol": symbol}

    async def mark_alert_enriched(
        self,
        *,
        alert_id: int,
        research_note_id: int | None,
    ):
        self.mark_alert_enriched_calls.append(
            {"alert_id": alert_id, "research_note_id": research_note_id}
        )
        self.enriched_ids.add(alert_id)
        return {"alert_id": alert_id, "research_note_id": research_note_id, "newly_marked": True}


@pytest.fixture
def fake_dive_mcp() -> FakeAlertDiveMcp:
    return FakeAlertDiveMcp()


def _alert(alert_id: int, symbol: str, kind: str = "detected") -> dict[str, Any]:
    return {
        "id": alert_id,
        "setup_id": alert_id * 10,
        "kind": kind,
        "fired_at": "2026-05-02T13:30:00Z",
        "payload": {"symbol": symbol, "trigger_price": 100.0},
    }


def _seed_market(mcp: FakeAlertDiveMcp, symbol: str) -> None:
    mcp.bars[symbol] = [
        {"close": 100 + i * 0.1, "high": 101, "low": 99} for i in range(252)
    ]
    mcp.fundamentals[symbol] = {"Sector": "Tech"}
    mcp.news[symbol] = [{"id": 1, "verdict": "bullish", "title": "X"}]
    mcp.sentiment[symbol] = []
    mcp.setups[symbol] = []


@pytest.mark.asyncio
async def test_happy_path_writes_note_and_marks_enriched(
    fake_dive_mcp: FakeAlertDiveMcp, fake_llm: FakeLlmClient
) -> None:
    fake_dive_mcp.alerts = [_alert(1, "AAPL")]
    _seed_market(fake_dive_mcp, "AAPL")
    fake_llm.responses = [
        make_tool_response(
            tool_name="write_research_note",
            tool_input={
                "body_md": "Earnings beat; tight base; volume profile constructive. 200+ words…",
                "conviction": "A",
                "evidence_refs": [{"type": "news", "id": 1}],
            },
            input_tokens=1500,
            output_tokens=400,
        )
    ]

    tick = await run_tick(
        mcp=fake_dive_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        per_alert_usd=0.05,
        max_concurrent=1,
        batch_limit=10,
    )

    assert tick.polled == 1
    assert tick.enriched == 1
    assert tick.skipped == 0
    assert tick.failed == 0

    # Note written with the alert ref auto-attached.
    assert len(fake_dive_mcp.write_research_note_calls) == 1
    call = fake_dive_mcp.write_research_note_calls[0]
    assert call["symbol"] == "AAPL"
    assert call["alert_id"] == 1
    refs = call["evidence_refs"]
    assert any(r.get("type") == "alert" and r.get("id") == 1 for r in refs)
    # Existing news ref preserved.
    assert any(r.get("type") == "news" and r.get("id") == 1 for r in refs)

    # Marker stamped with the new note id.
    assert fake_dive_mcp.mark_alert_enriched_calls == [
        {"alert_id": 1, "research_note_id": 100}
    ]


@pytest.mark.asyncio
async def test_skips_when_global_budget_above_cutoff(
    fake_dive_mcp: FakeAlertDiveMcp, fake_llm: FakeLlmClient
) -> None:
    # 95% used → above the 90% reserve floor regardless of cfg.
    fake_dive_mcp.budget_status = {"daily_usd_cap": 5.0, "daily_usd_spent": 4.75}
    fake_dive_mcp.alerts = [_alert(1, "AAPL"), _alert(2, "MSFT")]
    _seed_market(fake_dive_mcp, "AAPL")
    _seed_market(fake_dive_mcp, "MSFT")

    tick = await run_tick(
        mcp=fake_dive_mcp,
        llm=fake_llm,
        cfg=_cfg(abort_above=0.50),
        system_prompt="SYSTEM",
    )

    assert tick.polled == 2
    assert tick.skipped == 2
    assert tick.enriched == 0
    # No LLM calls made.
    assert len(fake_llm.recorded) == 0
    # Both rows stamped as skipped (no note id).
    assert len(fake_dive_mcp.mark_alert_enriched_calls) == 2
    for call in fake_dive_mcp.mark_alert_enriched_calls:
        assert call["research_note_id"] is None
    # And the floor we computed should match the constant.
    assert GLOBAL_RESERVE_FRAC == pytest.approx(0.10)


@pytest.mark.asyncio
async def test_no_alerts_no_calls(
    fake_dive_mcp: FakeAlertDiveMcp, fake_llm: FakeLlmClient
) -> None:
    tick = await run_tick(
        mcp=fake_dive_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
    )
    assert tick.polled == 0
    assert tick.enriched == 0
    assert len(fake_llm.recorded) == 0
    assert len(fake_dive_mcp.write_research_note_calls) == 0


@pytest.mark.asyncio
async def test_alert_missing_symbol_is_skipped_without_llm_call(
    fake_dive_mcp: FakeAlertDiveMcp, fake_llm: FakeLlmClient
) -> None:
    fake_dive_mcp.alerts = [{"id": 5, "kind": "detected", "payload": {}}]
    tick = await run_tick(
        mcp=fake_dive_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
    )
    assert tick.polled == 1
    assert tick.skipped == 1
    assert len(fake_llm.recorded) == 0
    assert len(fake_dive_mcp.write_research_note_calls) == 0


@pytest.mark.asyncio
async def test_burst_alerts_throttled_by_concurrency(
    fake_dive_mcp: FakeAlertDiveMcp, fake_llm: FakeLlmClient
) -> None:
    # 5 alerts, all distinct symbols.
    symbols = ["AAPL", "MSFT", "TSLA", "NVDA", "AMD"]
    for i, sym in enumerate(symbols, start=1):
        fake_dive_mcp.alerts.append(_alert(i, sym))
        _seed_market(fake_dive_mcp, sym)
    fake_llm.responses = [
        make_tool_response(
            tool_name="write_research_note",
            tool_input={
                "body_md": f"thesis for {sym} — extensive analysis here, more than fifty chars",
                "conviction": "B",
            },
        )
        for sym in symbols
    ]

    tick = await run_tick(
        mcp=fake_dive_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
        max_concurrent=2,
        batch_limit=10,
    )

    # All 5 alerts should be enriched — concurrency only bounds parallelism,
    # not throughput.
    assert tick.enriched == 5
    assert len(fake_dive_mcp.write_research_note_calls) == 5
    assert len(fake_dive_mcp.mark_alert_enriched_calls) == 5


@pytest.mark.asyncio
async def test_already_enriched_rows_not_repolled(
    fake_dive_mcp: FakeAlertDiveMcp, fake_llm: FakeLlmClient
) -> None:
    # Alert 1 already enriched on the server; only alert 2 should be polled.
    fake_dive_mcp.alerts = [_alert(1, "AAPL"), _alert(2, "MSFT")]
    fake_dive_mcp.enriched_ids.add(1)
    _seed_market(fake_dive_mcp, "MSFT")
    fake_llm.responses = [
        make_tool_response(
            tool_name="write_research_note",
            tool_input={
                "body_md": "msft thesis with at least fifty characters in the body markdown",
                "conviction": "B",
            },
        )
    ]

    tick = await run_tick(
        mcp=fake_dive_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
    )

    assert tick.polled == 1
    assert tick.enriched == 1
    # Only one mark — for alert 2.
    assert [c["alert_id"] for c in fake_dive_mcp.mark_alert_enriched_calls] == [2]


@pytest.mark.asyncio
async def test_recent_note_for_symbol_short_circuits_dive(
    fake_dive_mcp: FakeAlertDiveMcp, fake_llm: FakeLlmClient
) -> None:
    # Two alerts for the same symbol, separated in time. The first dive
    # writes a note; we patch the fake to expose `list_research_notes`
    # so the second call discovers the note and skips synthesis.
    fake_dive_mcp.alerts = [_alert(1, "AAPL")]
    _seed_market(fake_dive_mcp, "AAPL")

    now = datetime.now(tz=timezone.utc)

    # Pretend a note was written 5 minutes ago for AAPL.
    setattr(
        fake_dive_mcp,
        "list_research_notes",
        _make_list_notes({"AAPL": [{"id": 999, "written_at": int(now.timestamp()) - 300}]}),
    )

    tick = await run_tick(
        mcp=fake_dive_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        system_prompt="SYSTEM",
    )

    assert tick.enriched == 1
    # No LLM call needed — reused the existing note.
    assert len(fake_llm.recorded) == 0
    assert fake_dive_mcp.mark_alert_enriched_calls == [
        {"alert_id": 1, "research_note_id": 999}
    ]


def _make_list_notes(rows_by_symbol: dict[str, list[Mapping[str, Any]]]):
    async def _list_research_notes(*, symbol: str, limit: int = 1):
        return list(rows_by_symbol.get(symbol, []))[:limit]

    return _list_research_notes
