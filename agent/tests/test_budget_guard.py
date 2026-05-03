"""Budget guard unit tests."""

from __future__ import annotations

import pytest

import budget_guard as bg


def test_estimate_cost_known_model():
    # Sonnet 4.6: $3 in / $15 out per Mtok.
    cost = bg.estimate_call_cost("claude-sonnet-4-6", input_tokens=1_000_000, output_tokens=0)
    assert cost == pytest.approx(3.0)
    cost = bg.estimate_call_cost("claude-sonnet-4-6", input_tokens=0, output_tokens=1_000_000)
    assert cost == pytest.approx(15.0)


def test_estimate_cost_unknown_model_falls_back_to_opus():
    # Unknown model shouldn't silently zero out — should over-count to opus rate.
    cost_known = bg.estimate_call_cost("claude-opus-4-7", 1000, 1000)
    cost_unknown = bg.estimate_call_cost("does-not-exist", 1000, 1000)
    assert cost_unknown == pytest.approx(cost_known)


def test_parse_global_status_canonical_field_names():
    s = bg.parse_global_status({"daily_usd_cap": 10.0, "daily_usd_spent": 4.0})
    assert s.daily_usd_cap == 10.0
    assert s.daily_usd_spent == 4.0
    assert s.fraction_used == pytest.approx(0.4)


def test_parse_global_status_alternate_field_names():
    s = bg.parse_global_status({"cap_usd": 10.0, "spent_usd": 7.5})
    assert s.fraction_used == pytest.approx(0.75)


def test_parse_global_status_zero_cap():
    s = bg.parse_global_status({"daily_usd_cap": 0, "daily_usd_spent": 0})
    assert s.fraction_used == 1.0  # treats unconfigured cap as exhausted


def test_check_global_passes_when_below_threshold():
    guard = bg.BudgetGuard(per_loop_usd=0.5, abort_if_global_spend_above=0.5)
    guard.check_global(bg.GlobalBudgetStatus(daily_usd_cap=10.0, daily_usd_spent=2.0))


def test_check_global_raises_at_threshold():
    guard = bg.BudgetGuard(per_loop_usd=0.5, abort_if_global_spend_above=0.5)
    with pytest.raises(bg.GlobalBudgetExhausted):
        guard.check_global(bg.GlobalBudgetStatus(daily_usd_cap=10.0, daily_usd_spent=5.0))


def test_record_increments_spent():
    guard = bg.BudgetGuard(per_loop_usd=0.5, abort_if_global_spend_above=0.5)
    guard.record("claude-sonnet-4-6", 1000, 200)
    # 1000 * 3/Mtok + 200 * 15/Mtok = 0.003 + 0.003 = 0.006
    assert guard.spent_usd == pytest.approx(0.006)


def test_ensure_can_spend_raises_when_projected_busts_cap():
    guard = bg.BudgetGuard(per_loop_usd=0.10, abort_if_global_spend_above=0.5)
    guard.spent_usd = 0.09
    with pytest.raises(bg.BudgetExceeded):
        guard.ensure_can_spend(projected_usd=0.05)


def test_ensure_can_spend_passes_under_cap():
    guard = bg.BudgetGuard(per_loop_usd=0.10, abort_if_global_spend_above=0.5)
    guard.spent_usd = 0.05
    guard.ensure_can_spend(projected_usd=0.02)  # 0.07 < 0.10


def test_remaining_clamps_to_zero():
    guard = bg.BudgetGuard(per_loop_usd=0.10, abort_if_global_spend_above=0.5)
    guard.spent_usd = 0.20
    assert guard.remaining_usd == 0.0


def test_record_uses_envelope_cost_when_provided():
    """The CLI backend's `total_cost_usd` parses into
    `LlmResponse.cost_usd`; the loop threads it through `record(...)`.
    A non-zero envelope cost wins over the per-token estimate so the
    ledger reflects what the subscription actually charged."""
    guard = bg.BudgetGuard(per_loop_usd=1.0, abort_if_global_spend_above=0.5)
    cost = guard.record(
        "claude-sonnet-4-6",
        input_tokens=1000,
        output_tokens=200,
        envelope_cost_usd=0.0078,
    )
    # Estimate would be 0.006; envelope value wins.
    assert cost == pytest.approx(0.0078)
    assert guard.spent_usd == pytest.approx(0.0078)


def test_record_falls_back_to_estimate_when_envelope_zero():
    """Subscription-mode often reports total_cost_usd=0. Treating that
    as "free" would defeat the kill-switch — fall back to the per-token
    estimate so the cap still trips deterministically."""
    guard = bg.BudgetGuard(per_loop_usd=1.0, abort_if_global_spend_above=0.5)
    cost = guard.record(
        "claude-sonnet-4-6",
        input_tokens=1000,
        output_tokens=200,
        envelope_cost_usd=0.0,
    )
    assert cost == pytest.approx(0.006)


def test_record_falls_back_to_estimate_when_envelope_none():
    """The Anthropic API client doesn't surface a per-call cost.
    `cost_usd=None` ⇒ estimate."""
    guard = bg.BudgetGuard(per_loop_usd=1.0, abort_if_global_spend_above=0.5)
    cost = guard.record(
        "claude-sonnet-4-6", 1000, 200, envelope_cost_usd=None
    )
    assert cost == pytest.approx(0.006)
