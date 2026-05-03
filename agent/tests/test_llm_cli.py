"""Tests for `agent/llm_cli.py` — argv assembly, envelope parsing,
env hygiene, and the `make_llm_client` factory.

No test ever spawns a real `claude` subprocess; we patch
`asyncio.create_subprocess_exec` and `subprocess.run` to keep the
suite hermetic.
"""

from __future__ import annotations

import asyncio
import json
import os
from pathlib import Path
from typing import Any
from unittest.mock import patch

import pytest

import llm
import llm_cli
from llm import BackendError, make_llm_client, normalize_backend
from llm_cli import EMPTY_MCP_CONFIG, ClaudeCliLlmClient


# ---- Fixtures ---------------------------------------------------------------


@pytest.fixture
def fake_binary(tmp_path: Path) -> Path:
    """A no-op `claude` binary that satisfies the version probe so we
    can construct `ClaudeCliLlmClient` without a real CLI on PATH."""
    p = tmp_path / "claude"
    p.write_text("#!/usr/bin/env bash\necho '2.1.126 (Claude Code)'\n")
    p.chmod(0o755)
    return p


def _client(fake_binary: Path) -> ClaudeCliLlmClient:
    return ClaudeCliLlmClient(binary=fake_binary, version="2.1.126 (Claude Code)")


# ---- Argv -------------------------------------------------------------------


SAMPLE_TOOL = {
    "name": "write_research_note",
    "description": "x",
    "input_schema": {
        "type": "object",
        "properties": {
            "body_md": {"type": "string"},
            "conviction": {"type": "string", "enum": ["A", "B", "C"]},
        },
        "required": ["body_md", "conviction"],
    },
}


def test_argv_includes_every_surveillance_flag():
    argv = ClaudeCliLlmClient.build_argv(
        model="claude-sonnet-4-6",
        system="SYSTEM",
        tools=[SAMPLE_TOOL],
        tool_choice={"type": "tool", "name": "write_research_note"},
        messages=[{"role": "user", "content": "hello"}],
        max_budget_usd=0.50,
    )
    # Always-on surveillance lockdown — these literals are pinned by
    # the master plan's hard invariants.
    assert "-p" in argv
    assert "--output-format" in argv and argv[argv.index("--output-format") + 1] == "json"
    assert "--strict-mcp-config" in argv
    assert "--mcp-config" in argv
    assert argv[argv.index("--mcp-config") + 1] == EMPTY_MCP_CONFIG
    assert EMPTY_MCP_CONFIG == '{"mcpServers":{}}'
    assert "--permission-mode" in argv
    assert argv[argv.index("--permission-mode") + 1] == "dontAsk"
    assert "--tools" in argv
    assert argv[argv.index("--tools") + 1] == ""
    assert "--no-session-persistence" in argv
    # No `--bare`: we want subscription auth, not strict API key mode.
    assert "--bare" not in argv
    # Forced tool's schema becomes --json-schema.
    assert "--json-schema" in argv
    schema_str = argv[argv.index("--json-schema") + 1]
    assert json.loads(schema_str) == SAMPLE_TOOL["input_schema"]
    # System prompt threaded through.
    assert "--system-prompt" in argv
    assert argv[argv.index("--system-prompt") + 1] == "SYSTEM"
    # Budget cap formatted to 4 decimals.
    assert argv[argv.index("--max-budget-usd") + 1] == "0.5000"


def test_argv_omits_json_schema_for_text_calls():
    argv = ClaudeCliLlmClient.build_argv(
        model="claude-haiku-4-5",
        system="",
        tools=None,
        tool_choice=None,
        messages=[{"role": "user", "content": "summarize"}],
        max_budget_usd=0.10,
    )
    assert "--json-schema" not in argv
    # Empty system prompt is dropped.
    assert "--system-prompt" not in argv


def test_argv_rejects_multi_tool():
    with pytest.raises(BackendError, match="at most one tool"):
        ClaudeCliLlmClient.build_argv(
            model="claude-sonnet-4-6",
            system="x",
            tools=[SAMPLE_TOOL, SAMPLE_TOOL],
            tool_choice={"type": "tool", "name": "write_research_note"},
            messages=[{"role": "user", "content": "hi"}],
            max_budget_usd=1.0,
        )


