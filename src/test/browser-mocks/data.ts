import type {
  AccountSummary,
  ConnectionStatus,
  Position,
  Quote,
} from "../../shared/types"
import type { AppConfig } from "../../shared/api/settings"
import type { Alert, MorningPack, TrackedTicker } from "../../features/tracker/types"
import type { AgentMorningPack, McpAuditEntry, ResearchNote } from "../../features/research/types"
import type { Candidate } from "../../features/candidates/types"
import type {
  CalibrationStats,
  CostAttribution,
  PredictionWithOutcome,
} from "../../features/eval/types"

export const ACCT = "DU1234567"

export const now = () => new Date().toISOString()
export const isoMinusHours = (h: number) => new Date(Date.now() - h * 3_600_000).toISOString()
export const isoMinusDays = (d: number) => new Date(Date.now() - d * 86_400_000).toISOString()
export const unixMinusMin = (m: number) => Math.floor((Date.now() - m * 60_000) / 1000)

export const status = (overrides: Partial<ConnectionStatus> = {}): ConnectionStatus => ({
  connected: true,
  server_time: now(),
  client_id: 1,
  ...overrides,
})

export const accountSummary: AccountSummary[] = [
  { account: ACCT, tag: "NetLiquidation", value: "125430.55", currency: "USD" },
  { account: ACCT, tag: "TotalCashValue", value: "32500.00", currency: "USD" },
  { account: ACCT, tag: "BuyingPower", value: "501722.20", currency: "USD" },
  { account: ACCT, tag: "GrossPositionValue", value: "92930.55", currency: "USD" },
  { account: ACCT, tag: "MaintMarginReq", value: "23232.64", currency: "USD" },
]

export const positions: Position[] = [
  {
    account: ACCT,
    symbol: "NVDA",
    position: 50,
    average_cost: 480.25,
    market_price: 512.4,
    market_value: 25620,
    unrealized_pnl: 1607.5,
    realized_pnl: 0,
    contract_type: "STK",
    currency: "USD",
    exchange: "NASDAQ",
    local_symbol: "NVDA",
  },
  {
    account: ACCT,
    symbol: "MSFT",
    position: 30,
    average_cost: 412.1,
    market_price: 438.9,
    market_value: 13167,
    unrealized_pnl: 804,
    realized_pnl: 0,
    contract_type: "STK",
    currency: "USD",
    exchange: "NASDAQ",
    local_symbol: "MSFT",
  },
]

export const settings: AppConfig = {
  ibkr: {
    default_host: "127.0.0.1",
    default_port: 7497,
    default_client_id: 1,
    connection_timeout_ms: 5000,
    reconnect_interval_ms: 3000,
    max_reconnect_attempts: 10,
    rate_limit_per_second: 50,
  },
  logging: {
    level: "info",
    file_path: null,
    max_file_size_mb: 10,
    max_files: 5,
    console_output: true,
  },
  ui: {
    theme: "dark",
    default_refresh_interval_ms: 1000,
    show_notifications: true,
    auto_save_layout: true,
  },
  api: { alpha_vantage_api_key: "***" },
}

export const trackedTickers: TrackedTicker[] = [
  {
    symbol: "NVDA",
    source: "scanner",
    source_meta: null,
    status: "watching",
    tags: ["breakout"],
    notes: "Holding above 20EMA reclaim from yesterday.",
    added_at: isoMinusDays(2),
    last_checked_at: isoMinusHours(1),
    in_play_until: null,
    cool_down_until: null,
    archived_at: null,
  },
  {
    symbol: "TSLA",
    source: "agent",
    source_meta: { reason: "sentiment-surge" },
    status: "in_play",
    tags: ["episodic_pivot"],
    notes: null,
    added_at: isoMinusDays(1),
    last_checked_at: isoMinusHours(2),
    in_play_until: isoMinusHours(-6),
    cool_down_until: null,
    archived_at: null,
  },
]

export const alerts: Alert[] = [
  {
    id: 101,
    setup_id: 7,
    kind: "detected",
    fired_at: isoMinusHours(2),
    payload: { symbol: "NVDA", trigger_price: 510.0 },
    seen: false,
    enriched_at: isoMinusHours(1),
    research_note_id: 1,
  },
  {
    id: 100,
    setup_id: 6,
    kind: "target_hit",
    fired_at: isoMinusHours(5),
    payload: { symbol: "TSLA", target_label: "T1", target_price: 264.0 },
    seen: true,
    enriched_at: null,
    research_note_id: null,
  },
]

