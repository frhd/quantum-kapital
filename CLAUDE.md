# CLAUDE.md

A Tauri 2 desktop app pairing an Interactive Brokers connection with a strategy-driven **Tracker** subsystem (watchlist → detectors → LLM reasoning → alerts). Surveillance only — order placement exists but is not wired into the tracker.

Stack-specific rules:

- **`src-tauri/CLAUDE.md`** — Rust backend, IBKR adapter, services, tracker pipeline, cargo commands
- **`src/CLAUDE.md`** — React 19 + Vite frontend, feature folders, Tauri command wrappers

## Running the app

`pnpm tauri dev` from the repo root brings up Vite + the Rust backend with hot reload. Frontend-only and prod-build commands are in `src/CLAUDE.md`.

## Secrets

Backend reads `src-tauri/.env` (Alpha Vantage `ALPHA_VANTAGE_API_KEY`, etc.). The frontend never sees these — all external API calls go through Rust services. App falls back to mock data if the key is missing; failures here are silent in the UI, so check logs if fundamentals look stale.

## LLM budget

`LlmService` enforces a daily USD budget against the `llm_calls` ledger before every call. Any new code that calls an LLM must go through `LlmService` — never bypass it, even in tests; use the trait seam.

## Tracker is surveillance-only

The tracker pipeline (detectors → state machine → alerts) MUST NOT call order-placement code paths. Order commands exist in the IBKR adapter for manual UI use only. Wiring them into the tracker requires explicit project-level approval.

## Test discipline

TDD with red → green → refactor at every phase. Tests use the `IbkrClientTrait` seam (`MockIbkrClient` in `src-tauri/src/ibkr/mocks.rs`), never a live IBKR client. Any service that touches IBKR should be testable through that trait.

## Pre-commit

`.pre-commit-config.yaml` runs `cargo fmt --check`, `cargo clippy -D warnings`, `prettier --check`, and `eslint` on every commit. **Never bypass with `--no-verify`** — fix the underlying issue. If clippy fails on code you didn't touch, that's a real regression to investigate.

## File-size caps

Soft 300 (Rust) / 200 (TS/TSX); hard 500 / 350. Past the hard cap requires an `// allow-large-file: <reason>` justifier at the top of the file. Full rules and check commands in `CONTRIBUTING.md`.

## Cargo from repo root

`src-tauri/Cargo.toml` is the only manifest — no workspace at the root. From the repo root, every cargo command needs `--manifest-path src-tauri/Cargo.toml`. Backend command examples (and the `cd src-tauri/` shortcut) are in `src-tauri/CLAUDE.md`.
