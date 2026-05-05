"""Morning-sweep loop integration test with fakes for MCP + Anthropic.

Verifies the orchestration shape: budget gate → universe → gather → rank →
synthesize → write_morning_pack. No real network or subprocess.
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
from morning_sweep import is_trading_day, run_sweep

from tests.conftest import FakeLlmClient, FakeMcpClient, make_tool_response


def _cfg(*, per_loop: float = 0.50, abort_above: float = 0.50) -> AgentConfig:
    return AgentConfig(
        budget=BudgetConfig(per_loop_usd=per_loop, abort_if_global_spend_above=abort_above),
        universe=UniverseConfig(top_k=5, candidate_min_score=0.1, setups_lookback_days=30),
        output=OutputConfig(min_ideas=3, max_ideas=5),
        models=ModelsConfig(fast="claude-haiku-4-5", smart="claude-sonnet-4-6"),
        mcp=McpConfig(server_bin="./not-used-in-tests", socket_path=None),
    )


def _stock_responses() -> dict:
    return {
        "get_llm_budget_status": {"daily_usd_cap": 5.0, "daily_usd_spent": 0.5},
        "get_watchlist": [{"symbol": "AAPL", "status": "watching"}],
        "get_candidates": [
            {"symbol": "TSLA", "score": 0.8, "source": "scanner"},
            {"symbol": "NVDA", "score": 0.7, "source": "stocktwits"},
        ],
        "get_bars": {
            "AAPL": [{"close": 150 + i * 0.1, "high": 151, "low": 149} for i in range(252)],
            "TSLA": [{"close": 200, "high": 201, "low": 199} for _ in range(252)],
            "NVDA": [{"close": 800, "high": 801, "low": 799} for _ in range(252)],
        },
        "get_fundamentals": {
            "AAPL": {"Sector": "Technology", "PERatio": "30"},
            "TSLA": {"Sector": "Auto"},
            "NVDA": {"Sector": "Semis"},
        },
        "get_news": {
            "AAPL": [{"id": 1, "verdict": "bullish", "title": "iPhone sales beat"}],
            "TSLA": [],
            "NVDA": [{"id": 2, "verdict": "bullish", "title": "Datacenter strong"}],
        },
        "get_sentiment": {
            "AAPL": [{"source": "stocktwits", "mentions": 100, "bullish": 80, "bearish": 5}],
            "TSLA": [{"source": "reddit_wsb", "mentions": 200, "bullish": 100, "bearish": 50}],
            "NVDA": [],
        },
        "get_setups": {"AAPL": [], "TSLA": [], "NVDA": []},
    }


@pytest.mark.asyncio
async def test_happy_path_writes_morning_pack(fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient):
    fake_mcp.responses = _stock_responses()

    fake_llm.responses = [
        make_tool_response(
            tool_name="score_candidates",
            tool_input={
                "scores": [
                    {"symbol": "AAPL", "technical": 0.8, "fundamentals": 0.7, "sentiment": 0.7, "catalyst": 0.6},
                    {"symbol": "TSLA", "technical": 0.5, "fundamentals": 0.4, "sentiment": 0.5, "catalyst": 0.3},
                    {"symbol": "NVDA", "technical": 0.7, "fundamentals": 0.6, "sentiment": 0.5, "catalyst": 0.7},
                ]
            },
            input_tokens=2000,
            output_tokens=300,
        ),
        make_tool_response(
            tool_name="write_morning_pack",
            tool_input={
                "ranked_ideas": [
                    {
                        "symbol": "AAPL",
                        "thesis_md": "Earnings beat, sector leadership, near 52w high.",
                        "conviction": "A",
                        "entry_zone": "150-152",
                        "invalidation": "close < 145",
                        "evidence_refs": [{"kind": "news", "id": 1}],
                    },
                    {
                        "symbol": "NVDA",
                        "thesis_md": "Datacenter momentum.",
                        "conviction": "B",
                        "invalidation": "close < 780",
                        "evidence_refs": [{"kind": "news", "id": 2}],
                    },
                    {
                        "symbol": "TSLA",
                        "thesis_md": "Speculative, watching.",
                        "conviction": "C",
                        "invalidation": "close < 195",
                    },
                ]
            },
            input_tokens=3000,
            output_tokens=500,
        ),
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 4),  # Monday — explicit trading day
        system_prompt="SYSTEM",
        skip_playbook=True,
    )

    assert result.ideas_written == 3
    assert result.skipped_reason is None
    assert result.spent_usd > 0

    # Two LLM calls: rank + synthesize.
    assert len(fake_llm.recorded) == 2
    assert fake_llm.recorded[0]["tool_choice"] == {"type": "tool", "name": "score_candidates"}
    assert fake_llm.recorded[1]["tool_choice"] == {"type": "tool", "name": "write_morning_pack"}

    # write_morning_pack was called on the MCP client.
    mcp_tools = [c[0] for c in fake_mcp.calls]
    assert "write_morning_pack" in mcp_tools

    # NVDA and TSLA should be promoted to watchlist (not on it initially).
    add_calls = [c for c in fake_mcp.calls if c[0] == "add_ticker"]
    promoted = {c[1]["symbol"] for c in add_calls}
    assert promoted == {"NVDA", "TSLA"}


@pytest.mark.asyncio
async def test_aborts_when_global_budget_above_threshold(fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient):
    fake_mcp.responses = {
        "get_llm_budget_status": {"daily_usd_cap": 5.0, "daily_usd_spent": 4.5},  # 90%
    }
    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(abort_above=0.50),
        today=date(2026, 5, 4),
        system_prompt="SYSTEM",
    )
    assert result.ideas_written == 0
    assert result.skipped_reason is not None
    assert "global daily budget" in result.skipped_reason
    assert len(fake_llm.recorded) == 0


@pytest.mark.asyncio
async def test_empty_universe_skips_gracefully(fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient):
    fake_mcp.responses = {
        "get_llm_budget_status": {"daily_usd_cap": 5.0, "daily_usd_spent": 0.1},
        "get_watchlist": [],
        "get_candidates": [],
    }
    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 4),
        system_prompt="SYSTEM",
    )
    assert result.ideas_written == 0
    assert result.skipped_reason == "empty universe"
    assert len(fake_llm.recorded) == 0


@pytest.mark.asyncio
async def test_dry_run_does_not_write(fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient):
    fake_mcp.responses = _stock_responses()

    fake_llm.responses = [
        make_tool_response(
            tool_name="score_candidates",
            tool_input={
                "scores": [
                    {"symbol": "AAPL", "technical": 0.8, "fundamentals": 0.7, "sentiment": 0.7, "catalyst": 0.6}
                ]
            },
        ),
        make_tool_response(
            tool_name="write_morning_pack",
            tool_input={
                "ranked_ideas": [
                    {
                        "symbol": "AAPL",
                        "thesis_md": "X",
                        "conviction": "B",
                        "invalidation": "close < 140",
                    }
                ]
            },
        ),
    ]

    result = await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 4),
        dry_run=True,
        system_prompt="SYSTEM",
        skip_playbook=True,
    )

    # Loop reports an idea but didn't persist.
    assert result.ideas_written == 1
    mcp_tools = [c[0] for c in fake_mcp.calls]
    assert "write_morning_pack" not in mcp_tools
    assert "add_ticker" not in mcp_tools


@pytest.mark.asyncio
async def test_shadow_mode_passes_flag_into_synthesis(fake_mcp: FakeMcpClient, fake_llm: FakeLlmClient):
    fake_mcp.responses = _stock_responses()

    fake_llm.responses = [
        make_tool_response(
            tool_name="score_candidates",
            tool_input={
                "scores": [
                    {"symbol": "AAPL", "technical": 0.8, "fundamentals": 0.6, "sentiment": 0.5, "catalyst": 0.5}
                ]
            },
        ),
        make_tool_response(
            tool_name="write_morning_pack",
            tool_input={
                "ranked_ideas": [
                    {
                        "symbol": "AAPL",
                        "thesis_md": "**[SHADOW PACK]**\nThesis.",
                        "conviction": "B",
                        "invalidation": "close < 140",
                    }
                ]
            },
        ),
    ]

    await run_sweep(
        mcp=fake_mcp,
        llm=fake_llm,
        cfg=_cfg(),
        today=date(2026, 5, 4),
        shadow=True,
        system_prompt="SYSTEM",
        skip_playbook=True,
    )

    # The synthesis-step user message should contain a shadow-mode instruction.
    synth_msg = fake_llm.recorded[1]["messages"][0]["content"]
    assert "SHADOW PACK" in synth_msg


def test_holiday_skipped_by_is_trading_day():
    # 2026-05-25 is Memorial Day (last Monday of May).
    assert not is_trading_day(date(2026, 5, 25))
    # 2026-05-26 (Tue) is a regular trading day.
    assert is_trading_day(date(2026, 5, 26))
    # Weekend
    assert not is_trading_day(date(2026, 5, 23))


def test_fixed_holidays_2026():
    assert not is_trading_day(date(2026, 1, 1))   # New Year
    assert not is_trading_day(date(2026, 7, 3))   # July 4 observed (Sat)
    assert not is_trading_day(date(2026, 12, 25)) # Christmas
