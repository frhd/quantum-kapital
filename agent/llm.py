"""Thin Anthropic-API wrapper. Exists as a seam for tests — `LlmClient` is a
Protocol the orchestrator depends on; `AnthropicLlmClient` is the production
impl, and tests pass a `FakeLlmClient`.
"""

from __future__ import annotations

import os
from dataclasses import dataclass
from typing import Any, Iterable, Mapping, Protocol


@dataclass
class ToolUse:
    """One `tool_use` block extracted from an Anthropic response."""

    id: str
    name: str
    input: dict[str, Any]


@dataclass
class LlmResponse:
    """Subset of an Anthropic Messages response we use downstream."""

    text: str
    tool_uses: list[ToolUse]
    input_tokens: int
    output_tokens: int
    stop_reason: str | None
    raw: Any  # original SDK response, for debugging


class LlmClient(Protocol):
    async def call(
        self,
        *,
        model: str,
        system: str,
        messages: Iterable[Mapping[str, Any]],
        tools: Iterable[Mapping[str, Any]] | None = None,
        tool_choice: Mapping[str, Any] | None = None,
        max_tokens: int = 2048,
    ) -> LlmResponse: ...


class AnthropicLlmClient:
    """Production wrapper around `anthropic.AsyncAnthropic`."""

    def __init__(self, api_key: str | None = None) -> None:
        # Import here so tests can run without the SDK installed.
        from anthropic import AsyncAnthropic

        key = api_key or os.environ.get("ANTHROPIC_API_KEY")
        if not key:
            raise RuntimeError("ANTHROPIC_API_KEY not set")
        self._client = AsyncAnthropic(api_key=key)

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
        kwargs: dict[str, Any] = {
            "model": model,
            "max_tokens": max_tokens,
            "system": system,
            "messages": list(messages),
        }
        if tools is not None:
            kwargs["tools"] = list(tools)
        if tool_choice is not None:
            kwargs["tool_choice"] = dict(tool_choice)

        resp = await self._client.messages.create(**kwargs)
        return _to_llm_response(resp)


def _to_llm_response(resp: Any) -> LlmResponse:
    text_chunks: list[str] = []
    tool_uses: list[ToolUse] = []
    for block in getattr(resp, "content", []) or []:
        kind = getattr(block, "type", None)
        if kind == "text":
            text_chunks.append(getattr(block, "text", ""))
        elif kind == "tool_use":
            tool_uses.append(
                ToolUse(
                    id=getattr(block, "id", ""),
                    name=getattr(block, "name", ""),
                    input=dict(getattr(block, "input", {}) or {}),
                )
            )
    usage = getattr(resp, "usage", None)
    return LlmResponse(
        text="\n".join(text_chunks),
        tool_uses=tool_uses,
        input_tokens=int(getattr(usage, "input_tokens", 0) or 0),
        output_tokens=int(getattr(usage, "output_tokens", 0) or 0),
        stop_reason=getattr(resp, "stop_reason", None),
        raw=resp,
    )
