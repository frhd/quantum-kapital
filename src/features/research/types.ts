/**
 * Phase 02 — research-artifact frontend types.
 *
 * Mirrors the Rust shapes returned by `research_*` Tauri commands. Keep
 * the EvidenceRef variants and Conviction in lock-step with
 * `services/research_notes/mod.rs`.
 */

export type Conviction = "A" | "B" | "C"

export type EvidenceRef =
  | { type: "alert"; id: number }
  | { type: "news"; cache_id: number }
  | { type: "setup"; id: number }
  | { type: "bar_range"; symbol: string; from: string; to: string }

export interface ResearchNote {
  id: number
  symbol: string
  body_md: string
  conviction: Conviction | null
  evidence_refs: EvidenceRef[]
  written_by: string
  written_at: string
  setup_id: number | null
  alert_id: number | null
}

export interface RankedIdea {
  symbol: string
  thesis_md: string
  conviction: Conviction | null
  entry_zone: string | null
  invalidation: string | null
  evidence_refs: EvidenceRef[]
}

export interface AgentMorningPack {
  date: string
  ranked_ideas: RankedIdea[]
  written_by: string
  written_at: string
}

export interface McpAuditEntry {
  id: number
  tool: string
  input: unknown
  result_summary: string | null
  caller: string | null
  called_at: string
}

export interface ResearchNoteWrittenPayload {
  note_id: number
  symbol: string
  alert_id: number | null
  setup_id: number | null
}

export interface AgentMorningPackWrittenPayload {
  date: string
  idea_count: number
}

export interface AlertDecisionRecordedPayload {
  alert_id: number
  decision: string
  note_id: number | null
}
