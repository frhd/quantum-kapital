"""Test fixtures: fake MCP + fake LLM seams that satisfy the production
interfaces without real I/O. Tests inject these into `run_sweep`.
"""

from __future__ import annotations

from dataclasses import dataclass, field
from typing import Any, Iterable, Mapping

import pytest

from llm import LlmResponse, ToolUse


@dataclass
class FakeMcpClient:
    """Mirrors the methods of `mcp_client.McpClient` that the loop calls.
    Each method pulls a canned response from `responses[<tool>]`. Calls are
    recorded in `calls` for assertions.
    """

    responses: dict[str, Any] = field(default_factory=dict)
    calls: list[tuple[str, dict[str, Any]]] = field(default_factory=list)

    def _get(self, key: str, default: Any = None) -> Any:
        return self.responses.get(key, default)

    async def get_llm_budget_status(self) -> dict[str, Any]:
        self.calls.append(("get_llm_budget_status", {}))
        return self._get("get_llm_budget_status", {"daily_usd_cap": 5.0, "daily_usd_spent": 0.10})

    async def get_watchlist(self, status: str | None = None) -> list[dict[str, Any]]:
        self.calls.append(("get_watchlist", {"status": status}))
        return list(self._get("get_watchlist", []))

    async def get_candidates(self, **kwargs: Any) -> list[dict[str, Any]]:
        self.calls.append(("get_candidates", kwargs))
        return list(self._get("get_candidates", []))

    async def get_bars(self, symbol: str, bar_size: str, lookback_days: int) -> list[dict[str, Any]]:
        self.calls.append(("get_bars", {"symbol": symbol, "bar_size": bar_size, "lookback_days": lookback_days}))
        per_symbol = self._get("get_bars", {})
        if isinstance(per_symbol, dict):
            return list(per_symbol.get(symbol, []))
        return list(per_symbol)

    async def get_fundamentals(self, symbol: str) -> dict[str, Any]:
        self.calls.append(("get_fundamentals", {"symbol": symbol}))
        per_symbol = self._get("get_fundamentals", {})
        if isinstance(per_symbol, dict) and symbol in per_symbol:
            return dict(per_symbol[symbol])
        return dict(per_symbol) if isinstance(per_symbol, dict) else {}

    async def get_news(self, symbol: str, max_age_secs: int | None = None) -> list[dict[str, Any]]:
        self.calls.append(("get_news", {"symbol": symbol, "max_age_secs": max_age_secs}))
        per_symbol = self._get("get_news", {})
        if isinstance(per_symbol, dict):
            return list(per_symbol.get(symbol, []))
        return list(per_symbol)

    async def get_sentiment(self, symbol: str, **kwargs: Any) -> list[dict[str, Any]]:
        self.calls.append(("get_sentiment", {"symbol": symbol, **kwargs}))
        per_symbol = self._get("get_sentiment", {})
        if isinstance(per_symbol, dict):
            return list(per_symbol.get(symbol, []))
        return list(per_symbol)

    async def get_setups(self, *, symbol: str | None = None, **kwargs: Any) -> list[dict[str, Any]]:
        self.calls.append(("get_setups", {"symbol": symbol, **kwargs}))
        per_symbol = self._get("get_setups", {})
        if isinstance(per_symbol, dict):
            return list(per_symbol.get(symbol or "", []))
        return list(per_symbol)

    async def write_morning_pack(self, *, date: str, ranked_ideas: list[dict[str, Any]]) -> dict[str, Any]:
        self.calls.append(("write_morning_pack", {"date": date, "ranked_ideas": ranked_ideas}))
        return {"date": date, "written": len(ranked_ideas)}

    async def add_ticker(self, symbol: str, reason: str) -> dict[str, Any]:
        self.calls.append(("add_ticker", {"symbol": symbol, "reason": reason}))
        return {"symbol": symbol, "added": True}

    async def write_playbook(
        self,
        *,
        date_iso: str,
        ranked_setups: list[dict[str, Any]],
        skip_list: list[dict[str, Any]],
        account: str | None = None,
        llm_call_id: str | None = None,
    ) -> dict[str, Any]:
        self.calls.append(
            (
                "write_playbook",
                {
                    "date": date_iso,
                    "account": account,
                    "ranked_setups": [dict(s) for s in ranked_setups],
                    "skip_list": [dict(s) for s in skip_list],
                    "llm_call_id": llm_call_id,
                },
            )
        )
        return self._get(
            "write_playbook",
            {
                "date": date_iso,
                "account": account or "U-test",
                "generation_id": 1,
                "n_setups": len(ranked_setups),
                "n_skip": len(skip_list),
                "generated_at": "2026-05-05T11:00:00Z",
            },
        )


@dataclass
class FakeLlmClient:
    """Returns canned `LlmResponse`s in order. Tests assert call shapes via
    `recorded` (one entry per `call()` invocation)."""

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
            raise AssertionError("FakeLlmClient ran out of canned responses")
        return self.responses.pop(0)


def make_tool_response(
    *,
    tool_name: str,
    tool_input: dict[str, Any],
    input_tokens: int = 800,
    output_tokens: int = 400,
) -> LlmResponse:
    return LlmResponse(
        text="",
        tool_uses=[ToolUse(id="toolu_test", name=tool_name, input=tool_input)],
        input_tokens=input_tokens,
        output_tokens=output_tokens,
        stop_reason="tool_use",
        raw=None,
    )


@pytest.fixture
def fake_mcp() -> FakeMcpClient:
    return FakeMcpClient()


@pytest.fixture
def fake_llm() -> FakeLlmClient:
    return FakeLlmClient()
