import { invoke } from "@tauri-apps/api/core"

// Mirrors `services::backtester::spec::*` + `results::*`. Field
// names are snake_case to match the wire JSON serde emits — the
// frontend stays close to the Rust types so a future rename is
// caught at the type-checker.

export type StrategyTagWire = "breakout" | "episodic_pivot" | "parabolic_short" | string

export type PositionSizingMode = "conviction_scaled_r" | "fixed_r" | "no_sizing"

export type FillModelKind =
  | { kind: "naive_next_open"; slippage_bps: number }
  | {
      kind: "calibrated"
      date_from: string
      date_to_inclusive: string
      account: string | null
      fallback_bps: number
    }

export interface WalkForwardSplits {
  train_months: number
  oos_months: number
  roll_months: number
}

export interface BacktestSpec {
  date_from: string
  date_to_inclusive: string
  symbols: string[]
  detector_tags: StrategyTagWire[]
  fill_model: FillModelKind
  position_sizing: PositionSizingMode
  splits: WalkForwardSplits
  commission_usd: number
  starting_equity_usd: number
  event_blackouts_enabled: boolean
  max_hold_bars: number
  rng_seed: number
  label: string | null
}

export type ExitReason = "stop" | "target" | "time_stop"

export type Direction = "long" | "short"

export interface BacktestTrade {
  seq: number
  symbol: string
  strategy: string
  direction: Direction
  entry_time: string
  entry_price: number
  exit_time: string
  exit_price: number
  qty: number
  realized_r: number
  realized_pnl: number
  exit_reason: ExitReason
  conviction: string | null
}

export interface RiskMetrics {
  sharpe: number | null
  sortino: number | null
  calmar: number | null
  profit_factor: number
  expectancy_r: number
  max_dd: number
  max_dd_duration: number
  win_rate: number | null
  avg_win_r: number | null
  avg_loss_r: number | null
  n_days: number
  n_trades: number
  risk_free_rate_annual: number
}

export interface EquityPoint {
  date: string
  equity: number
  daily_pnl: number
}

export interface StrategyRollup {
  strategy: string
  n_trades: number
  metrics: RiskMetrics
  net_pnl: number
  gross_pnl: number
  commission: number
  stop_count: number
  target_count: number
  time_stop_count: number
}

export interface MonthRollup {
  month: string
  n_trades: number
  net_pnl: number
  realized_r_sum: number
}

export interface BacktestResult {
  run_id: string
  spec_hash: string
  headline: RiskMetrics
  equity_curve: EquityPoint[]
  by_strategy: StrategyRollup[]
  by_month: MonthRollup[]
  trades: BacktestTrade[]
  n_setups_fired: number
  n_setups_blackout_skipped: number
  n_setups_unsizable: number
}

export interface BacktestRunSummary {
  run_id: string
  label: string | null
  spec_hash: string
  n_trades: number
  started_at: string
  finished_at: string | null
  status: string
  error: string | null
}

export interface BacktestComparison {
  run_id_a: string
  run_id_b: string
  a_n_trades: number
  b_n_trades: number
  a_pf: number
  b_pf: number
  a_expectancy_r: number
  b_expectancy_r: number
  a_sharpe: number | null
  b_sharpe: number | null
  a_max_dd: number
  b_max_dd: number
}

/** Kick off a backtest run; blocks until results land. */
export async function backtestRun(spec: BacktestSpec): Promise<BacktestResult> {
  return await invoke("backtest_run", { spec })
}

/** Read a stored run — `trades` are hydrated from the rows table. */
export async function backtestGetRun(runId: string): Promise<BacktestResult | null> {
  return await invoke("backtest_get_run", { runId })
}

/** List recent runs, most-recent first. Defaults to 50 rows. */
export async function backtestListRuns(limit?: number): Promise<BacktestRunSummary[]> {
  return await invoke("backtest_list_runs", { limit: limit ?? null })
}

export async function backtestCompare(
  runIdA: string,
  runIdB: string,
): Promise<BacktestComparison | null> {
  return await invoke("backtest_compare", { runIdA, runIdB })
}

/** Build a default spec the runner UI can pre-fill. */
export function defaultBacktestSpec(): BacktestSpec {
  const today = new Date()
  const yyyy = today.getFullYear()
  const lastYear = yyyy - 1
  const isoFrom = `${lastYear}-01-01`
  const isoTo = `${yyyy}-${String(today.getMonth() + 1).padStart(2, "0")}-${String(
    today.getDate(),
  ).padStart(2, "0")}`
  return {
    date_from: isoFrom,
    date_to_inclusive: isoTo,
    symbols: [],
    detector_tags: [],
    fill_model: { kind: "naive_next_open", slippage_bps: 8 },
    position_sizing: "conviction_scaled_r",
    splits: { train_months: 12, oos_months: 3, roll_months: 1 },
    commission_usd: 1.0,
    starting_equity_usd: 100_000,
    event_blackouts_enabled: true,
    max_hold_bars: 10,
    // 32-bit-safe default. Backend zero-checks and substitutes a
    // nonzero constant so the xorshift state never lands at zero.
    rng_seed: 0x5345_4544,
    label: null,
  }
}
