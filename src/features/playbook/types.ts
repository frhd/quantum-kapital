/**
 * Phase 7 — Today's Playbook feature types.
 *
 * Mirrors `src-tauri/src/services/playbooks/types.rs`. The MCP read
 * tool (`get_today_playbook`) and the Tauri command both serve this
 * shape.
 */

export type SetupBias = "long" | "short"

export type Conviction = "A" | "B" | "C"

export interface EvidenceRef {
  source: string
  note: string
}

export interface RankedSetup {
  symbol: string
  bias: SetupBias
  trigger: string
  entry: string
  invalidation: string
  target_1: string
  target_2?: string
  conviction: Conviction
  rationale_md: string
  evidence_refs: EvidenceRef[]
}

export interface SkipEntry {
  symbol: string
  reason: string
}

export interface Playbook {
  /** ISO date `YYYY-MM-DD`, ET trading day. */
  date: string
  account: string
  generation_id: number
  /** UTC ISO 8601. */
  generated_at: string
  ranked_setups: RankedSetup[]
  skip_list: SkipEntry[]
  llm_call_id?: string | null
}
