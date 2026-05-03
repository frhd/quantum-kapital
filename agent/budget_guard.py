"""Budget enforcement for the headless research loop.

Two layers:

1. **Global (server-side)**: `LlmService` in Rust enforces a daily USD cap
   across every LLM call (UI, schedulers, agents). The agent queries this via
   the MCP `get_llm_budget_status` tool. We refuse to start the loop if more
   than `abort_if_global_spend_above` of that cap has already been used.

2. **Per-loop (client-side)**: a simple ledger this class maintains. Anthropic
   responses include `usage.input_tokens` / `usage.output_tokens`; we convert
   to USD with the per-model price table and refuse the next call when the
   loop's own running total would exceed `per_loop_usd`.

The two layers must both pass. The global one stops a runaway day; the local
one stops a runaway loop within a healthy day.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Mapping

# Per-million-token prices (USD). Mirrors `src-tauri/src/services/llm_service/prices.rs`.
# Keep in sync manually; if these drift we under- or over-count, but the Rust
# server's ledger is authoritative for the global cap.
_PRICES_USD_PER_MTOK: dict[str, tuple[float, float]] = {
    # (input, output)
    "claude-sonnet-4-6": (3.0, 15.0),
    "claude-haiku-4-5": (1.0, 5.0),
    "claude-opus-4-7": (15.0, 75.0),
}


def estimate_call_cost(model: str, input_tokens: int, output_tokens: int) -> float:
    if model not in _PRICES_USD_PER_MTOK:
        # Fallback to the most expensive published price we know — over-count
        # rather than silently zero out an unknown model.
        in_price, out_price = _PRICES_USD_PER_MTOK["claude-opus-4-7"]
    else:
        in_price, out_price = _PRICES_USD_PER_MTOK[model]
    return (input_tokens * in_price + output_tokens * out_price) / 1_000_000.0


@dataclass
class GlobalBudgetStatus:
    daily_usd_cap: float
    daily_usd_spent: float

    @property
    def fraction_used(self) -> float:
        if self.daily_usd_cap <= 0:
            return 1.0
        return self.daily_usd_spent / self.daily_usd_cap


def parse_global_status(payload: Mapping[str, Any]) -> GlobalBudgetStatus:
    """Decode the JSON returned by the MCP `get_llm_budget_status` tool.

    The tool's payload uses snake_case fields. Accept both `daily_usd_cap` and
    `cap_usd` to be tolerant if the Rust side renames one of them.
    """
    cap = float(
        payload.get("daily_usd_cap")
        or payload.get("cap_usd")
        or payload.get("daily_cap_usd")
        or 0.0
    )
    spent = float(
        payload.get("daily_usd_spent")
        or payload.get("spent_usd")
        or payload.get("daily_spend_usd")
        or 0.0
    )
    return GlobalBudgetStatus(daily_usd_cap=cap, daily_usd_spent=spent)


class BudgetExceeded(RuntimeError):
    """Raised when the loop's per-loop budget is exhausted mid-flight."""


class GlobalBudgetExhausted(RuntimeError):
    """Raised at loop start if the global daily budget is already past the
    abort threshold."""


@dataclass
class BudgetGuard:
    """Tracks the loop's accumulated spend and enforces the per-loop cap."""

    per_loop_usd: float
    abort_if_global_spend_above: float
    spent_usd: float = 0.0

    def check_global(self, status: GlobalBudgetStatus) -> None:
        if status.fraction_used >= self.abort_if_global_spend_above:
            raise GlobalBudgetExhausted(
                f"global daily budget {status.fraction_used:.0%} used "
                f"(${status.daily_usd_spent:.2f} / ${status.daily_usd_cap:.2f}) "
                f">= abort threshold {self.abort_if_global_spend_above:.0%}"
            )

    def record(
        self,
        model: str,
        input_tokens: int,
        output_tokens: int,
        envelope_cost_usd: float | None = None,
    ) -> float:
        """Record one LLM call against the loop's running total.

        `envelope_cost_usd` is the per-call USD figure parsed from the
        backend's response envelope (only the `claude_cli` backend
        surfaces this — see `llm_cli.ClaudeCliLlmClient.parse_envelope`).
        Values <= 0 are treated as missing so a zero-cost envelope under
        subscription auth still gets the per-token estimate, otherwise
        the kill-switch would think inference is free. Returns the cost
        actually charged."""
        if envelope_cost_usd is not None and envelope_cost_usd > 0:
            cost = float(envelope_cost_usd)
        else:
            cost = estimate_call_cost(model, input_tokens, output_tokens)
        self.spent_usd += cost
        return cost

    def ensure_can_spend(self, projected_usd: float = 0.0) -> None:
        """Call before the next LLM request. `projected_usd` is the optional
        worst-case estimate for the upcoming call (input tokens × in price plus
        max_output × out price). Pass 0 to just check the running total."""
        if self.spent_usd + projected_usd > self.per_loop_usd:
            raise BudgetExceeded(
                f"per-loop budget exceeded: spent ${self.spent_usd:.4f} + "
                f"projected ${projected_usd:.4f} > cap ${self.per_loop_usd:.2f}"
            )

    @property
    def remaining_usd(self) -> float:
        return max(0.0, self.per_loop_usd - self.spent_usd)
