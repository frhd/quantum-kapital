"""LLM client seam for the agent loops.

Two production transports:

- `AnthropicLlmClient` — POSTs to `api.anthropic.com` via the SDK; needs
  `ANTHROPIC_API_KEY`.
- `ClaudeCliLlmClient` (`agent/llm_cli.py`) — spawns `claude -p` under
  the user's Claude Code subscription; no key required.

Selection is via `QK_LLM_BACKEND` (`anthropic` | `claude_cli`); use
`make_llm_client(...)` rather than constructing either class directly so
a single shell-level flip toggles every loop.
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
    """Subset of an Anthropic Messages response we use downstream.

    `cost_usd` carries the envelope-reported cost when the CLI backend
    parses one (see `llm_cli.ClaudeCliLlmClient.parse_envelope`). The
    Anthropic SDK does not surface a per-call USD figure, so the API
    backend leaves it `None` and `BudgetGuard.record` falls back to the
    per-token estimate. Any value <= 0 is treated as missing — callers
    must not record a free call.
    """

    text: str
    tool_uses: list[ToolUse]
    input_tokens: int
    output_tokens: int
    stop_reason: str | None
    raw: Any  # original SDK response, for debugging
    cost_usd: float | None = None


class BackendError(RuntimeError):
    """Raised by a non-Anthropic backend (e.g. the CLI client) on
    subprocess failure, unparseable envelopes, or `is_error=true`. Loops
    catch this like any other LLM error — it does not bypass the
    per-loop graceful-skip path."""


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


_ANTHROPIC_ALIASES = {"anthropic", "anthropic-api", "anthropic_api", ""}
_CLAUDE_CLI_ALIASES = {"claude_cli", "claude-cli", "cli"}


def normalize_backend(value: str | None) -> str:
    """Canonicalize a backend name. Mirrors
    `LlmBackendKind::from_env_str` in `src-tauri/src/config/settings.rs`
    so the same `QK_LLM_BACKEND` value parses identically in both
    languages. Unknown values raise — callers that want a default
    should pre-substitute it."""
    v = (value or "").strip().lower()
    if v in _ANTHROPIC_ALIASES:
        return "anthropic"
    if v in _CLAUDE_CLI_ALIASES:
        return "claude_cli"
    raise ValueError(f"unknown llm backend {value!r}; expected anthropic | claude_cli")


def make_llm_client(backend: str | None = None) -> LlmClient:
    """Construct the configured backend. `backend` defaults to
    `QK_LLM_BACKEND` and falls back to `anthropic`. The CLI client is
    imported lazily so the API path doesn't pay for the subprocess
    plumbing import (and vice versa)."""
    selected = normalize_backend(backend or os.environ.get("QK_LLM_BACKEND") or "anthropic")
    if selected == "anthropic":
        return AnthropicLlmClient()
    if selected == "claude_cli":
        from llm_cli import ClaudeCliLlmClient

        return ClaudeCliLlmClient()
    # `normalize_backend` already validates; defensive fall-through.
    raise ValueError(f"unsupported backend {selected!r}")
