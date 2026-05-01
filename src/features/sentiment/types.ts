/**
 * Phase 3 — social-sentiment frontend types.
 *
 * Mirrors the Rust shapes returned by `social_*` Tauri commands. Keep
 * `SocialSentimentRow` in lock-step with
 * `services/social_sentiment/types.rs::SocialSentimentRow`.
 */

export type SentimentSourceId = "reddit_wsb" | "stocktwits" | "apewisdom"

export type SentimentLabel = "bullish" | "bearish" | "neutral"

export interface SocialSentimentRow {
  id: number
  source: string
  symbol: string
  score: number | null
  mentions_24h: number | null
  sentiment_label: string | null
  rank: number | null
  raw_payload: string
  is_stale: boolean
  fetched_at: number
}
