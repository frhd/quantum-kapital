# Contributing to Quantum Kapital

This file holds project conventions that affect **how code is shaped**. For build commands, architecture, IBKR integration details, and TDD workflow, see `CLAUDE.md` — that remains the canonical engineering reference.

## File-size limits

Files that grow without bound become hard to navigate, hard to review, and tend to accumulate unrelated responsibilities. To keep that drift in check, the repo has soft caps:

| Language | Soft cap | Hard cap |
|---|---|---|
| Rust (`*.rs`) | **500 lines** | 800 lines |
| TypeScript / TSX (`*.ts`, `*.tsx`) | **300 lines** | 500 lines |

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
find src-tauri/src -name '*.rs' | xargs wc -l | awk '$1 > 500' | sort -rn

# Frontend files over the soft cap
find src -name '*.ts' -o -name '*.tsx' | xargs wc -l | awk '$1 > 300' | sort -rn
```

### Why these numbers

- **Rust at 500/800**: Tauri command + service code is dense; 500 fits comfortably on one screen with vim splits, 800 is roughly the top of what a reviewer can hold in working memory in one pass.
- **TSX at 300/500**: React components past 300 lines almost always hide a subcomponent that wants to be extracted; 500 is the point where review feedback starts asking the same factor-out question every time.

These caps target **structural debt**, not cleverness. A 600-line file with one well-defined responsibility is preferable to four 150-line files that share private helpers across module boundaries — but in practice, files that hit the cap rarely have one responsibility.

### Existing offenders

These files exceeded the cap before the rule landed and are scheduled for splits in `impl/phase-25-cleanup-pass.md`:

- `src-tauri/src/ibkr/client.rs` (754 lines)
- `src-tauri/src/services/tracker_service/mod.rs` (662 lines)
- `src-tauri/src/services/projection_service.rs` (641 lines)
- `src-tauri/src/services/financial_data_service.rs` (553 lines)
- `src/features/tracker/components/Watchlist.tsx` (326 lines)

Until Phase 25 lands, contributions touching these files are exempt from the rule for the lines they don't add. New files always follow the cap.

## See also

- `CLAUDE.md` — full engineering guide (build, test, architecture, IBKR setup)
- `impl.md` — phased implementation plan and dependency graph
- `.pre-commit-config.yaml` — lint/format gates that run on every commit
