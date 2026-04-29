# Contributing to Quantum Kapital

This file holds project conventions that affect **how code is shaped**. For build commands, architecture, IBKR integration details, and TDD workflow, see `CLAUDE.md` — that remains the canonical engineering reference.

## File-size limits

Files that grow without bound become hard to navigate, hard to review, and tend to accumulate unrelated responsibilities. To keep that drift in check, the repo has soft caps:

| Language | Soft cap | Hard cap |
|---|---|---|
| Rust (`*.rs`) | **300 lines** | 500 lines |
| TypeScript / TSX (`*.ts`, `*.tsx`) | **200 lines** | 350 lines |

**Soft cap** = if your change pushes a file past this, the PR description must either (a) include a sentence justifying why it should stay one file, or (b) include the split.

**Hard cap** = no exceptions in normal review. If you need to land code that pushes a file past the hard cap as a stopgap, add a top-of-file justifier and open a follow-up issue:

```rust
// allow-large-file: this module is the IBKR API adapter; splitting is tracked
// in impl/phase-25-cleanup-pass.md and will land before Phase 26.
```

```tsx
// allow-large-file: <one-line reason + link to follow-up>
```

The justifier comment is the contract. A file without it that exceeds the hard cap blocks the merge.

### How to check

```bash
# Rust files over the soft cap
find src-tauri/src -name '*.rs' | xargs wc -l | awk '$1 > 300' | sort -rn

# Frontend files over the soft cap
find src -name '*.ts' -o -name '*.tsx' | xargs wc -l | awk '$1 > 200' | sort -rn
```