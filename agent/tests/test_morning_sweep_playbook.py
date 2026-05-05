"""Tests for the structured-playbook extension to `morning_sweep.py`.

The morning_pack happy path is covered in `test_morning_sweep.py`;
these tests focus on the second LLM call that produces structured
`ranked_setups` + `skip_list` and the `write_playbook` MCP rail.
"""

from __future__ import annotations

from datetime import date

import pytest

from config import (
    AgentConfig,
    BudgetConfig,
    McpConfig,
    ModelsConfig,
    OutputConfig,
    UniverseConfig,
)
from morning_sweep import run_sweep

from tests.conftest import FakeLlmClient, FakeMcpClient, make_tool_response


def _cfg() -> AgentConfig:
    return AgentConfig(
        budget=BudgetConfig(per_loop_usd=0.50, abort_if_global_spend_above=0.50),
        universe=UniverseConfig(top_k=5, candidate_min_score=0.1, setups_lookback_days=30),
        output=OutputConfig(min_ideas=3, max_ideas=5),
        models=ModelsConfig(fast="claude-haiku-4-5", smart="claude-sonnet-4-6"),
        mcp=McpConfig(server_bin="./not-used-in-tests", socket_path=None),
    )


def _stock_responses() -> dict:
    return {
        "get_llm_budget_status": {"daily_usd_cap": 5.0, "daily_usd_spent": 0.5},
        "get_watchlist": [{"symbol": "AAPL", "status": "watching"}],
        "get_candidates": [{"symbol": "TSLA", "score": 0.8, "source": "scanner"}],
        "get_bars": {
            "AAPL": [{"close": 150 + i * 0.1, "high": 151, "low": 149} for i in range(252)],
            "TSLA": [{"close": 200, "high": 201, "low": 199} for _ in range(252)],
        },
        "get_fundamentals": {
            "AAPL": {"Sector": "Technology"},
            "TSLA": {"Sector": "Auto"},
        },
        "get_news": {"AAPL": [], "TSLA": []},
        "get_sentiment": {"AAPL": [], "TSLA": []},
        "get_setups": {"AAPL": [], "TSLA": []},
    }


def _scoring_response():
    return make_tool_response(
        tool_name="score_candidates",
        tool_input={
            "scores": [
                {"symbol": "AAPL", "technical": 0.8, "fundamentals": 0.7, "sentiment": 0.7, "catalyst": 0.6},
                {"symbol": "TSLA", "technical": 0.5, "fundamentals": 0.4, "sentiment": 0.5, "catalyst": 0.3},
            ]
        },
    )


def _morning_pack_response():
    return make_tool_response(
        tool_name="write_morning_pack",
        tool_input={
            "ranked_ideas": [
                {
                    "symbol": "AAPL",
                    "thesis_md": "Earnings beat.",
                    "conviction": "A",
                    "invalidation": "close < 145",
                }
            ]
        },
    )


def _playbook_response():
    return make_tool_response(
        tool_name="submit_playbook",
        tool_input={
            "ranked_setups": [
                {
                    "symbol": "AAPL",
                    "bias": "long",
                    "trigger": "reclaim 5/4 HOD on volume",
                    "entry": "$150-152",
                    "invalidation": "lose $145",
                    "target_1": "$160",
                    "target_2": "$170",
                    "conviction": "A",
                    "rationale_md": "Catalyst + base.",
                    "evidence_refs": [{"source": "news", "note": "earnings"}],
                }
            ],
            "skip_list": [{"symbol": "TSLA", "reason": "no setup"}],
        },
        input_tokens=2500,
        output_tokens=600,
    )


# ---- Tests ------------------------------------------------------------------


