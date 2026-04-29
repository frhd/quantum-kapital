export type TrackerSource = "scanner" | "manual" | "news"

export type TrackerStatus = "watching" | "in_play" | "setup_active" | "cool_down"

export type BuiltInStrategyTag = "breakout" | "episodic_pivot" | "parabolic_short"

export type StrategyTag = BuiltInStrategyTag | string

export const BUILT_IN_TAGS: ReadonlyArray<{ value: BuiltInStrategyTag; label: string }> = [
  { value: "breakout", label: "Breakout" },
  { value: "episodic_pivot", label: "Episodic Pivot" },
  { value: "parabolic_short", label: "Parabolic Short" },
]

export const STATUS_LABELS: Record<TrackerStatus, string> = {
  watching: "Watching",
  in_play: "In Play",
  setup_active: "Setup Active",
  cool_down: "Cool Down",
}

export interface TrackedTicker {
  symbol: string
  source: TrackerSource
  source_meta: Record<string, unknown> | null
  status: TrackerStatus
  tags: StrategyTag[]
  notes: string | null
  added_at: string
  last_checked_at: string | null
  in_play_until: string | null
  cool_down_until: string | null
}

export interface AddToTrackerPrefill {
  symbol: string
  source: TrackerSource
  sourceMeta?: Record<string, unknown> | null
  tags?: StrategyTag[]
  notes?: string
}

export interface TickerSentiment {
  ticker: string
  relevance_score: number
  ticker_sentiment_score: number
  ticker_sentiment_label: string
}

export interface NewsItem {
  time_published: string
  title: string
  summary: string
  source: string
  url: string
  overall_sentiment_score: number | null
  overall_sentiment_label: string | null
  ticker_sentiment: TickerSentiment[]
}

export type BarSize =
  | "Sec1"
  | "Sec5"
  | "Sec15"
  | "Sec30"
  | "Min1"
  | "Min2"
  | "Min3"
  | "Min5"
  | "Min15"
  | "Min20"
  | "Min30"
  | "Hour1"
  | "Day1"

export interface HistoricalBar {
  time: string
  open: number
  high: number
  low: number
  close: number
  volume: number
  wap: number
  count: number
}

export type SetupStatus = "active" | "invalidated" | "completed"

export type Direction = "long" | "short"

export interface TargetLevel {
  label: string
  price: number
}

export interface InvalidationLevel {
  label: string
  price: number
  reason: string
}

export interface ThesisStructured {
  thesis_md: string
  conviction: "A" | "B" | "C"
  invalidation_levels: InvalidationLevel[]
  risk_notes: string
}

export interface Setup {
  id: number
  symbol: string
  strategy: string
  direction: Direction
  detected_at: string
  trigger_price: number
  stop_price: number
  targets: TargetLevel[]
  raw_signals: unknown
  thesis: string | null
  /** Phase 17: full structured thesis JSON (markdown + conviction + invalidation_levels + risk_notes). */
  thesis_json: ThesisStructured | null
  status: SetupStatus
  invalidated_at: string | null
  invalidation_reason: string | null
}

// --- Tracker / scheduler events emitted by the Rust backend ---
//
// AppEvent is wire-tagged as { type, data }. The variants below mirror
// `src-tauri/src/events/emitter.rs`; only the tracker subset is typed
// here since other features consume their own events directly.

export interface SetupDetectedPayload {
  setup: Setup
  thesis: string | null
}

export interface SetupInvalidatedPayload {
  setup_id: number
  symbol: string
  reason: string
}

export interface TickerStatusChangedPayload {
  symbol: string
  from: TrackerStatus
  to: TrackerStatus
}

export type TrackerEvent =
  | { kind: "setup-detected"; payload: SetupDetectedPayload }
  | { kind: "setup-invalidated"; payload: SetupInvalidatedPayload }
  | { kind: "ticker-status-changed"; payload: TickerStatusChangedPayload }
