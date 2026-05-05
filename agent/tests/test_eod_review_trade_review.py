"""Tests for the structured trade-review extension to `eod_review.py`.

The journal-entry path is covered in `test_eod_review.py`; these tests
focus on the second LLM call that scores actual fills via
`get_trade_legs` + `write_trade_review`.
"""

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
from eod_review import run_eod_review
from llm import LlmResponse, ToolUse


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

    async def get_trade_legs(
        self, *, date_iso: str, account: str | None = None
    ) -> Any:
        self.calls.append(
            ("get_trade_legs", {"date": date_iso, "account": account})
        )
        return self.responses.get(
            "get_trade_legs",
            {"date": date_iso, "account": "U-test", "legs": [], "totals": {}},
        )

    async def write_trade_review(
        self,
        *,
        date_iso: str,
        account: str,
        prompt_version: int,
        summary: Mapping[str, Any],
        behavioral_tags: list[str],
        leg_observations: list[Mapping[str, Any]],
        narrative_md: str,
        llm_call_id: str | None = None,
    ) -> Mapping[str, Any]:
        self.calls.append(
            (
                "write_trade_review",
                {
                    "date": date_iso,
                    "account": account,
                    "prompt_version": prompt_version,
                    "summary": dict(summary),
                    "behavioral_tags": list(behavioral_tags),
                    "leg_observations": [dict(o) for o in leg_observations],
                    "narrative_md": narrative_md,
                    "llm_call_id": llm_call_id,
                },
            )
        )
        return self.responses.get(
            "write_trade_review",
            {
                "date": date_iso,
                "account": account,
                "prompt_version": prompt_version,
                "grade": "B",
                "score": 12.5,
                "generated_at": "2026-05-04T22:00:00Z",
            },
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


def text_response(text: str) -> LlmResponse:
    return LlmResponse(
        text=text,
        tool_uses=[],
        input_tokens=800,
        output_tokens=400,
        stop_reason="end_turn",
        raw=None,
    )


def tool_response(tool_input: dict[str, Any]) -> LlmResponse:
    return LlmResponse(
        text="",
        tool_uses=[ToolUse(id="tu_1", name="submit_trade_review", input=tool_input)],
        input_tokens=1500,
        output_tokens=800,
        stop_reason="tool_use",
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


def round_trip_leg(leg_id: str, symbol: str, net: float, gross: float, commission: float) -> dict:
    return {
        "leg_id": leg_id,
        "symbol": symbol,
        "tags": ["round_trip"],
        "net_pnl": net,
        "gross_pnl": gross,
        "commission_total": commission,
        "opened_at": "2026-05-01T14:30:00Z",
        "closed_at": "2026-05-01T19:00:00Z",
        "hold_minutes": 270,
    }


# ---- Tests ------------------------------------------------------------------


@pytest.mark.asyncio
async def test_run_eod_review_writes_trade_review_when_legs_present():
    """End-to-end: journal entry + structured trade review written."""
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [
                    {"symbol": "TSLA", "thesis_md": "x", "conviction": "A"}
                ],
            },
            "get_outcomes": {"items": [], "count": 0},
            "get_trade_legs": {
                "date": "2026-05-01",
                "account": "U1234567",
                "legs": [
                    round_trip_leg("L-TSLA-1", "TSLA", 250.0, 260.0, 10.0),
                    round_trip_leg("L-AAPL-1", "AAPL", -75.0, -65.0, 10.0),
                ],
                "totals": {},
            },
        }
    )
    llm = FakeLlm(
        responses=[
            text_response("- TSLA hit entry"),  # journal entry
            tool_response(
                {
                    "behavioral_tags": ["flat_close", "discipline_on_loser"],
                    "leg_observations": [
                        {
                            "leg_id": "L-TSLA-1",
                            "observation_md": "Best leg of the day.",
                            "tag": "discipline_on_loser",
                        }
                    ],
                    "narrative_md": "Solid +$175 day; cut the AAPL loser fast.",
                }
            ),
        ]
    )

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
    )

    assert result.skipped_reason is None
    assert result.trade_review is not None
    tr_outcome = result.trade_review
    assert tr_outcome.legs_considered == 2
    assert tr_outcome.skipped_reason is None
    assert tr_outcome.grade == "B"
    assert tr_outcome.score == 12.5

    # Both LLM calls happened — first journal, second trade-review.
    assert len(llm.recorded) == 2
    journal_call = llm.recorded[0]
    trade_review_call = llm.recorded[1]
    # The trade-review call uses a forced tool.
    assert journal_call["tools"] is None
    assert trade_review_call["tools"] is not None
    assert trade_review_call["tool_choice"] == {
        "type": "tool",
        "name": "submit_trade_review",
    }

    # write_trade_review carried through expected args.
    write_calls = [c for c in mcp.calls if c[0] == "write_trade_review"]
    assert len(write_calls) == 1
    args = write_calls[0][1]
    assert args["date"] == "2026-05-01"
    assert args["account"] == "U1234567"
    assert args["prompt_version"] == 1
    assert args["behavioral_tags"] == ["flat_close", "discipline_on_loser"]
    assert "Solid +$175" in args["narrative_md"]
    # Summary computed client-side and forwarded.
    assert args["summary"]["net_pnl"] == 175.0
    assert args["summary"]["n_round_trips"] == 2
    # leg_observations carried through.
    assert args["leg_observations"][0]["leg_id"] == "L-TSLA-1"


