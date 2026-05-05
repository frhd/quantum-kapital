#!/usr/bin/env bash
# Phase 3 (quant-decisions roadmap) — surveillance-plus-confirmed-execution
# invariant.
#
# Master Hard Invariant 9 (CI-grep): "Any file under `services/` or
# `strategies/` that calls `place_order` directly (not through
# `OrderTicket::with_brackets`) fails CI after P3."
#
# Rationale: the only path to a live IBKR parent order is the
# `OrderTicket` chokepoint, fronted by the `order_ticket_take_setup`
# Tauri command. A scheduler / detector / agent / LLM bypassing this
# would violate Hard Invariant 1 (surveillance + confirmed execution).
#
# Allowed call sites for `IbkrClient::place_order(...)`:
#   - `src-tauri/src/ibkr/client/orders.rs` — the place_order
#     primitive itself + `place_bracket` (which calls into the underlying
#     ibapi `client.place_order` to ship parent + stop + targets).
#   - `src-tauri/src/ibkr/commands/trading.rs` — `ibkr_place_order` Tauri
#     command (legacy single-leg path; surveillance-only).
#   - Anything in `src-tauri/src/services/order_ticket/` — by design.
#   - Tests (`#[cfg(test)]` blocks, `tests.rs` modules) and `tests/`
#     integration tests — exercise the primitive against the mock.
#
# Anywhere else under `services/` or `strategies/` is forbidden.
# `.place_order(` matches both `client.place_order(...)` and
# `client_clone.place_order(...)`; the leading dot makes the regex
# pickier than a bare `place_order(`, which would also catch the
# Tauri command name string and our own `OrderTicket::with_brackets`
# helpers that don't actually call the primitive.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

# Use git ls-files so we never scan target/ or vendored crates.
# Bare directory arguments recurse, including top-level *.rs files;
# a `**/*.rs` glob would silently miss those.
hits="$(git -C "$ROOT" ls-files \
    'src-tauri/src/services/' \
    'src-tauri/src/strategies/' \
    | grep -E '\.rs$' \
    | grep -v '^src-tauri/src/services/order_ticket/' \
    | xargs grep -nE '\.place_order\(' 2>/dev/null \
    || true)"

if [[ -n "$hits" ]]; then
    echo "ERROR: surveillance-plus-confirmed-execution invariant violated."
    echo
    echo "Direct \`.place_order(\` calls are only allowed in:"
    echo "  - src-tauri/src/ibkr/client/orders.rs"
    echo "  - src-tauri/src/ibkr/commands/trading.rs"
    echo "  - src-tauri/src/services/order_ticket/*"
    echo
    echo "Found violations:"
    echo "$hits"
    echo
    echo "Route the call through OrderTicket::with_brackets (P3 chokepoint),"
    echo "or document an explicit exception in master plan invariants."
    exit 1
fi

echo "OK: no direct place_order calls outside the OrderTicket chokepoint."
