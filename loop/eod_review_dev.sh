#!/usr/bin/env bash
# Run one EOD review iteration. Intended for both manual dev use and as the
# command invoked by cron. Idempotent: the trading-calendar check inside the
# Python loop early-exits on weekends/holidays, and `append_journal_entry`
# upserts on (date, section) by the Rust side — re-running on the same day
# overwrites that day's agent section without touching user notes.
#
# Usage:
#   ./loop/eod_review_dev.sh             # real run
#   ./loop/eod_review_dev.sh --dry-run   # skips the journal write
#   ./loop/eod_review_dev.sh --force     # run on weekends/holidays
#
# Any extra args are forwarded into the Python entrypoint.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
agent_dir="$repo_root/agent"

if [[ -f "$agent_dir/.env" ]]; then
    set -a
    # shellcheck disable=SC1091
    source "$agent_dir/.env"
    set +a
fi

cd "$agent_dir"

if ! command -v uv >/dev/null 2>&1; then
    export PATH="$HOME/.local/bin:$PATH"
fi

if [[ ! -d ".venv" ]]; then
    echo "[eod_review_dev] .venv missing — running uv venv + install" >&2
    uv venv
    uv pip install -e ".[dev]"
fi

exec uv run qk-eod-review "$@"
