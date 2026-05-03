"""`claude -p` subprocess backend — runs inference under the user's
Claude Code subscription instead of an `ANTHROPIC_API_KEY`.

Activated by `QK_LLM_BACKEND=claude_cli` (via `llm.make_llm_client`).
Mirrors the Rust `ClaudeCliBackend` in
`src-tauri/src/services/llm_service/cli_backend.rs` — same flag set,
same envelope shape, same single-tool / no-multi-turn restrictions, so
the two languages stay byte-compatible against the same CLI version.

Surveillance lockdown is unconditional: every spawn passes
`--tools ""`, `--strict-mcp-config`, `--mcp-config '{"mcpServers":{}}'`,
and `--permission-mode dontAsk`. The argv unit test pins the literal
strings.

Observed envelope (claude v2.1.126, `--output-format json`): `is_error`,
`result` (text when no `--json-schema`), `structured_output`
(schema-constrained JSON), `usage.{input_tokens, output_tokens,
cache_read_input_tokens, cache_creation_input_tokens}`,
`total_cost_usd` (best-effort under subscription auth).
"""

from __future__ import annotations

import asyncio
import json
import logging
import os
import shutil
import subprocess
import tempfile
from pathlib import Path
from typing import Any, Iterable, Mapping, Sequence

from llm import BackendError, LlmResponse, ToolUse


log = logging.getLogger("llm_cli")


DEFAULT_TIMEOUT_SECS = 60.0
DEFAULT_MAX_BUDGET_USD = 1.0

# The CLI rejects bare `{}` — see `loop/plan/QUESTIONS.md::Phase 1`.
EMPTY_MCP_CONFIG: str = '{"mcpServers":{}}'


