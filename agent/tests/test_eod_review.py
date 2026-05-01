"""Tests for the EOD review agent loop."""

from __future__ import annotations

from dataclasses import dataclass, field
from datetime import date
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
from eod_review import (
    DEFAULT_SECTION,
    previous_trading_day,
    run_eod_review,
)
from llm import LlmResponse


# ---- Fakes ------------------------------------------------------------------


@dataclass
class FakeEodMcp:
    responses: dict[str, Any] = field(default_factory=dict)
    calls: list[tuple[str, dict[str, Any]]] = field(default_factory=list)

    async def get_llm_budget_status(self) -> dict[str, Any]:
        self.calls.append(("get_llm_budget_status", {}))
        return self.responses.get(
            "get_llm_budget_status",
            {"daily_usd_cap": 5.0, "daily_usd_spent": 0.10},
        )

    async def get_morning_pack(self, *, date_iso: str) -> Any:
        self.calls.append(("get_morning_pack", {"date": date_iso}))
        return self.responses.get("get_morning_pack", {})

    async def get_outcomes(
        self, *, since_iso: str, eval_window_days: int = 1
    ) -> Any:
        self.calls.append(
            (
                "get_outcomes",
                {"since": since_iso, "eval_window_days": eval_window_days},
            )
        )
        return self.responses.get("get_outcomes", {"items": [], "count": 0})

    async def append_journal_entry(
        self, *, date_iso: str, section: str, body_md: str
    ) -> Mapping[str, Any]:
        self.calls.append(
            (
                "append_journal_entry",
                {"date": date_iso, "section": section, "body_md": body_md},
            )
        )
        return self.responses.get(
            "append_journal_entry",
            {"entry_id": 1, "date": date_iso, "section": section},
        )


@dataclass
class FakeLlm:
    responses: list[LlmResponse] = field(default_factory=list)
    recorded: list[dict[str, Any]] = field(default_factory=list)

    async def call(
        self,
        *,
        model: str,
        system: str,
        messages: Iterable[Mapping[str, Any]],
        tools: Iterable[Mapping[str, Any]] | None = None,
        tool_choice: Mapping[str, Any] | None = None,
        max_tokens: int = 2048,
    ) -> LlmResponse:
        self.recorded.append(
            {
                "model": model,
                "system": system,
                "messages": list(messages),
                "tools": list(tools) if tools else None,
                "tool_choice": dict(tool_choice) if tool_choice else None,
                "max_tokens": max_tokens,
            }
        )
        if not self.responses:
            raise AssertionError("FakeLlm ran out of responses")
        return self.responses.pop(0)


def text_response(text: str, input_tokens: int = 800, output_tokens: int = 400) -> LlmResponse:
    return LlmResponse(
        text=text,
        tool_uses=[],
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        stop_reason="end_turn",
        raw=None,
    )


def make_cfg() -> AgentConfig:
    return AgentConfig(
        budget=BudgetConfig(per_loop_usd=0.50, abort_if_global_spend_above=0.95),
        universe=UniverseConfig(top_k=10, candidate_min_score=0.0, setups_lookback_days=90),
        output=OutputConfig(min_ideas=3, max_ideas=5),
        models=ModelsConfig(fast="claude-haiku-4-5", smart="claude-sonnet-4-6"),
        mcp=McpConfig(server_bin="x", socket_path=None),
    )


# ---- previous_trading_day ---------------------------------------------------


def test_previous_trading_day_skips_weekends():
    # 2026-05-04 is Mon; previous trading day is Fri 2026-05-01.
    assert previous_trading_day(date(2026, 5, 4)) == date(2026, 5, 1)


def test_previous_trading_day_basic_weekday():
    # 2026-05-05 Tue → 2026-05-04 Mon
    assert previous_trading_day(date(2026, 5, 5)) == date(2026, 5, 4)


def test_previous_trading_day_skips_holiday():
    # 2026-05-26 is the day after Memorial Day (2026-05-25). Tue → previous
    # trading day must be Friday 2026-05-22 (skipping holiday + weekend).
    assert previous_trading_day(date(2026, 5, 26)) == date(2026, 5, 22)


# ---- run_eod_review ---------------------------------------------------------


