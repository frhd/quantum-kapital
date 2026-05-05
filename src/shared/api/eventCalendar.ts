import { invoke } from "@tauri-apps/api/core"
import type { Setup } from "../../features/tracker/types"

// Mirrors `services::event_calendar::types::SkipReason` and the
// `Blackout` JSON the gate writes onto skipped setups.

export type SkipReason = "earnings_blackout" | "fomc_blackout"

export type BlackoutKind = "earnings" | "fomc"

export type BlackoutConfidence = "estimated" | "confirmed"

/** Persisted on `setups.skip_window_json` and surfaced to the UI. */
export interface BlackoutWindow {
  kind: BlackoutKind
  /** UTC ISO 8601 — first instant inside the window. */
  start: string
  /** UTC ISO 8601 — first instant *after* the window. */
  end: string
  /** ISO `YYYY-MM-DD` — pivot date the window is anchored to. */
  pivot_date: string
  reason: string
  /** `"alpha_vantage"` / `"manual"` / `"fomc_dataset"` / `"unknown"`. */
  source: string
  confidence: BlackoutConfidence
}

export interface EarningsLookup {
  date: string
  confidence: BlackoutConfidence
  source: string
  trading_days_until: number
}

export interface EventCalendarLookup {
  symbol: string
  next_earnings: EarningsLookup | null
  days_to_fomc: number | null
  fomc_dataset_stale: boolean
}

/** Look up next-earnings + days-to-FOMC for a symbol. */
export async function eventCalendarLookup(symbol: string): Promise<EventCalendarLookup> {
  return await invoke("event_calendar_lookup", { symbol })
}

/** Discard the earnings cache so the next lookup re-fetches. */
export async function eventCalendarForceRefresh(): Promise<void> {
  return await invoke("event_calendar_force_refresh")
}

/**
 * Override a blackout-skipped setup. Produces a fresh non-skipped
 * `setups` row, audited via `setup_blackout_overrides`. The `reason`
 * must be non-empty — the backend rejects the call otherwise.
 */
export async function setupOverrideBlackout(
  setupId: number,
  reason: string,
  actor?: string,
): Promise<Setup> {
  return await invoke("setup_override_blackout", { setupId, reason, actor })
}

/** List skipped (`skipped_reason !== null`) setups. */
export async function trackerGetSkippedSetups(since?: string | null): Promise<Setup[]> {
  return await invoke("tracker_get_skipped_setups", { since: since ?? null })
}

export const SKIP_REASON_LABELS: Record<SkipReason, string> = {
  earnings_blackout: "Earnings blackout",
  fomc_blackout: "FOMC blackout",
}

/** Short copy for the SetupCard badge. */
export function describeSkip(setup: Setup): string {
  if (!setup.skipped_reason) return ""
  const window = setup.skip_window_json
  if (setup.skipped_reason === "earnings_blackout" && window?.pivot_date) {
    return `Earnings ${window.pivot_date}`
  }
  if (setup.skipped_reason === "fomc_blackout") {
    return "FOMC day"
  }
  return SKIP_REASON_LABELS[setup.skipped_reason]
}