@pytest.mark.asyncio
async def test_run_sweep_writes_playbook_after_morning_pack(
    fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient
):
    fake_mcp.responses = _stock_responses()
    fake_llm.responses = [
        _scoring_response(),
        _morning_pack_response(),
        _playbook_response(),
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 5),
        system_prompt="SYSTEM",
        playbook_system_prompt="PLAYBOOK_SYSTEM",
    )

    assert result.skipped_reason is None
    assert result.playbook is not None
    assert result.playbook.n_setups == 1
    assert result.playbook.n_skip == 1
    assert result.playbook.generation_id == 1
    assert result.playbook.skipped_reason is None
    assert result.playbook.spent_usd > 0

    # Three LLM calls — rank + synthesize + playbook.
    assert len(fake_llm.recorded) == 3
    playbook_call = fake_llm.recorded[2]
    assert playbook_call["system"] == "PLAYBOOK_SYSTEM"
    assert playbook_call["tool_choice"] == {"type": "tool", "name": "submit_playbook"}
    # The forced tool schema is included.
    tool_names = [t["name"] for t in (playbook_call["tools"] or [])]
    assert "submit_playbook" in tool_names

    # write_playbook reached the MCP client with the expected args.
    write_calls = [c for c in fake_mcp.calls if c[0] == "write_playbook"]
    assert len(write_calls) == 1
    args = write_calls[0][1]
    assert args["date"] == "2026-05-05"
    # account omitted — server resolves it.
    assert args["account"] is None
    assert args["ranked_setups"][0]["symbol"] == "AAPL"
    assert args["skip_list"] == [{"symbol": "TSLA", "reason": "no setup"}]


@pytest.mark.asyncio
async def test_run_sweep_skip_playbook_flag_bypasses_extension(
    fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient
):
    fake_mcp.responses = _stock_responses()
    fake_llm.responses = [_scoring_response(), _morning_pack_response()]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 5),
        system_prompt="SYSTEM",
        skip_playbook=True,
    )

    assert result.playbook is None
    # Only two LLM calls + no write_playbook.
    assert len(fake_llm.recorded) == 2
    assert all(c[0] != "write_playbook" for c in fake_mcp.calls)


@pytest.mark.asyncio
async def test_run_sweep_dry_run_skips_playbook_write(
    fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient
):
    fake_mcp.responses = _stock_responses()
    fake_llm.responses = [
        _scoring_response(),
        _morning_pack_response(),
        _playbook_response(),
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 5),
        dry_run=True,
        system_prompt="SYSTEM",
    )

    # Playbook LLM call still happens; only the MCP write is skipped.
    assert result.playbook is not None
    assert result.playbook.skipped_reason == "dry-run"
    assert result.playbook.n_setups == 1
    assert all(c[0] != "write_playbook" for c in fake_mcp.calls)


@pytest.mark.asyncio
async def test_run_sweep_playbook_skipped_when_llm_omits_tool_call(
    fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient
):
    fake_mcp.responses = _stock_responses()
    # Third response is a bare-text response with no tool_use block.
    from llm import LlmResponse

    no_tool_response = LlmResponse(
        text="forgot the tool",
        tool_uses=[],
        input_tokens=2000,
        output_tokens=300,
        stop_reason="end_turn",
        raw=None,
    )
    fake_llm.responses = [
        _scoring_response(),
        _morning_pack_response(),
        no_tool_response,
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 5),
        system_prompt="SYSTEM",
    )

    assert result.playbook is not None
    assert result.playbook.skipped_reason == "LLM did not call submit_playbook"
    assert all(c[0] != "write_playbook" for c in fake_mcp.calls)


