//! `claude -p` subprocess backend — runs inference under the user's
//! Claude Code subscription instead of an `ANTHROPIC_API_KEY`.
//! Activated by `QK_LLM_BACKEND=claude_cli`. Always surveillance-only
//! (`--tools ""`, `--strict-mcp-config`, `--mcp-config
//! '{"mcpServers":{}}'`, `--permission-mode dontAsk`); these flags are
//! unconditional and unit-tested.
//!
//! Observed envelope (claude v2.1.126, `--output-format json`):
//! `is_error`, `result` (text when no `--json-schema`),
//! `structured_output` (schema-constrained JSON), `usage.*_tokens`,
//! `total_cost_usd` (best-effort under subscription auth; > 0 →
//! `LlmResponse.cost_usd_override`).

use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;

use async_trait::async_trait;
use serde_json::Value;
use tokio::io::AsyncWriteExt;

use super::backend::LlmBackend;
use super::{LlmError, LlmRequest, LlmResponse, Role, ToolCall, ToolChoice, Usage};

pub const DEFAULT_TIMEOUT: Duration = Duration::from_secs(60);

/// Empty MCP-server set for the CLI subprocess. The CLI requires the
/// `mcpServers` key — bare `{}` is rejected. Used with
/// `--strict-mcp-config` to keep the spawned instance from inheriting
/// `~/.claude/settings.json` MCP entries (which would re-enter the
/// app's own MCP socket and risk recursion).
pub const EMPTY_MCP_CONFIG: &str = r#"{"mcpServers":{}}"#;

fn backend_err(stage: &str, message: impl Into<String>) -> LlmError {
    LlmError::Backend {
        stage: stage.to_string(),
        message: message.into(),
    }
}

fn u32_field(u: &Value, key: &str) -> u32 {
    u.get(key).and_then(|v| v.as_u64()).unwrap_or(0) as u32
}

fn bool_field(v: &Value, key: &str) -> bool {
    v.get(key).and_then(|x| x.as_bool()).unwrap_or(false)
}

pub struct ClaudeCliBackend {
    binary: PathBuf,
    version: String,
    timeout: Duration,
}

impl ClaudeCliBackend {
    pub fn new(binary: PathBuf, version: String, timeout: Duration) -> Self {
        Self {
            binary,
            version,
            timeout,
        }
    }

    /// Synchronous version probe used by `lib.rs::run` at startup. Sync
    /// so it composes with Tauri's non-async `setup` callback without
    /// a runtime hop.
    pub fn probe_version(binary: &Path) -> Result<String, LlmError> {
        let output = std::process::Command::new(binary)
            .arg("--version")
            .output()
            .map_err(|e| {
                backend_err(
                    "version probe spawn",
                    format!("{}: {}", binary.display(), e),
                )
            })?;
        if !output.status.success() {
            return Err(backend_err(
                "version probe",
                format!(
                    "{} --version exited {}: {}",
                    binary.display(),
                    output.status,
                    String::from_utf8_lossy(&output.stderr).trim()
                ),
            ));
        }
        Ok(String::from_utf8_lossy(&output.stdout).trim().to_string())
    }

    /// Build the argv (excluding the binary path itself). Pure so the
    /// unit test asserts every always-on flag is present without
    /// spawning a process.
    pub fn build_argv(req: &LlmRequest, max_budget_usd: f64) -> Result<Vec<String>, LlmError> {
        if max_budget_usd <= 0.0 {
            return Err(LlmError::BudgetExhausted);
        }
        let tools_len = req.tools.as_ref().map_or(0, |t| t.len());
        if tools_len > 1 {
            return Err(backend_err(
                "argv",
                format!("claude_cli backend supports at most one tool, got {tools_len}"),
            ));
        }
        if matches!(req.tool_choice, Some(ToolChoice::Auto)) {
            return Err(backend_err(
                "argv",
                "claude_cli backend doesn't support tool_choice=Auto",
            ));
        }
        if req.messages.iter().any(|m| m.role == Role::Assistant) {
            return Err(backend_err(
                "argv",
                "claude_cli backend doesn't support multi-turn (assistant) messages",
            ));
        }

        let mut argv: Vec<String> = vec![
            "-p".into(),
            "--output-format".into(),
            "json".into(),
            "--model".into(),
            req.model.into(),
            "--max-budget-usd".into(),
            format!("{:.4}", max_budget_usd),
            "--tools".into(),
            "".into(),
            "--strict-mcp-config".into(),
            "--mcp-config".into(),
            EMPTY_MCP_CONFIG.into(),
            "--permission-mode".into(),
            "dontAsk".into(),
            "--no-session-persistence".into(),
        ];

        if !req.system.is_empty() {
            // Cache breakpoints aren't exposed by `claude -p`; concatenate
            // all blocks and drop the per-block cache flag silently.
            let combined = req
                .system
                .iter()
                .map(|b| b.text.as_str())
                .collect::<Vec<_>>()
                .join("\n\n");
            argv.push("--system-prompt".into());
            argv.push(combined);
        }

        if let (Some(tools), Some(ToolChoice::ForceTool(_))) = (&req.tools, &req.tool_choice) {
            if let Some(tool) = tools.first() {
                argv.push("--json-schema".into());
                argv.push(serde_json::to_string(&tool.input_schema)?);
            }
        }

        Ok(argv)
    }

