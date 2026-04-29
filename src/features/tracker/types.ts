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
