#!/usr/bin/env bash
# Run one morning-sweep iteration. Intended for both manual dev use and as the
# command invoked by cron. Idempotent: the trading-calendar check inside the
# Python loop early-exits on weekends/holidays, and `write_morning_pack` is
# upserted on `date` PK by the Rust side — re-running on the same day
# overwrites that day's pack.
#
# Usage:
#   ./loop/morning_sweep_dev.sh             # real run
#   ./loop/morning_sweep_dev.sh --dry-run   # skips the morning_pack DB write
#   ./loop/morning_sweep_dev.sh --shadow    # tags pack as shadow output
#
# Any extra args are forwarded into the Python entrypoint.

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
agent_dir="$repo_root/agent"

# Pull ANTHROPIC_API_KEY etc. from the agent .env if present.
if [[ -f "$agent_dir/.env" ]]; then
    set -a
    # shellcheck disable=SC1091
    source "$agent_dir/.env"
    set +a
fi

cd "$agent_dir"

if ! command -v uv >/dev/null 2>&1; then
    # Fall back to a PATH that picks up the default uv install location.
    export PATH="$HOME/.local/bin:$PATH"
fi

if [[ ! -d ".venv" ]]; then
    echo "[morning_sweep_dev] .venv missing — running uv venv + install" >&2
    uv venv
    uv pip install -e ".[dev]"
fi

exec uv run morning_sweep "$@"