@pytest.mark.asyncio
async def test_run_eod_review_writes_journal_with_pack_and_outcomes():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "written_by": "agent_morning_sweep",
                "written_at": 0,
                "ranked_ideas": [
                    {
                        "symbol": "TSLA",
                        "thesis_md": "breakout thesis",
                        "conviction": "A",
                        "entry_zone": "100-105",
                        "invalidation": "close < 95",
                    },
                    {
                        "symbol": "AAPL",
                        "thesis_md": "fade thesis",
                        "conviction": "B",
                        "entry_zone": "180-185",
                        "invalidation": "close < 175",
                    },
                ],
            },
            "get_outcomes": {
                "items": [
                    {
                        "pack_date": "2026-05-01",
                        "symbol": "TSLA",
                        "outcome_class": "hit_entry",
                        "conviction": "A",
                        "realized_high": 107.0,
                        "realized_low": 99.0,
                        "realized_close": 103.0,
                        "thesis_md": "breakout thesis",
                    },
                    {
                        "pack_date": "2026-05-01",
                        "symbol": "AAPL",
                        "outcome_class": "drifted",
                        "conviction": "B",
                        "realized_high": 178.0,
                        "realized_low": 175.5,
                        "realized_close": 176.0,
                        "thesis_md": "fade thesis",
                    },
                ],
                "count": 2,
            },
        }
    )
    llm = FakeLlm(responses=[text_response("- TSLA hit entry as planned\n- AAPL drifted")])

    today = date(2026, 5, 4)
    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=today,
    )

    assert result.skipped_reason is None
    assert result.journal_date == "2026-05-04"
    assert result.pack_date == "2026-05-01"
    assert result.predictions_considered == 2
    assert result.outcomes_scored == 2
    assert result.body_len > 0

    # Pack lookup used previous trading day.
    pack_calls = [c for c in mcp.calls if c[0] == "get_morning_pack"]
    assert pack_calls and pack_calls[0][1]["date"] == "2026-05-01"

    # Journal write happened for today.
    write_calls = [c for c in mcp.calls if c[0] == "append_journal_entry"]
    assert len(write_calls) == 1
    assert write_calls[0][1]["date"] == "2026-05-04"
    assert write_calls[0][1]["section"] == DEFAULT_SECTION
    assert "TSLA" in write_calls[0][1]["body_md"]

    # User message rendered both predictions + outcome counts.
    user_msg = llm.recorded[0]["messages"][0]["content"]
    assert "TSLA" in user_msg
    assert "AAPL" in user_msg
    assert "hit_entry" in user_msg
    assert "drifted" in user_msg
    assert "OUTCOME COUNTS" in user_msg


@pytest.mark.asyncio
async def test_run_eod_review_no_pack_writes_placeholder_entry():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "written_by": None,
                "written_at": None,
                "ranked_ideas": [],
            }
        }
    )
    llm = FakeLlm()  # never called

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
    )

    assert result.skipped_reason == "no morning pack to score"
    assert result.predictions_considered == 0
    assert result.outcomes_scored == 0

    write_calls = [c for c in mcp.calls if c[0] == "append_journal_entry"]
    assert len(write_calls) == 1
    body = write_calls[0][1]["body_md"]
    assert "No morning pack" in body
    assert llm.recorded == [], "LLM should not have been called"


@pytest.mark.asyncio
async def test_run_eod_review_aborts_when_global_budget_exhausted():
    mcp = FakeEodMcp(
        responses={
            "get_llm_budget_status": {
                "daily_usd_cap": 5.0,
                "daily_usd_spent": 5.00,
            },
        }
    )
    llm = FakeLlm()

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
    )

    assert result.skipped_reason is not None
    assert result.body_len == 0
    # No pack/outcomes/journal calls — aborted before that.
    assert all(
        c[0] not in ("get_morning_pack", "get_outcomes", "append_journal_entry")
        for c in mcp.calls
    )
    assert llm.recorded == []


@pytest.mark.asyncio
async def test_run_eod_review_dry_run_skips_journal_write():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [
                    {
                        "symbol": "TSLA",
                        "thesis_md": "x",
                        "conviction": "A",
                        "entry_zone": "100-105",
                        "invalidation": "close < 95",
                    }
                ],
            },
            "get_outcomes": {"items": [], "count": 0},
        }
    )
    llm = FakeLlm(responses=[text_response("dry-run body")])

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
        dry_run=True,
    )

    assert result.skipped_reason is None
    assert result.body_len > 0
    # Crucially: no journal write.
    assert all(c[0] != "append_journal_entry" for c in mcp.calls)


@pytest.mark.asyncio
async def test_run_eod_review_empty_llm_body_skips_write():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [
                    {
                        "symbol": "TSLA",
                        "thesis_md": "x",
                        "conviction": "A",
                    }
                ],
            },
        }
    )
    llm = FakeLlm(responses=[text_response("   \n  ")])

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
    )
    assert result.skipped_reason == "empty LLM body"
    assert all(c[0] != "append_journal_entry" for c in mcp.calls)


@pytest.mark.asyncio
async def test_run_eod_review_filters_outcomes_to_target_pack_date():
    # get_outcomes may legitimately return earlier dates if the table
    # has them. We only score yesterday's pack — older rows must not
    # bleed into the prompt.
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [
                    {"symbol": "TSLA", "thesis_md": "x", "conviction": "A"}
                ],
            },
            "get_outcomes": {
                "items": [
                    {
                        "pack_date": "2026-04-30",  # earlier — must filter out
                        "symbol": "OLD",
                        "outcome_class": "drifted",
                    },
                    {
                        "pack_date": "2026-05-01",
                        "symbol": "TSLA",
                        "outcome_class": "hit_entry",
                    },
                ],
                "count": 2,
            },
        }
    )
    llm = FakeLlm(responses=[text_response("body")])

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
    )

    assert result.outcomes_scored == 1
    user_msg = llm.recorded[0]["messages"][0]["content"]
    assert "OLD" not in user_msg
    assert "TSLA" in user_msg
