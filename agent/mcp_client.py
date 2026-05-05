"""Thin async wrapper over the local stdio MCP server.

The Quantum Kapital MCP server is a stdio binary (`mcp-server` in
`src-tauri/src/bin/`) that bridges JSON-RPC stdio to a unix socket the Tauri
app listens on. We spawn it and call typed tools with auto-JSON parsing.

The wrapper deliberately does not abstract every tool — it exposes the calls
the morning-sweep loop needs and a generic `call_tool` escape hatch.
"""

from __future__ import annotations

import json
import os
from contextlib import asynccontextmanager
from datetime import datetime, timedelta, timezone
from typing import TYPE_CHECKING, Any, AsyncIterator, Mapping, Sequence

if TYPE_CHECKING:
    from mcp import ClientSession  # noqa: F401


def _parse_tool_result(result: Any) -> Any:
    """MCP tool results come back as a CallToolResult with a .content list of
    TextContent blocks. Our Rust tools always return one JSON-encoded text
    block; decode it transparently. Returns the raw string if not JSON."""
    content = getattr(result, "content", None)
    if not content:
        return None
    block = content[0]
    text = getattr(block, "text", None)
    if text is None:
        return None
    try:
        return json.loads(text)
    except json.JSONDecodeError:
        return text


class McpClient:
    """Wraps an active MCP `ClientSession`. Construct via `McpClient.connect()`."""

    def __init__(self, session: Any) -> None:
        self._session = session

    @classmethod
    @asynccontextmanager
    async def connect(
        cls,
        server_bin: str,
        *,
        socket_path: str | None = None,
        extra_env: Mapping[str, str] | None = None,
    ) -> AsyncIterator["McpClient"]:
        """Launch the mcp-server bridge and open a session against it."""
        # Lazy-import the SDK so unit tests can run without it installed.
        from mcp import ClientSession, StdioServerParameters
        from mcp.client.stdio import stdio_client

        env = dict(os.environ)
        if socket_path:
            env["QK_MCP_SOCKET"] = socket_path
        if extra_env:
            env.update(extra_env)

        params = StdioServerParameters(command=server_bin, args=[], env=env)
        async with stdio_client(params) as (read, write):
            async with ClientSession(read, write) as session:
                await session.initialize()
                yield cls(session)

    async def call_tool(self, name: str, arguments: Mapping[str, Any] | None = None) -> Any:
        result = await self._session.call_tool(name, dict(arguments or {}))
        if getattr(result, "isError", False):
            raise McpToolError(name, _parse_tool_result(result) or "unknown error")
        return _parse_tool_result(result)

    # ---- Convenience wrappers for the morning-sweep loop ----------------------

    async def get_llm_budget_status(self) -> dict[str, Any]:
        return await self.call_tool("get_llm_budget_status")

    async def get_watchlist(self, status: str | None = None) -> list[dict[str, Any]]:
        args: dict[str, Any] = {}
        if status:
            args["status"] = status
        return await self.call_tool("get_watchlist", args)

    async def get_candidates(
        self,
        *,
        min_score: float | None = None,
        source: str | None = None,
        since_unix: int | None = None,
        include_promoted: bool = False,
        limit: int | None = None,
    ) -> list[dict[str, Any]]:
        args: dict[str, Any] = {"include_promoted": include_promoted}
        if min_score is not None:
            args["min_score"] = min_score
        if source:
            args["source"] = source
        if since_unix is not None:
            args["since_unix"] = since_unix
        if limit is not None:
            args["limit"] = limit
        return await self.call_tool("get_candidates", args)

    async def get_bars(
        self,
        symbol: str,
        bar_size: str,
        lookback_days: int,
    ) -> list[dict[str, Any]]:
        return await self.call_tool(
            "get_bars",
            {"symbol": symbol, "bar_size": bar_size, "lookback_days": lookback_days},
        )

    async def get_fundamentals(self, symbol: str) -> dict[str, Any]:
        return await self.call_tool("get_fundamentals", {"symbol": symbol})

    async def get_news(self, symbol: str, max_age_secs: int | None = None) -> list[dict[str, Any]]:
        args: dict[str, Any] = {"symbol": symbol}
        if max_age_secs is not None:
            args["max_age_secs"] = max_age_secs
        return await self.call_tool("get_news", args)

    async def get_sentiment(
        self,
        symbol: str,
        *,
        since: datetime | None = None,
        sources: Sequence[str] | None = None,
    ) -> list[dict[str, Any]]:
        args: dict[str, Any] = {"symbol": symbol}
        if since is not None:
            args["since_unix"] = int(since.timestamp())
        if sources:
            args["sources"] = list(sources)
        return await self.call_tool("get_sentiment", args)

    async def get_setups(
        self,
        *,
        symbol: str | None = None,
        since: datetime | None = None,
    ) -> list[dict[str, Any]]:
        args: dict[str, Any] = {}
        if symbol:
            args["symbol"] = symbol
        if since is not None:
            args["since"] = since.astimezone(timezone.utc).isoformat()
        return await self.call_tool("get_setups", args)

    async def write_morning_pack(
        self,
        *,
        date: str,
        ranked_ideas: list[dict[str, Any]],
    ) -> dict[str, Any]:
        return await self.call_tool(
            "write_morning_pack",
            {"date": date, "ranked_ideas": ranked_ideas},
        )

    async def add_ticker(self, symbol: str, reason: str) -> dict[str, Any]:
        return await self.call_tool("add_ticker", {"symbol": symbol, "reason": reason})

    async def get_morning_pack(self, *, date_iso: str) -> dict[str, Any]:
        return await self.call_tool("get_morning_pack", {"date": date_iso})

    async def get_outcomes(
        self,
        *,
        since_iso: str,
        eval_window_days: int | None = None,
    ) -> Any:
        args: dict[str, Any] = {"since": since_iso}
        if eval_window_days is not None:
            args["eval_window_days"] = eval_window_days
        return await self.call_tool("get_outcomes", args)

    async def append_journal_entry(
        self,
        *,
        date_iso: str,
        section: str,
        body_md: str,
    ) -> dict[str, Any]:
        return await self.call_tool(
            "append_journal_entry",
            {"date": date_iso, "section": section, "body_md": body_md},
        )

    async def get_trade_legs(
        self,
        *,
        date_iso: str,
        account: str | None = None,
        symbol: str | None = None,
    ) -> Any:
        args: dict[str, Any] = {"date": date_iso}
        if account is not None:
            args["account"] = account
        if symbol is not None:
            args["symbol"] = symbol
        return await self.call_tool("get_trade_legs", args)

    async def get_trade_review(
        self,
        *,
        date_iso: str,
        account: str | None = None,
        prompt_version: int | None = None,
    ) -> Any:
        args: dict[str, Any] = {"date": date_iso}
        if account is not None:
            args["account"] = account
        if prompt_version is not None:
            args["prompt_version"] = prompt_version
        return await self.call_tool("get_trade_review", args)

    async def write_trade_review(
        self,
        *,
        date_iso: str,
        account: str,
        prompt_version: int,
        summary: Mapping[str, Any],
        behavioral_tags: Sequence[str],
        leg_observations: Sequence[Mapping[str, Any]],
        narrative_md: str,
        llm_call_id: str | None = None,
    ) -> dict[str, Any]:
        args: dict[str, Any] = {
            "date": date_iso,
            "account": account,
            "prompt_version": prompt_version,
            "summary": dict(summary),
            "behavioral_tags": list(behavioral_tags),
            "leg_observations": [dict(o) for o in leg_observations],
            "narrative_md": narrative_md,
        }
        if llm_call_id is not None:
            args["llm_call_id"] = llm_call_id
        return await self.call_tool("write_trade_review", args)


class McpToolError(RuntimeError):
    """Raised when an MCP tool call returns an error result."""

    def __init__(self, tool: str, payload: Any) -> None:
        super().__init__(f"MCP tool {tool!r} failed: {payload}")
        self.tool = tool
        self.payload = payload


def hours_ago_unix(hours: int) -> int:
    return int((datetime.now(tz=timezone.utc) - timedelta(hours=hours)).timestamp())
