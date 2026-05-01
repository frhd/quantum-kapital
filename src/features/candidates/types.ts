/**
 * Phase 4 — candidate-universe frontend types.
 *
 * Mirrors the Rust shapes returned by `candidates_*` Tauri commands.
 * Keep `Candidate` / `CandidateSource` in lock-step with
 * `services/candidate_universe/types.rs::Candidate`.
 */

export interface CandidateSource {
  source: string
  score: number
  rank: number | null
  meta: Record<string, unknown>
  last_seen: number
}

export interface Candidate {
  symbol: string
  score: number
  sources: CandidateSource[]
  reason_md: string | null
  first_seen: number
  last_seen: number
  decay_at: number
  promoted_at: number | null
}

export interface CandidatesQuery {
  source?: string | null
  min_score?: number | null
  since_unix?: number | null
  include_promoted?: boolean
  limit?: number | null
}

export interface CandidatesRefreshOutcome {
  surge_upserted: number
  surge_auto_promoted: number
  decay_evicted: number
}