@pytest.mark.asyncio
async def test_playbook_prompt_carries_trader_profile_when_present(
    fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient
):
    """The moat: when the trader has a recent negative-tag pattern on a
    name (e.g. chase_own_exit on TSLA in 3 of last 7 days), the
    playbook generator must SEE that signal in its prompt and the LLM
    must surface TSLA in skip_list with a reason citing the pattern.

    This test pins the wiring (profile reaches the prompt) and confirms
    the LLM's tool-call response — with a canned skip_list entry naming
    TSLA + the pattern — propagates to write_playbook unmodified."""
    fake_mcp.responses = _stock_responses()
    fake_mcp.responses["get_trader_profile"] = {
        "account": "U1",
        "window_days": 30,
        "since_date": "2026-04-05",
        "n_reviews": 7,
        "tag_frequencies": [
            {"tag": "chase_own_exit", "count": 3, "pct_of_reviews": 3 / 7},
            {"tag": "flat_close", "count": 5, "pct_of_reviews": 5 / 7},
        ],
        "pnl_by_tag": [],
        "trendline": {
            "last_7d": {
                "n_reviews": 5,
                "tag_counts": {"chase_own_exit": 3, "flat_close": 5},
                "net_pnl": -200.0,
                "avg_grade_score": -2.0,
            },
            "prior_21d": {
                "n_reviews": 2,
                "tag_counts": {"flat_close": 2},
                "net_pnl": 100.0,
                "avg_grade_score": 5.0,
            },
        },
        "recent_incidents": [
            {
                "date": "2026-05-04",
                "symbol": "TSLA",
                "tag": "chase_own_exit",
                "leg_observation": "Re-entered 395C at $2.50 within 2 min of selling at $2.45",
            },
            {
                "date": "2026-05-03",
                "symbol": "TSLA",
                "tag": "chase_own_exit",
                "leg_observation": "Re-bought after taking profit",
            },
            {
                "date": "2026-05-02",
                "symbol": "TSLA",
                "tag": "chase_own_exit",
                "leg_observation": "Chased back in 5 min after exit",
            },
        ],
    }

    skip_response = make_tool_response(
        tool_name="submit_playbook",
        tool_input={
            "ranked_setups": [
                {
                    "symbol": "AAPL",
                    "bias": "long",
                    "trigger": "reclaim 5/4 HOD",
                    "entry": "$150",
                    "invalidation": "lose $145",
                    "target_1": "$160",
                    "conviction": "B",
                    "rationale_md": "Catalyst + base.",
                }
            ],
            "skip_list": [
                {
                    "symbol": "TSLA",
                    "reason": "recent chase_own_exit pattern (3 of last 7 days)",
                }
            ],
        },
    )
    fake_llm.responses = [
        _scoring_response(),
        _morning_pack_response(),
        skip_response,
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 5),
        system_prompt="SYSTEM",
        playbook_system_prompt="PLAYBOOK_SYSTEM",
    )

    assert result.skipped_reason is None
    assert result.playbook is not None
    assert result.playbook.n_skip == 1

    # The prompt the LLM saw must carry the TRADER PROFILE block.
    playbook_call = fake_llm.recorded[2]
    user_msg = playbook_call["messages"][0]["content"]
    assert "TRADER PROFILE" in user_msg
    assert "chase_own_exit" in user_msg
    assert "TSLA" in user_msg
    assert "Re-entered 395C" in user_msg

    # write_playbook reached MCP with TSLA in skip_list, reason citing pattern.
    write_calls = [c for c in fake_mcp.calls if c[0] == "write_playbook"]
    assert len(write_calls) == 1
    skip = write_calls[0][1]["skip_list"]
    assert any(
        e["symbol"] == "TSLA" and "chase_own_exit" in e["reason"] for e in skip
    ), f"expected TSLA skip with chase_own_exit reason, got {skip}"

    # get_trader_profile was actually called with the configured window.
    profile_calls = [c for c in fake_mcp.calls if c[0] == "get_trader_profile"]
    assert len(profile_calls) == 1
    assert profile_calls[0][1]["window_days"] == 30


@pytest.mark.asyncio
async def test_playbook_prompt_omits_profile_block_text_when_no_reviews(
    fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient
):
    """When the profile is empty (n_reviews=0), the LLM still gets a
    playbook prompt but the TRADER PROFILE section is the placeholder —
    no behavioral conditioning. Mirrors the without-profile baseline of
    the moat test."""
    fake_mcp.responses = _stock_responses()
    # default get_trader_profile in the fake returns n_reviews=0
    fake_llm.responses = [
        _scoring_response(),
        _morning_pack_response(),
        _playbook_response(),
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 5),
        system_prompt="SYSTEM",
    )

    assert result.playbook is not None
    playbook_call = fake_llm.recorded[2]
    user_msg = playbook_call["messages"][0]["content"]
    assert "TRADER PROFILE" in user_msg
    assert "no profile available" in user_msg


@pytest.mark.asyncio
async def test_run_sweep_playbook_handles_empty_ranked_setups(
    fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient
):
    """No-trade day: the LLM returns empty ranked_setups + a populated
    skip_list. The playbook still ships."""
    fake_mcp.responses = _stock_responses()
    empty_response = make_tool_response(
        tool_name="submit_playbook",
        tool_input={
            "ranked_setups": [],
            "skip_list": [
                {"symbol": "AAPL", "reason": "earnings AMC"},
                {"symbol": "TSLA", "reason": "no setup"},
            ],
        },
    )
    fake_llm.responses = [
        _scoring_response(),
        _morning_pack_response(),
        empty_response,
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 5),
        system_prompt="SYSTEM",
    )

    assert result.playbook is not None
    assert result.playbook.n_setups == 0
    assert result.playbook.n_skip == 2
    assert result.playbook.skipped_reason is None

    write_calls = [c for c in fake_mcp.calls if c[0] == "write_playbook"]
    assert len(write_calls) == 1
    args = write_calls[0][1]
    assert args["ranked_setups"] == []
    assert len(args["skip_list"]) == 2
