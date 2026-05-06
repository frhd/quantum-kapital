import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::tilt_guard`. Wire shape matches `TiltStatus` /
// `TiltEpisodeView` from the Rust serde derives.

export type TiltTriggerKind = "cum_r_negative" | "two_consecutive_losses"

export type TiltReleaseKind = "auto" | "manual_override" | "session_end"

export interface TiltEpisodeView {
  id: number
  account: string
  /** UTC ISO 8601. */
  triggered_at: string
  trigger_kind: TiltTriggerKind | string
  cumulative_r: number
  consecutive_losses: number
  /** UTC ISO 8601. */
  auto_reset_at: string
  /** UTC ISO 8601. NULL while the episode is open. */
  released_at: string | null
  release_kind: TiltReleaseKind | string | null
  release_reason: string | null
}

export interface TiltStatus {
  account: string
  paused: boolean
  episode: TiltEpisodeView | null
  /** Cumulative-R floor effective for today (-3.0 default; -2.0 after override the prev trading day). */
  day_threshold_cum_r: number
  /** Cumulative R observed so far today. */
  cumulative_r_today: number
  closed_trade_count_today: number
}

export interface TiltActivatedPayload {
  episode_id: number
  account: string
  trigger_kind: string
  cumulative_r: number
  /** UTC ISO 8601. */
  auto_reset_at: string
}

export interface TiltReleasedPayload {
  episode_id: number
  account: string
  release_kind: string
}

export async function tiltGuardStatus(): Promise<TiltStatus> {
  return await invoke("tilt_guard_status")
}

export async function tiltGuardOverride(reason: string): Promise<TiltStatus> {
  return await invoke("tilt_guard_override", { input: { reason } })
}

export async function tiltGuardHistory(days?: number): Promise<TiltEpisodeView[]> {
  return await invoke("tilt_guard_history", { input: days != null ? { days } : null })
}

// ----- formatting helpers -----

export const TILT_TRIGGER_LABELS: Record<string, string> = {
  cum_r_negative: "Cumulative-R floor",
  two_consecutive_losses: "Two consecutive losses",
}

export const TILT_RELEASE_LABELS: Record<string, string> = {
  auto: "Auto-reset",
  manual_override: "Manual override",
  session_end: "Session end",
}

export function formatRelativeReset(utcIso: string): string {
  const reset = new Date(utcIso)
  if (Number.isNaN(reset.getTime())) return "—"
  const ms = reset.getTime() - Date.now()
  if (ms <= 0) return "now"
  const minutes = Math.round(ms / 60_000)
  if (minutes < 60) return `${minutes}m`
  const hours = Math.round(minutes / 60)
  if (hours < 24) return `${hours}h`
  const days = Math.round(hours / 24)
  return `${days}d`
}

export function formatEtTime(utcIso: string): string {
  const d = new Date(utcIso)
  if (Number.isNaN(d.getTime())) return "—"
  // Eastern time hardcoded to ET (matches market_calendar::et_offset).
  return d.toLocaleString("en-US", {
    timeZone: "America/New_York",
    month: "short",
    day: "numeric",
    hour: "2-digit",
    minute: "2-digit",
  })
}
