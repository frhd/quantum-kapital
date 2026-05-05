# In-app trade-review generator (no sidecar) — ~1 week

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development (recommended) or superpowers:executing-plans to implement this plan task-by-task. Steps use checkbox (`- [ ]`) syntax for tracking.

**Goal:** Make the Trade Review card's empty state trigger a review on click — a Rust-native pipeline (`executions → FIFO legs → LegSummary → Anthropic tool-call → TradeReviewStore`) that produces byte-identical artifacts to `agent/eod_review.py` without spawning a Python sidecar.

**Architecture:** A new `services/trade_reviews/generator/` Rust submodule mirrors the Python helpers in `agent/trade_review.py` (prompt formatter, tool schema, response parser, leg-summary builder) and an orchestrator that calls `LlmService` (budget-gated) and persists via the existing `TradeReviewStore`. A new Tauri command `generate_trade_review(date, account?)` exposes it; the empty state grows a "Generate review" button.

**Tech Stack:** Rust (`tokio`, `serde`, `serde_json`, `chrono`), Tauri 2 (`#[tauri::command]`), React 19 + TypeScript + Tailwind 4 + Vite (frontend), `LlmService` Anthropic Messages backend (budget-gated against `llm_calls` ledger).

---

## Context

The Trade Review card today is a passive read-only view of the `day_reviews` SQLite table. Rows are written by `agent/eod_review.py` — a Python loop that's *meant* to run via a 17:00 ET cron entry but, in practice, is rarely installed. When the user opens the app the next morning, they see:

> No trade review for 2026-05-04 yet. Reviews are written automatically at 17:00 ET. Check back after market close, or run `uv run qk-eod-review --date 2026-05-04` manually.

Both halves of that copy are misleading: the auto-write isn't running, and `qk-eod-review` was superseded last session by the `/eod-review` slash command. The user can also invoke the slash command from another Claude Code session, but that's friction — the desktop app should be able to write its own reviews on click.

**Inversion.** Today: writing a review requires either an installed cron, an external Claude Code session, or a manual Python invocation. End state: the user clicks "Generate review" inside the app and a Rust-native pipeline writes the row.

**Why no sidecar.** Spawning `uv run qk-eod-review` from Tauri would mean depending on the user's PATH/venv, shelling out, and the Python agent calling back over MCP into the same desktop app — error surface is muddy and the data dependency loops. A Rust-native generator is ~600 lines that mostly mirrors what `agent/trade_review.py` already does; the deterministic pieces (FIFO matcher, tag enum, grade computation) are *already* in Rust. We're not duplicating logic — we're un-detouring it.

**The Python copy stays.** `agent/eod_review.py` + the `/eod-review` slash command keep working unchanged. They serve cron-driven and external-Claude-driven flows. The new Rust path is the in-app button — three flows, one persisted artifact.

## End-state architecture

| Component | Layer | Responsibility |
|---|---|---|
| **`LlmKind::Review`** (Phase 1) | LLM ledger | New variant in the closed `LlmKind` enum so Trade-Review LLM calls log as `kind='review'` in `llm_calls`. Required before any Phase 2+ test that exercises `LlmService::message`. |
| **`generator/prompt.rs`** (Phase 2) | pure | Rust port of `agent/trade_review.py::format_trade_review_prompt`. No I/O. |
| **`generator/tool.rs`** (Phase 3) | pure | `submit_trade_review` tool schema + `parse_tool_response(Value) -> ParsedReview`. Mirrors the Python schema but consumes `BehavioralTag` (Rust enum) for free. |
| **`generator/summary.rs`** (Phase 4) | pure | `summarize(legs: &[TradeLeg]) -> LegSummary`. Rust port of `leg_summary_from_legs`. Pure function — no DB. |
| **`generator/mod.rs::TradeReviewGenerator::generate`** (Phase 5) | orchestrator | Pulls executions via `ExecutionsStore::query`, runs `match_legs`, computes summary, builds prompt, calls `LlmService::message` (forced tool), parses, writes via `TradeReviewStore::write`. Returns the populated `TradeReview` or a typed error. |
| **`generate_trade_review` Tauri command** (Phase 6) | command | Resolves account, calls the generator, returns `Result<TradeReview, String>` to the frontend. |
| **`assessmentsApi.generateTradeReview`** (Phase 7) | FE wrapper | The only place that names the new command string. |
| **`EmptyTradeReview` button** (Phase 8) | UI | Replaces the stale message with accurate copy + a "Generate review" button that wires through the wrapper, optimistically refreshes the card. |

## Hard invariants

