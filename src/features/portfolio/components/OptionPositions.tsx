import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"
import { Target } from "lucide-react"
import { formatCurrency } from "../utils"
import type { Position } from "../../../shared/types"

interface OptionPositionsProps {
  positions: Position[]
}

export function OptionPositions({ positions }: OptionPositionsProps) {
  const optionPositions = positions.filter((pos) => pos.contract_type === "OPT")

  if (optionPositions.length === 0) {
    return null
  }

  return (
    <Card className="border-slate-700 bg-slate-800/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-white">
          <Target className="h-5 w-5 text-orange-400" />
          Option Positions
        </CardTitle>
        <CardDescription className="text-slate-400">Current option holdings</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow className="h-8 border-slate-700">
                <TableHead className="py-2 text-xs text-slate-300">Contract</TableHead>
                <TableHead className="py-2 text-right text-xs text-slate-300">Qty</TableHead>
                <TableHead className="py-2 text-right text-xs text-slate-300">Avg Cost</TableHead>
                <TableHead className="py-2 text-right text-xs text-slate-300">Price</TableHead>
                <TableHead className="py-2 text-right text-xs text-slate-300">Value</TableHead>
                <TableHead className="py-2 text-right text-xs text-slate-300">P&L</TableHead>
                <TableHead className="py-2 text-right text-xs text-slate-300">%</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {optionPositions.map((position, index) => {
                const percentChange =
                  ((position.market_price - position.average_cost) / position.average_cost) * 100
                return (
                  <TableRow
                    key={`${position.local_symbol}-${index}`}
                    className="h-10 border-slate-700"
                  >
                    <TableCell className="py-2 font-medium text-white">
                      <div className="text-sm">{position.local_symbol}</div>
                      <div className="text-xs text-slate-500">{position.symbol}</div>
                    </TableCell>
                    <TableCell className="py-2 text-right text-sm text-white">
                      {position.position.toFixed(0)}
                    </TableCell>
                    <TableCell className="py-2 text-right text-sm text-white">
                      ${position.average_cost.toFixed(2)}
                    </TableCell>
                    <TableCell className="py-2 text-right text-sm text-white">
                      ${position.market_price.toFixed(2)}
                    </TableCell>
                    <TableCell className="py-2 text-right text-sm text-white">
                      {formatCurrency(position.market_value)}
                    </TableCell>
                    <TableCell
                      className={`py-2 text-right text-sm font-medium ${position.unrealized_pnl >= 0 ? "text-green-400" : "text-red-400"}`}
                    >
                      {position.unrealized_pnl >= 0 ? "+" : ""}
                      {formatCurrency(Math.abs(position.unrealized_pnl))}
                    </TableCell>
                    <TableCell
                      className={`py-2 text-right text-sm font-medium ${percentChange >= 0 ? "text-green-400" : "text-red-400"}`}
                    >
                      {percentChange >= 0 ? "+" : ""}
                      {percentChange.toFixed(1)}%
                    </TableCell>
                  </TableRow>
                )
              })}
            </TableBody>
          </Table>
        </div>
      </CardContent>
    </Card>
  )
}
