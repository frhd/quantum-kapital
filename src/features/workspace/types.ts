import type { NewsItem } from "../tracker/types"

export type WorkspaceTabId =
  | "overview"
  | "projection"
  | "news"
  | "research"
  | "alerts"
  | "history"
  | "watchlist"

/**
 * Workspace Phase 3 — shape returned by the cache-only `news_get_cached`
 * Tauri command. `fetched_at_unix === 0` means "no cache row" (vs.
 * "cached but empty"), so the panel can distinguish a quiet symbol from
 * a producer outage. `verdict_json` is the raw `news_verdict_json`
 * column verbatim — null until `news_interpreter` has run.
 */
export interface CachedTickerNews {
  symbol: string
  items: NewsItem[]
  verdict_json: string | null
  fetched_at_unix: number
}

/**
 * Workspace Phase 3 — best-effort decoding of `news_verdict_json` into
 * the `NewsVerdict` shape `news_interpreter` writes. Kept loose so a
 * future schema change doesn't crash the panel — unrecognized fields
 * fall back to the raw JSON display path.
 */
export interface NewsVerdict {
  tone?: "bullish" | "bearish" | "neutral" | string
  ep_worthy?: boolean
  parabolic_risk?: boolean
  summary?: string
}

export const WORKSPACE_TAB_ORDER: WorkspaceTabId[] = [
  "overview",
  "projection",
  "news",
  "research",
  "alerts",
  "history",
  "watchlist",
]

export const WORKSPACE_TAB_LABELS: Record<WorkspaceTabId, string> = {
  overview: "Overview",
  projection: "Projection",
  news: "News",
  research: "Research",
  alerts: "Alerts",
  history: "History",
  watchlist: "Watchlist",
}
