#!/usr/bin/env bash
# Phase 4 (quant-decisions roadmap) — R-edge + discipline scoring.
#
# Master Hard Invariant 9 (CI-grep): "`net_pnl\s*/\s*100` must not
# appear in non-test grading code after P4."
#
# Rationale: the legacy v1 grade formula
# (`score = clamp(net_pnl/100, ±25) + Σ(tag_weights)`) conflated edge
# with discipline and ignored risk taken. Phase 4 replaced it with two
# separately-surfaced numbers (`score_v2` = Σ(realized_R × conviction
# weight); `discipline_v2` = Σ(tag_weights)). The legacy term must not
# regress into production code under `services/trade_reviews/`. Test
# fixtures may still reference the old number for backward-read
# regression coverage; this check scopes to non-`tests`/`#[cfg(test)]`
# code only.

set -euo pipefail

ROOT="$(cd "$(dirname "$0")/../.." && pwd)"

# `git ls-files` keeps us inside the working tree (no target/), and
# the `\.rs$` filter scopes to Rust sources. We grep the production
# subtree under `services/trade_reviews/` plus the orchestrator under
# `mcp/tools/write_trade_review.rs` — anywhere a v2 row gets written.
# Test files and their fixture modules are excluded by file-name and
# by the `#[cfg(test)]` skip rule (we drop matching files whose path
# ends in `tests.rs` or contains `__tests__`).
# Real-code matches: lines that actually compute or compare against
# `net_pnl/100`. The leading `[^/]` (non-comment-leader char) plus a
# preceding non-quote character avoids the `//` doc-comment / string-
# literal references the master plan and module docs intentionally
# carry to *describe* the retired formula. We only flag a match when
# `net_pnl` and `/ 100` appear inline as expression operands.
hits="$(git -C "$ROOT" ls-files 'src-tauri/src/services/trade_reviews/' \
        'src-tauri/src/mcp/tools/write_trade_review.rs' \
    | grep -E '\.rs$' \
    | grep -vE '(/tests\.rs$|/__tests__/|/test_support/)' \
    | xargs grep -nE 'net_pnl\s*/\s*100' 2>/dev/null \
    | grep -vE '^\s*//' \
    | grep -vE '^[^:]+:[0-9]+:\s*//' \
    | grep -vE ':\s*"[^"]*net_pnl' \
    || true)"

if [[ -n "$hits" ]]; then
    echo "ERROR: Phase-4 grading invariant violated."
    echo
    echo "The legacy 'net_pnl / 100' term is forbidden in Phase 4 production"
    echo "scoring code. Replace with score_v2 (Σ realized_R × conviction weight)"
    echo "and surface discipline_v2 (Σ tag_weights) separately — never sum them."
    echo
    echo "Found violations:"
    echo "$hits"
    exit 1
fi

echo "OK: no 'net_pnl / 100' occurrences in Phase-4 production scoring code."