1. **Surveillance-only stays.** The generator reads fills, calls an LLM, and writes a `day_reviews` row. It MUST NOT call any order-placement code path.
2. **Budget-gating is non-negotiable.** Every Anthropic call goes through `LlmService::message`. No raw `reqwest` to the API. The `llm_calls` ledger gets exactly one row per generated review (model name, token counts, cost, kind=`review`).
3. **Idempotency.** The generator persists via `TradeReviewStore::write` which UPSERTs on `(date, account, prompt_version)`. Re-clicking "Generate review" overwrites the existing row, never duplicates.
4. **No live IBKR in tests.** The generator reads from `ExecutionsStore`, not from the IBKR adapter. Tests use the in-memory `make_db()` fixture and seed the store directly.
5. **No live Anthropic in tests.** All LLM-touching tests use `LlmService::with_http(Arc<dyn AnthropicHttp>)` to inject a `MockHttp`-style fake that returns a canned tool-call envelope.
6. **Deterministic grade.** The generator passes `(summary, behavioral_tags)` into `TradeReviewStore::write` which calls `compute_grade` deterministically. The LLM never picks the grade — that's the same invariant Phase 4 of the prior plan locked in.
7. **In-app writes do NOT use the `mcp_audit` rail.** `write_trade_review` (the MCP tool) audits agent writes from external clients. The in-app generator writes directly via `TradeReviewStore::write` — provenance comes from the `llm_call_id` field, which we set to the `llm_calls` row id of the generation call.
8. **The Python copy stays in sync.** The `BehavioralTag` Rust enum remains the source of truth for `agent/trade_review.py::BEHAVIORAL_TAGS` (the existing `agent/tests/test_tag_mirror.py` enforces that). The new Rust prompt builder is allowed to drift from the Python one ONLY in formatting (whitespace, ordering); the *facts it conveys* (summary fields, leg fields, tag menu) MUST match.
9. **Pre-commit sacred.** `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, `eslint`. Never `--no-verify`.
10. **File-size caps.** Rust soft 300 / hard 500. TS/TSX soft 200 / hard 350. Past hard cap requires `// allow-large-file: <reason>`.

## Defaults committed (overridable per-phase)

- **Date format:** `YYYY-MM-DD` interpreted as the ET trading day. Same convention as `get_trade_review`, `get_trade_legs`, etc.
- **Account resolution:** reuse `mcp::tools::resolve_account(reader, opt)` from the existing assessment commands.
- **Prompt version:** `pub const PROMPT_VERSION_RUST: i32 = 1;` for the new Rust path. Distinct from any future Python bump — bumping either side bumps independently. Old reviews stay queryable via `TradeReviewStore::read(date, account, version)`.
- **Model:** `claude-sonnet-4-6` (mirrors the thesis path's structured-output use case). Tag picking + 200–400-word narrative is well within sonnet's wheelhouse and within the per-call cost cap (`LlmService` clamps at $1.00).
- **Max tokens:** 2048 (narrative budget ~400 words ≈ 600 output tokens; tool input adds ~200; 2× headroom).
- **Pack-ideas section:** OMITTED in v1. The Python prompt optionally includes today's playbook ranked ideas; the Rust path skips this initially. Adding it back is a Phase 5 follow-up that pulls `PlaybookStore::read_latest(date - 1)`.
- **Empty-day behaviour:** when `executions.query(date, account)` returns zero rows, the generator returns `Err(GenerateError::NoFills)`. The Tauri command maps this to a non-error empty result (`Ok(None)`) so the UI can render a "no fills to review" state distinct from "no review written yet".

## Phase index

| Phase | File | Status |
|---|---|---|
| 1 | [phase-1-llm-kind-review.md](phase-1-llm-kind-review.md) | done (commit 474f58c, 2026-05-05) |
| 2 | [phase-2-prompt-builder.md](phase-2-prompt-builder.md) | done (commit 85b5487, 2026-05-05) |
| 3 | [phase-3-tool-schema-and-parser.md](phase-3-tool-schema-and-parser.md) | done (commit 704f80f, 2026-05-05) |
| 4 | [phase-4-summary-builder.md](phase-4-summary-builder.md) | done (commit 79a6f2b, 2026-05-05) |
| 5 | [phase-5-generator-orchestrator.md](phase-5-generator-orchestrator.md) | done (commit 67f71a7, 2026-05-05) |
| 6 | [phase-6-tauri-command-and-wiring.md](phase-6-tauri-command-and-wiring.md) | done (commit 424ac20, 2026-05-05) |
| 7 | [phase-7-fe-wrapper.md](phase-7-fe-wrapper.md) | done (commit daf8310, 2026-05-05) |
| 8 | [phase-8-ui-button-and-empty-state.md](phase-8-ui-button-and-empty-state.md) | todo |

Each phase ships in a single commit. TDD red→green→refactor at every phase. Tests inline as `#[cfg(test)] mod tests` (Rust) or `__tests__/*.test.tsx` (frontend), per project convention.