    /// Encode the user-side prompt body. All four current Rust call
    /// sites are single-turn user messages; multi-turn is rejected at
    /// argv-build.
    pub fn build_prompt(req: &LlmRequest) -> String {
        req.messages
            .iter()
            .map(|m| m.content.as_str())
            .collect::<Vec<_>>()
            .join("\n\n")
    }

    /// Parse the JSON envelope. `tool_name` (the sole tool name from
    /// the request, when forced) synthesizes a `ToolCall { name, input
    /// }` from `structured_output` so callers see the same shape as
    /// today's API path.
    pub fn parse_envelope(value: &Value, tool_name: Option<&str>) -> Result<LlmResponse, LlmError> {
        if bool_field(value, "is_error") {
            let msg = value
                .get("result")
                .and_then(|v| v.as_str())
                .or_else(|| value.get("subtype").and_then(|v| v.as_str()))
                .unwrap_or("claude CLI returned is_error=true")
                .to_string();
            return Err(backend_err("envelope", msg));
        }

        let usage = value
            .get("usage")
            .map(|u| Usage {
                input_tokens: u32_field(u, "input_tokens"),
                output_tokens: u32_field(u, "output_tokens"),
                cache_read_input_tokens: u32_field(u, "cache_read_input_tokens"),
                cache_creation_input_tokens: u32_field(u, "cache_creation_input_tokens"),
            })
            .unwrap_or_default();

        let cost_usd_override = value
            .get("total_cost_usd")
            .and_then(|v| v.as_f64())
            .filter(|c| *c > 0.0);

        let mut text: Option<String> = None;
        let mut tool_calls: Vec<ToolCall> = Vec::new();
        if let Some(structured) = value.get("structured_output") {
            if let Some(name) = tool_name {
                tool_calls.push(ToolCall {
                    name: name.to_string(),
                    input: structured.clone(),
                });
            }
        } else if let Some(t) = value.get("result").and_then(|v| v.as_str()) {
            if !t.is_empty() {
                text = Some(t.to_string());
            }
        }

        Ok(LlmResponse {
            text,
            tool_calls,
            usage,
            cost_usd_override,
        })
    }
}

#[async_trait]
impl LlmBackend for ClaudeCliBackend {
    async fn call(&self, req: &LlmRequest, max_budget_usd: f64) -> Result<LlmResponse, LlmError> {
        let argv = Self::build_argv(req, max_budget_usd)?;
        let prompt = Self::build_prompt(req);

        // Empty tempdir so the CLI doesn't auto-discover the repo's
        // CLAUDE.md. PATH+HOME only; strip ANTHROPIC_* so the CLI
        // uses subscription auth, not the parent's API key.
        let workdir = tempfile::tempdir().map_err(|e| backend_err("workdir", e.to_string()))?;
        let env: Vec<(String, String)> = std::env::vars()
            .filter(|(k, _)| k == "PATH" || k == "HOME")
            .collect();

        let mut child = tokio::process::Command::new(&self.binary)
            .args(&argv)
            .current_dir(workdir.path())
            .env_clear()
            .envs(env)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .map_err(|e| backend_err("spawn", format!("{}: {}", self.binary.display(), e)))?;

        if let Some(mut stdin) = child.stdin.take() {
            stdin
                .write_all(prompt.as_bytes())
                .await
                .map_err(|e| backend_err("stdin", e.to_string()))?;
            drop(stdin);
        }

        let output = match tokio::time::timeout(self.timeout, child.wait_with_output()).await {
            Ok(Ok(o)) => o,
            Ok(Err(e)) => return Err(backend_err("wait", e.to_string())),
            Err(_) => {
                return Err(backend_err(
                    "timeout",
                    format!("claude CLI exceeded {:?}", self.timeout),
                ))
            }
        };

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
            return Err(backend_err(
                "exit",
                format!("claude CLI exited {}: {}", output.status, stderr),
            ));
        }
        if !output.stderr.is_empty() {
            tracing::debug!(
                stderr = %String::from_utf8_lossy(&output.stderr).trim(),
                "claude_cli backend: stderr"
            );
        }

        let value: Value = serde_json::from_slice(&output.stdout)
            .map_err(|e| backend_err("parse", format!("envelope JSON: {e}")))?;
        let tool_name = req
            .tools
            .as_ref()
            .and_then(|t| t.first())
            .map(|t| t.name.as_str());
        Self::parse_envelope(&value, tool_name)
    }

    fn kind(&self) -> &'static str {
        "claude-cli"
    }

    fn version(&self) -> Option<&str> {
        Some(&self.version)
    }
}