def test_argv_rejects_tool_choice_auto():
    with pytest.raises(BackendError, match="tool_choice=auto"):
        ClaudeCliLlmClient.build_argv(
            model="claude-sonnet-4-6",
            system="x",
            tools=[SAMPLE_TOOL],
            tool_choice={"type": "auto"},
            messages=[{"role": "user", "content": "hi"}],
            max_budget_usd=1.0,
        )


def test_argv_rejects_multi_turn():
    with pytest.raises(BackendError, match="multi-turn"):
        ClaudeCliLlmClient.build_argv(
            model="claude-sonnet-4-6",
            system="x",
            tools=None,
            tool_choice=None,
            messages=[
                {"role": "user", "content": "hi"},
                {"role": "assistant", "content": "..."},
            ],
            max_budget_usd=1.0,
        )


def test_argv_rejects_zero_budget():
    with pytest.raises(BackendError, match="max_budget_usd"):
        ClaudeCliLlmClient.build_argv(
            model="claude-sonnet-4-6",
            system="x",
            tools=None,
            tool_choice=None,
            messages=[{"role": "user", "content": "hi"}],
            max_budget_usd=0.0,
        )


# ---- Envelope ---------------------------------------------------------------


def test_parse_envelope_structured_output_yields_tool_use():
    payload = {
        "is_error": False,
        "result": "",
        "structured_output": {"body_md": "x" * 60, "conviction": "B"},
        "usage": {"input_tokens": 1500, "output_tokens": 400},
        "total_cost_usd": 0.0078,
    }
    resp = ClaudeCliLlmClient.parse_envelope(payload, tool_name="write_research_note")
    assert resp.text == ""
    assert len(resp.tool_uses) == 1
    assert resp.tool_uses[0].name == "write_research_note"
    assert resp.tool_uses[0].input["conviction"] == "B"
    assert resp.input_tokens == 1500
    assert resp.output_tokens == 400
    assert resp.cost_usd == pytest.approx(0.0078)
    assert resp.stop_reason == "tool_use"


def test_parse_envelope_text_path():
    payload = {
        "is_error": False,
        "result": "plain text answer",
        "usage": {"input_tokens": 200, "output_tokens": 50},
    }
    resp = ClaudeCliLlmClient.parse_envelope(payload, tool_name=None)
    assert resp.text == "plain text answer"
    assert resp.tool_uses == []
    assert resp.cost_usd is None  # missing field → None
    assert resp.stop_reason == "end_turn"


def test_parse_envelope_zero_cost_treated_as_missing():
    """Subscription auth often reports total_cost_usd=0; we MUST NOT
    treat that as 'free' in the budget guard, so parse_envelope strips
    it down to None."""
    payload = {
        "is_error": False,
        "result": "x",
        "usage": {"input_tokens": 10, "output_tokens": 5},
        "total_cost_usd": 0,
    }
    resp = ClaudeCliLlmClient.parse_envelope(payload, tool_name=None)
    assert resp.cost_usd is None


def test_parse_envelope_is_error_raises_backend_error():
    payload = {"is_error": True, "result": "rate limited"}
    with pytest.raises(BackendError, match="rate limited"):
        ClaudeCliLlmClient.parse_envelope(payload, tool_name=None)


# ---- Spawn / env hygiene ----------------------------------------------------


class _FakeProc:
    def __init__(self, stdout: bytes, stderr: bytes = b"", returncode: int = 0):
        self._stdout = stdout
        self._stderr = stderr
        self.returncode = returncode

    async def communicate(self, input: bytes | None = None) -> tuple[bytes, bytes]:
        return self._stdout, self._stderr

    def kill(self) -> None:  # pragma: no cover — only used by timeout test
        pass

    async def wait(self) -> int:  # pragma: no cover
        return self.returncode