class ClaudeCliLlmClient:
    """`LlmClient` impl that shells out to `claude -p`."""

    def __init__(
        self,
        binary: str | os.PathLike[str] | None = None,
        *,
        version: str | None = None,
        timeout_secs: float = DEFAULT_TIMEOUT_SECS,
        max_budget_usd: float = DEFAULT_MAX_BUDGET_USD,
    ) -> None:
        if binary is None:
            binary = os.environ.get("QK_CLAUDE_BINARY") or "claude"
        resolved = self._resolve_binary(binary)
        self._binary: Path = resolved
        self._timeout = float(timeout_secs)
        self._max_budget_usd = float(max_budget_usd)
        if version is None:
            version = self._probe_version(resolved)
        self._version = version
        log.info("llm: backend=claude_cli version=%s", self._version)

    @staticmethod
    def _resolve_binary(binary: str | os.PathLike[str]) -> Path:
        p = Path(binary)
        if p.is_absolute():
            if not p.exists():
                raise BackendError(f"claude binary not found at {p}")
            return p
        found = shutil.which(str(binary))
        if not found:
            raise BackendError(f"claude binary {binary!r} not on PATH")
        return Path(found)

    @staticmethod
    def _probe_version(binary: Path) -> str:
        """Synchronous version probe used at construction. Mirrors
        `ClaudeCliBackend::probe_version` in the Rust backend so a
        misconfigured deployment fails fast with the same message
        shape rather than crashing on the first call."""
        try:
            out = subprocess.run(
                [str(binary), "--version"],
                check=False,
                capture_output=True,
                text=True,
                timeout=10.0,
            )
        except (OSError, subprocess.SubprocessError) as e:
            raise BackendError(f"version probe spawn {binary}: {e}") from e
        if out.returncode != 0:
            raise BackendError(
                f"{binary} --version exited {out.returncode}: {out.stderr.strip()}"
            )
        return out.stdout.strip()

    @property
    def version(self) -> str:
        return self._version

    @staticmethod
    def build_argv(
        *,
        model: str,
        system: str,
        tools: Iterable[Mapping[str, Any]] | None,
        tool_choice: Mapping[str, Any] | None,
        messages: Sequence[Mapping[str, Any]],
        max_budget_usd: float,
    ) -> list[str]:
        """Pure argv assembly so the unit test can assert every
        always-on flag without spawning a process."""
        if max_budget_usd <= 0:
            raise BackendError("max_budget_usd must be > 0")
        tools_list = list(tools) if tools is not None else []
        if len(tools_list) > 1:
            raise BackendError(
                f"claude_cli backend supports at most one tool, got {len(tools_list)}"
            )
        if tool_choice is not None:
            choice_type = tool_choice.get("type")
            if choice_type == "auto":
                raise BackendError("claude_cli backend doesn't support tool_choice=auto")
            if choice_type not in (None, "tool"):
                raise BackendError(
                    f"claude_cli backend doesn't support tool_choice type {choice_type!r}"
                )
        if any(m.get("role") == "assistant" for m in messages):
            raise BackendError(
                "claude_cli backend doesn't support multi-turn (assistant) messages"
            )

        argv: list[str] = [
            "-p",
            "--output-format",
            "json",
            "--model",
            str(model),
            "--max-budget-usd",
            f"{max_budget_usd:.4f}",
            "--tools",
            "",
            "--strict-mcp-config",
            "--mcp-config",
            EMPTY_MCP_CONFIG,
            "--permission-mode",
            "dontAsk",
            "--no-session-persistence",
        ]

        if system:
            argv.extend(["--system-prompt", str(system)])

        if tools_list and tool_choice and tool_choice.get("type") == "tool":
            schema = tools_list[0].get("input_schema")
            if schema is None:
                raise BackendError("forced tool missing input_schema")
            argv.extend(["--json-schema", json.dumps(schema, separators=(",", ":"))])

        return argv

    @staticmethod
    def build_prompt(messages: Sequence[Mapping[str, Any]]) -> str:
        """Concat user-message contents. The agent loops only ever
        emit single-turn user messages today; multi-turn is rejected
        in `build_argv`."""
        parts: list[str] = []
        for m in messages:
            role = m.get("role")
            if role and role != "user":
                continue
            content = m.get("content")
            if isinstance(content, str):
                parts.append(content)
            elif isinstance(content, list):
                # Anthropic-style content blocks. Best-effort: collect text.
                for block in content:
                    if isinstance(block, Mapping) and block.get("type") == "text":
                        parts.append(str(block.get("text", "")))
        return "\n\n".join(parts)

    @staticmethod
    def parse_envelope(payload: Mapping[str, Any], tool_name: str | None) -> LlmResponse:
        if payload.get("is_error"):
            msg = (
                payload.get("result")
                or payload.get("subtype")
                or "claude CLI returned is_error=true"
            )
            raise BackendError(str(msg))

        usage = payload.get("usage") or {}
        input_tokens = int(usage.get("input_tokens") or 0)
        output_tokens = int(usage.get("output_tokens") or 0)

        cost_raw = payload.get("total_cost_usd")
        cost_usd: float | None
        try:
            cost_usd = float(cost_raw) if cost_raw is not None else None
        except (TypeError, ValueError):
            cost_usd = None
        if cost_usd is not None and cost_usd <= 0:
            cost_usd = None

        text = ""
        tool_uses: list[ToolUse] = []
        if "structured_output" in payload and tool_name:
            tool_uses.append(
                ToolUse(
                    id="cli",
                    name=tool_name,
                    input=dict(payload["structured_output"] or {}),
                )
            )
        else:
            result = payload.get("result")
            if isinstance(result, str):
                text = result

        return LlmResponse(
            text=text,
            tool_uses=tool_uses,
            input_tokens=input_tokens,
            output_tokens=output_tokens,
            stop_reason="tool_use" if tool_uses else "end_turn",
            raw=payload,
            cost_usd=cost_usd,
        )

    async def call(
        self,
        *,
        model: str,
        system: str,
        messages: Iterable[Mapping[str, Any]],
        tools: Iterable[Mapping[str, Any]] | None = None,
        tool_choice: Mapping[str, Any] | None = None,
        max_tokens: int = 2048,  # noqa: ARG002 — CLI doesn't expose; kept for Protocol parity.
    ) -> LlmResponse:
        msg_list = list(messages)
        argv = self.build_argv(
            model=model,
            system=system,
            tools=tools,
            tool_choice=tool_choice,
            messages=msg_list,
            max_budget_usd=self._max_budget_usd,
        )
        prompt = self.build_prompt(msg_list)
        tool_name = None
        if tools is not None:
            tool_list = list(tools)
            if tool_list:
                tool_name = str(tool_list[0].get("name") or "")

        # Strip `ANTHROPIC_*` so the CLI uses subscription auth, not
        # the parent's API key (the CLI's auth precedence prefers env
        # over keychain). PATH+HOME only — same shape as the Rust
        # backend's `env_clear` + filtered envs.
        env = {
            k: v for k, v in os.environ.items() if k in {"PATH", "HOME"}
        }

        with tempfile.TemporaryDirectory() as workdir:
            try:
                proc = await asyncio.create_subprocess_exec(
                    str(self._binary),
                    *argv,
                    cwd=workdir,
                    env=env,
                    stdin=asyncio.subprocess.PIPE,
                    stdout=asyncio.subprocess.PIPE,
                    stderr=asyncio.subprocess.PIPE,
                )
            except OSError as e:
                raise BackendError(f"spawn {self._binary}: {e}") from e

            try:
                stdout_b, stderr_b = await asyncio.wait_for(
                    proc.communicate(input=prompt.encode("utf-8")),
                    timeout=self._timeout,
                )
            except asyncio.TimeoutError as e:
                proc.kill()
                try:
                    await proc.wait()
                except Exception:  # noqa: BLE001
                    pass
                raise BackendError(
                    f"claude CLI exceeded {self._timeout:.0f}s timeout"
                ) from e

        if proc.returncode != 0:
            stderr = stderr_b.decode("utf-8", errors="replace").strip()
            raise BackendError(
                f"claude CLI exited {proc.returncode}: {stderr}"
            )
        if stderr_b:
            log.debug("claude_cli stderr: %s", stderr_b.decode("utf-8", errors="replace").strip())

        try:
            payload = json.loads(stdout_b.decode("utf-8"))
        except (UnicodeDecodeError, json.JSONDecodeError) as e:
            raise BackendError(f"envelope JSON: {e}") from e
        if not isinstance(payload, Mapping):
            raise BackendError(f"envelope not a JSON object: {type(payload).__name__}")
        return self.parse_envelope(payload, tool_name)
