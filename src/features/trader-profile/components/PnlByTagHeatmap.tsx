import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"
import type { PnlByTag } from "../types"

function fmtUsd(value: number): string {
  const sign = value < 0 ? "-" : ""
  const abs = Math.abs(value)
  return `${sign}$${abs.toFixed(2)}`
}

function pnlClass(value: number): string {
  if (value > 0) return "text-green-400"
  if (value < 0) return "text-red-400"
  return "text-muted-foreground"
}

export function PnlByTagHeatmap({ rows }: { rows: PnlByTag[] }) {
  if (rows.length === 0) {
    return (
      <p className="text-muted-foreground py-4 text-center text-xs">
        No P&L attribution available for this window.
      </p>
    )
  }
  return (
    <div className="overflow-x-auto" data-testid="pnl-by-tag-heatmap">
      <Table>
        <TableHeader>
          <TableRow>
            <TableHead>Tag</TableHead>
            <TableHead className="text-right">Days</TableHead>
            <TableHead className="text-right">Total P&L</TableHead>
            <TableHead className="text-right">Avg / day</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {rows.map((row) => (
            <TableRow key={row.tag}>
              <TableCell className="font-mono">{row.tag}</TableCell>
              <TableCell className="text-right font-mono tabular-nums">{row.n_days}</TableCell>
              <TableCell
                className={`text-right font-mono tabular-nums ${pnlClass(row.net_pnl_total)}`}
              >
                {fmtUsd(row.net_pnl_total)}
              </TableCell>
              <TableCell
                className={`text-right font-mono tabular-nums ${pnlClass(row.net_pnl_per_day_avg)}`}
              >
                {fmtUsd(row.net_pnl_per_day_avg)}
              </TableCell>
            </TableRow>
          ))}
        </TableBody>
      </Table>
    </div>
  )
}