@pytest.mark.asyncio
async def test_call_strips_anthropic_env_and_writes_prompt_to_stdin(
    fake_binary: Path, monkeypatch: pytest.MonkeyPatch
):
    """The CLI's auth precedence prefers the env var over OAuth/keychain.
    Leaking ANTHROPIC_API_KEY into the subprocess would silently
    disable subscription-mode behavior — assert it does NOT propagate."""
    monkeypatch.setenv("ANTHROPIC_API_KEY", "sk-leaked")
    monkeypatch.setenv("ANTHROPIC_AUTH_TOKEN", "should-also-not-leak")

    captured: dict[str, Any] = {}

    async def fake_exec(*args, **kwargs):
        captured["args"] = args
        captured["env"] = kwargs.get("env")
        captured["cwd"] = kwargs.get("cwd")
        return _FakeProc(
            stdout=json.dumps(
                {
                    "is_error": False,
                    "result": "",
                    "structured_output": {"body_md": "x" * 60, "conviction": "B"},
                    "usage": {"input_tokens": 100, "output_tokens": 50},
                    "total_cost_usd": 0.001,
                }
            ).encode(),
        )

    monkeypatch.setattr(asyncio, "create_subprocess_exec", fake_exec)

    client = _client(fake_binary)
    resp = await client.call(
        model="claude-sonnet-4-6",
        system="S",
        messages=[{"role": "user", "content": "hi"}],
        tools=[SAMPLE_TOOL],
        tool_choice={"type": "tool", "name": "write_research_note"},
    )

    env = captured["env"]
    assert "ANTHROPIC_API_KEY" not in env
    assert "ANTHROPIC_AUTH_TOKEN" not in env
    assert set(env.keys()) <= {"PATH", "HOME"}
    # Spawn happened in a temp dir, not the project working dir.
    assert captured["cwd"] is not None
    assert Path(captured["cwd"]).exists() is False  # cleaned up after `with` block.
    assert resp.input_tokens == 100
    assert resp.cost_usd == pytest.approx(0.001)


@pytest.mark.asyncio
async def test_call_surfaces_nonzero_exit_as_backend_error(
    fake_binary: Path, monkeypatch: pytest.MonkeyPatch
):
    async def fake_exec(*args, **kwargs):
        return _FakeProc(stdout=b"", stderr=b"boom", returncode=2)

    monkeypatch.setattr(asyncio, "create_subprocess_exec", fake_exec)

    client = _client(fake_binary)
    with pytest.raises(BackendError, match="exited 2"):
        await client.call(
            model="claude-sonnet-4-6",
            system="",
            messages=[{"role": "user", "content": "hi"}],
        )


# ---- Version probe ----------------------------------------------------------


def test_missing_binary_fails_fast(tmp_path: Path):
    """Per master invariant #6 — no silent fallback when the binary is
    missing. The construct-time probe must raise."""
    missing = tmp_path / "does-not-exist"
    with pytest.raises(BackendError, match="not found"):
        ClaudeCliLlmClient(binary=missing)


def test_probe_reads_version_from_real_binary(fake_binary: Path):
    client = ClaudeCliLlmClient(binary=fake_binary)
    assert "2.1.126" in client.version


# ---- Factory ----------------------------------------------------------------


def test_normalize_backend_aliases():
    assert normalize_backend("anthropic") == "anthropic"
    assert normalize_backend("anthropic-api") == "anthropic"
    assert normalize_backend("") == "anthropic"
    assert normalize_backend(None) == "anthropic"
    assert normalize_backend("claude_cli") == "claude_cli"
    assert normalize_backend("claude-cli") == "claude_cli"
    assert normalize_backend("CLI") == "claude_cli"
    with pytest.raises(ValueError):
        normalize_backend("openai")


def test_factory_returns_anthropic_by_default(monkeypatch: pytest.MonkeyPatch):
    """`make_llm_client("anthropic")` must construct the API client.
    We stub `AnthropicLlmClient.__init__` so the test doesn't need a
    real key."""

    def _init(self, api_key: str | None = None):
        self._client = object()

    monkeypatch.setattr(llm.AnthropicLlmClient, "__init__", _init)
    client = make_llm_client("anthropic")
    assert isinstance(client, llm.AnthropicLlmClient)


def test_factory_returns_cli_for_claude_cli(monkeypatch: pytest.MonkeyPatch, fake_binary: Path):
    """`make_llm_client("claude_cli")` must construct the CLI client.
    Patch the version probe so we don't depend on a real CLI."""
    monkeypatch.setenv("QK_CLAUDE_BINARY", str(fake_binary))
    client = make_llm_client("claude_cli")
    assert isinstance(client, ClaudeCliLlmClient)
    assert client.version  # populated from the fake binary above
