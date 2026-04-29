# CLAUDE.md

A Tauri 2 desktop app pairing an Interactive Brokers connection with a strategy-driven **Tracker** subsystem (watchlist → detectors → LLM reasoning → alerts). Surveillance only — order placement exists but is not wired into the tracker.

Stack-specific rules:

- **`src-tauri/CLAUDE.md`** — Rust backend, IBKR adapter, services, tracker pipeline, cargo commands
- **`src/CLAUDE.md`** — React 19 + Vite frontend, feature folders, Tauri command wrappers

## Test discipline

TDD with red → green → refactor at every phase. Tests use the `IbkrClientTrait` seam (`MockIbkrClient` in `src-tauri/src/ibkr/mocks.rs`), never a live IBKR client. Any service that touches IBKR should be testable through that trait.

## Pre-commit

`.pre-commit-config.yaml` runs `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, and `eslint` on every commit. **Never bypass with `--no-verify`** — fix the underlying issue. If clippy fails on code you didn't touch, that's a real regression to investigate.

## File-size caps

Soft 300 (Rust) / 200 (TS/TSX); hard 500 / 350. Past the hard cap requires an `// allow-large-file: <reason>` justifier at the top of the file. Full rules and check commands in `CONTRIBUTING.md`.

## Cargo from repo root

`src-tauri/Cargo.toml` is the only manifest — no workspace at the root. From the repo root, every cargo command needs `--manifest-path src-tauri/Cargo.toml`. Backend command examples (and the `cd src-tauri/` shortcut) are in `src-tauri/CLAUDE.md`.
