# Phase 1 — `LlmKind::Review` variant

> Part of [In-app trade-review generator](master.md). See master for invariants.

**Status:** done (commit 474f58c, 2026-05-05)

**Depends on:** —

**Goal:** Extend the closed `LlmKind` enum with a `Review` variant so the generator's `LlmService::message` calls log to `llm_calls` as `kind='review'`. Required by every later phase that exercises the LLM seam.

**Why this is its own phase:** the variant addition is one line of source code but touches the `LlmKind::as_str()` match arm + the public re-export surface, and it must land before any code that constructs `LlmRequest { kind: LlmKind::Review, ... }`. Splitting it out keeps Phase 5's diff focused on the orchestrator.

## Files

**Modify:**
- `src-tauri/src/services/llm_service/types.rs` — add `Review` variant + `as_str` arm.

**No tests need changing.** Existing `as_str` round-trip behaviour is exercised implicitly by the suite; the new arm is covered by Phase 5's generator tests. The match exhaustiveness check would surface a missed arm at compile time anyway.

## Steps

- [ ] **Step 1: Add the variant.**

Open `src-tauri/src/services/llm_service/types.rs` and edit lines 4–9:

```rust
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum LlmKind {
    Thesis,
    Decay,
    News,
    Ranker,
    Review,
}
```

- [ ] **Step 2: Add the `as_str` arm.**

Edit lines 12–19 in the same file:

```rust
impl LlmKind {
    pub fn as_str(&self) -> &'static str {
        match self {
            LlmKind::Thesis => "thesis",
            LlmKind::Decay => "decay",
            LlmKind::News => "news",
            LlmKind::Ranker => "ranker",
            LlmKind::Review => "review",
        }
    }
}
```

- [ ] **Step 3: Compile-check.**

Run: `cargo check --manifest-path src-tauri/Cargo.toml`
Expected: clean. No new warnings. (If a `match` somewhere is non-exhaustive on `LlmKind`, fix it inline — exhaustiveness errors should be rare since the only other consumer is `as_str` itself.)

- [ ] **Step 4: Format + clippy.**

Run: `cd src-tauri && cargo fmt --all -- --check && cargo clippy --all-targets --all-features -- -D warnings`
Expected: clean.

- [ ] **Step 5: Run the lib test suite (sanity).**

Run: `cd src-tauri && cargo test --lib`
Expected: same green-set as before this phase. The known pre-existing flake `services::decay_watcher::tests::respects_budget_kill_switch` is unrelated and may still fail — confirm it was already failing on the parent commit before treating as a regression.

- [ ] **Step 6: Commit.**

```bash
git add src-tauri/src/services/llm_service/types.rs
git commit -m "$(cat <<'EOF'
feat(llm): add LlmKind::Review variant

Required by the in-app trade-review generator (next phase) so its
LlmService::message calls log to llm_calls as kind='review'.

Co-Authored-By: Claude Opus 4.7 (1M context) <noreply@anthropic.com>
EOF
)"
```
