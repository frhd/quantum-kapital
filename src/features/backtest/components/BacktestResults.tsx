import type { BacktestResult } from "../../../shared/api/backtest"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"

interface BacktestResultsProps {
  result: BacktestResult
}

const fmtNum = (n: number, frac = 2) => (Number.isFinite(n) ? n.toFixed(frac) : "—")

const fmtPct = (n: number) => (Number.isFinite(n) ? `${(n * 100).toFixed(1)}%` : "—")

const fmtUsd = (n: number) =>
  Number.isFinite(n)
    ? n.toLocaleString("en-US", {
        style: "currency",
        currency: "USD",
        minimumFractionDigits: 0,
        maximumFractionDigits: 0,
      })
    : "—"

const fmtOpt = (n: number | null, frac = 2): string =>
  n == null || !Number.isFinite(n) ? "—" : n.toFixed(frac)

export function BacktestResults({ result }: BacktestResultsProps) {
  const h = result.headline
  return (
    <div className="space-y-4">
      <Card className="border-border/50 bg-card/30">
        <CardHeader>
          <CardTitle className="text-base">Headline</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-x-4 gap-y-2 text-sm md:grid-cols-4">
            <Metric label="Trades" value={String(h.n_trades)} />
            <Metric
              label="Profit factor"
              value={Number.isFinite(h.profit_factor) ? fmtNum(h.profit_factor, 2) : "∞"}
            />
            <Metric label="Expectancy R" value={fmtNum(h.expectancy_r, 3)} />
            <Metric label="Max DD" value={fmtPct(h.max_dd)} />
            <Metric label="Sharpe" value={fmtOpt(h.sharpe)} />
            <Metric label="Sortino" value={fmtOpt(h.sortino)} />
            <Metric label="Calmar" value={fmtOpt(h.calmar)} />
            <Metric label="Win rate" value={h.win_rate == null ? "—" : fmtPct(h.win_rate)} />
          </div>
          <p className="text-muted-foreground mt-3 text-xs">
            Run <code className="font-mono">{result.run_id}</code> · spec{" "}
            <code className="font-mono">{result.spec_hash}</code>
            {" · "}
            fired {result.n_setups_fired} · gated {result.n_setups_blackout_skipped} · unsizable{" "}
            {result.n_setups_unsizable}
          </p>
        </CardContent>
      </Card>

      {result.by_strategy.length > 0 && (
        <Card className="border-border/50 bg-card/30">
          <CardHeader>
            <CardTitle className="text-base">Per-strategy breakdown</CardTitle>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Strategy</TableHead>
                  <TableHead className="text-right">N</TableHead>
                  <TableHead className="text-right">PF</TableHead>
                  <TableHead className="text-right">Expectancy R</TableHead>
                  <TableHead className="text-right">Net PnL</TableHead>
                  <TableHead className="text-right">Stop</TableHead>
                  <TableHead className="text-right">Target</TableHead>
                  <TableHead className="text-right">Time</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {result.by_strategy.map((s) => (
                  <TableRow key={s.strategy}>
                    <TableCell className="font-medium">{s.strategy}</TableCell>
                    <TableCell className="text-right">{s.n_trades}</TableCell>
                    <TableCell className="text-right">
                      {Number.isFinite(s.metrics.profit_factor)
                        ? fmtNum(s.metrics.profit_factor, 2)
                        : "∞"}
                    </TableCell>
                    <TableCell className="text-right">
                      {fmtNum(s.metrics.expectancy_r, 3)}
                    </TableCell>
                    <TableCell className="text-right">{fmtUsd(s.net_pnl)}</TableCell>
                    <TableCell className="text-right">{s.stop_count}</TableCell>
                    <TableCell className="text-right">{s.target_count}</TableCell>
                    <TableCell className="text-right">{s.time_stop_count}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}

      {result.by_month.length > 0 && (
        <Card className="border-border/50 bg-card/30">
          <CardHeader>
            <CardTitle className="text-base">Per-month</CardTitle>
          </CardHeader>
          <CardContent>
            <Table>
              <TableHeader>
                <TableRow>
                  <TableHead>Month</TableHead>
                  <TableHead className="text-right">Trades</TableHead>
                  <TableHead className="text-right">Net PnL</TableHead>
                  <TableHead className="text-right">Σ R</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {result.by_month.map((m) => (
                  <TableRow key={m.month}>
                    <TableCell>{m.month}</TableCell>
                    <TableCell className="text-right">{m.n_trades}</TableCell>
                    <TableCell className="text-right">{fmtUsd(m.net_pnl)}</TableCell>
                    <TableCell className="text-right">{fmtNum(m.realized_r_sum, 2)}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </CardContent>
        </Card>
      )}
    </div>
  )
}

function Metric({ label, value }: { label: string; value: string }) {
  return (
    <div>
      <p className="text-muted-foreground text-xs">{label}</p>
      <p className="text-foreground text-lg font-semibold">{value}</p>
    </div>
  )
}