export const candidates: Candidate[] = [
  {
    symbol: "AMD",
    score: 0.82,
    sources: [
      { source: "scanner_top_gainers", score: 0.78, rank: 3, meta: {}, last_seen: unixMinusMin(15) },
      { source: "sentiment_surge", score: 0.86, rank: null, meta: {}, last_seen: unixMinusMin(8) },
    ],
    reason_md: "Top-3 gainer with concurrent WSB sentiment surge.",
    first_seen: unixMinusMin(120),
    last_seen: unixMinusMin(8),
    decay_at: unixMinusMin(-360),
    promoted_at: null,
  },
]

export const researchNotes: ResearchNote[] = [
  {
    id: 1,
    symbol: "NVDA",
    body_md:
      "## Thesis\n\nReclaim of 20EMA on volume; break of yesterday's high triggers continuation.\n\n## Risk\n\nClose below 505 invalidates.",
    conviction: "B",
    evidence_refs: [
      { type: "alert", id: 101 },
      { type: "bar_range", symbol: "NVDA", from: isoMinusDays(5), to: now() },
    ],
    written_by: "alert-dive-agent",
    written_at: isoMinusHours(1),
    setup_id: 7,
    alert_id: 101,
    price_at_write: 511.2,
    invalidation_price: 505,
    invalidation_kind: "close_below",
    targets: [
      { label: "T1", price: 525 },
      { label: "T2", price: 540 },
    ],
    catalyst_date: null,
  },
]

export const agentMorningPack: AgentMorningPack = {
  date: new Date().toISOString().slice(0, 10),
  written_by: "morning-ranker",
  written_at: now(),
  ranked_ideas: [
    {
      symbol: "NVDA",
      thesis_md: "Continuation off 20EMA reclaim; sector tailwind from semis.",
      conviction: "B",
      entry_zone: "510-513",
      invalidation: "Close below 505",
      evidence_refs: [{ type: "alert", id: 101 }],
    },
  ],
}

export const calibrationStats: CalibrationStats = {
  window_days: 30,
  since_unix: unixMinusMin(60 * 24 * 30),
  buckets: [
    {
      conviction: "A",
      total: 12,
      hit_target: 7,
      hit_entry: 9,
      hit_invalidation: 2,
      drifted: 1,
      no_movement: 0,
      skipped: 0,
      unparseable: 0,
      win_rate: 0.75,
      target_rate: 0.58,
    },
    {
      conviction: "B",
      total: 24,
      hit_target: 9,
      hit_entry: 14,
      hit_invalidation: 6,
      drifted: 4,
      no_movement: 0,
      skipped: 0,
      unparseable: 0,
      win_rate: 0.58,
      target_rate: 0.38,
    },
  ],
  overall: {
    conviction: "overall",
    total: 36,
    hit_target: 16,
    hit_entry: 23,
    hit_invalidation: 8,
    drifted: 5,
    no_movement: 0,
    skipped: 0,
    unparseable: 0,
    win_rate: 0.64,
    target_rate: 0.44,
  },
}

export const costAttribution: CostAttribution = {
  window_days: 30,
  since_unix: unixMinusMin(60 * 24 * 30),
  total_cost_usd: 12.83,
  total_calls: 184,
  buckets: [
    { bucket: "alert_dive", call_count: 92, cost_usd: 7.21 },
    { bucket: "morning_ranker", call_count: 30, cost_usd: 3.4 },
    { bucket: "kind:reasoning", call_count: 62, cost_usd: 2.22 },
  ],
  a_conviction_count: 12,
  usd_per_a_conviction: 1.07,
}

export const morningPack: MorningPack = {
  date: new Date().toISOString().slice(0, 10),
  ranked: [{ setup_id: 7, rank: 1, why_top_pick: "Cleanest setup; conviction B+." }],
  generated_at: now(),
}

export const mcpAudit: McpAuditEntry[] = [
  {
    id: 1,
    tool: "write_research_note",
    input: { symbol: "NVDA" },
    result_summary: "wrote note id=1",
    caller: "alert-dive-agent",
    called_at: isoMinusHours(1),
  },
]

export const quote: Quote = {
  symbol: "NVDA",
  lastPrice: 512.4,
  prevClose: 504.9,
  volume: 18_523_400,
  timestamp: Math.floor(Date.now() / 1000),
}

export const predictionHistory: PredictionWithOutcome[] = []