@pytest.mark.asyncio
async def test_run_eod_review_skips_trade_review_when_no_legs():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [
                    {"symbol": "TSLA", "thesis_md": "x", "conviction": "A"}
                ],
            },
            "get_outcomes": {"items": [], "count": 0},
            # Default get_trade_legs response has empty legs.
        }
    )
    llm = FakeLlm(responses=[text_response("body")])  # only journal call

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
    )

    assert result.skipped_reason is None
    assert result.trade_review is not None
    assert result.trade_review.legs_considered == 0
    assert result.trade_review.skipped_reason == "no fills"

    # No second LLM call, no write_trade_review call.
    assert len(llm.recorded) == 1
    assert all(c[0] != "write_trade_review" for c in mcp.calls)


@pytest.mark.asyncio
async def test_run_eod_review_skip_trade_review_flag_bypasses_extension():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [
                    {"symbol": "TSLA", "thesis_md": "x", "conviction": "A"}
                ],
            },
            "get_outcomes": {"items": [], "count": 0},
            "get_trade_legs": {
                "date": "2026-05-01",
                "account": "U1",
                "legs": [round_trip_leg("L-1", "TSLA", 100.0, 110.0, 10.0)],
                "totals": {},
            },
        }
    )
    llm = FakeLlm(responses=[text_response("body")])  # journal only

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
        skip_trade_review=True,
    )

    assert result.skipped_reason is None
    assert result.trade_review is None  # extension bypassed entirely
    # No get_trade_legs / write_trade_review calls.
    assert all(
        c[0] not in ("get_trade_legs", "write_trade_review") for c in mcp.calls
    )


@pytest.mark.asyncio
async def test_run_eod_review_trade_review_skipped_when_llm_omits_tool_call():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [{"symbol": "TSLA", "thesis_md": "x", "conviction": "A"}],
            },
            "get_outcomes": {"items": [], "count": 0},
            "get_trade_legs": {
                "date": "2026-05-01",
                "account": "U1",
                "legs": [round_trip_leg("L-1", "TSLA", 100.0, 110.0, 10.0)],
                "totals": {},
            },
        }
    )
    llm = FakeLlm(
        responses=[
            text_response("journal body"),
            text_response("oops, forgot the tool"),  # no tool_use block
        ]
    )

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
    )

    assert result.trade_review is not None
    assert result.trade_review.skipped_reason == "LLM did not call submit_trade_review"
    assert all(c[0] != "write_trade_review" for c in mcp.calls)


@pytest.mark.asyncio
async def test_run_eod_review_dry_run_skips_trade_review_write():
    mcp = FakeEodMcp(
        responses={
            "get_morning_pack": {
                "date": "2026-05-01",
                "ranked_ideas": [{"symbol": "TSLA", "thesis_md": "x", "conviction": "A"}],
            },
            "get_outcomes": {"items": [], "count": 0},
            "get_trade_legs": {
                "date": "2026-05-01",
                "account": "U1",
                "legs": [round_trip_leg("L-1", "TSLA", 100.0, 110.0, 10.0)],
                "totals": {},
            },
        }
    )
    llm = FakeLlm(
        responses=[
            text_response("journal"),
            tool_response(
                {
                    "behavioral_tags": [],
                    "leg_observations": [],
                    "narrative_md": "ok day",
                }
            ),
        ]
    )

    result = await run_eod_review(
        mcp=mcp,
        llm=llm,
        cfg=make_cfg(),
        today=date(2026, 5, 4),
        dry_run=True,
    )

    # Trade-review LLM call still happens in dry-run; only the write is skipped.
    assert all(c[0] != "write_trade_review" for c in mcp.calls)
    assert all(c[0] != "append_journal_entry" for c in mcp.calls)
    assert result.trade_review is not None
    assert result.trade_review.skipped_reason == "dry-run"
