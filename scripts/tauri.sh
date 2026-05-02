#!/usr/bin/env bash
# Wrapper around the `tauri` CLI invoked via `pnpm tauri ...`.
#
# When the subcommand is `dev`, sets tracing-friendly RUST_LOG defaults and
# tees combined stdout/stderr to /tmp/qk-tauri.log so debugging sessions
# (Claude Code or human) can grep one shared file. Other subcommands
# (`build`, `info`, `icon`, ...) pass through to the CLI untouched.
#
# Overrides:
#   QK_TAURI_LOG=/path/to/log.log   redirect the log file
#   RUST_LOG=...                    bypass the default filter
#
# The log file is truncated on each fresh `pnpm tauri dev` invocation
# (default `tee` behaviour); subsequent hot-reloads append within the
# same session. Set QK_TAURI_LOG_APPEND=1 to keep history across sessions.

set -euo pipefail

LOG_FILE="${QK_TAURI_LOG:-/tmp/qk-tauri.log}"
TEE_FLAGS=()
if [ "${QK_TAURI_LOG_APPEND:-0}" = "1" ]; then
  TEE_FLAGS+=(-a)
fi

if [ "${1-}" = "dev" ]; then
  shift
  export RUST_LOG="${RUST_LOG:-info,quantum_kapital_lib=debug,rmcp=info,ibapi=info}"
  echo "qk: tauri dev with RUST_LOG=$RUST_LOG → $LOG_FILE" >&2
  tauri dev "$@" 2>&1 | tee "${TEE_FLAGS[@]}" "$LOG_FILE"
else
  exec tauri "$@"
fi
