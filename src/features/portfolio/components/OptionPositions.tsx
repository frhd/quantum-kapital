import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../shared/components/ui/table"
import { Target } from "lucide-react"
import { formatCurrency } from "../utils"
import type { Position } from "../../../shared/types"

interface OptionPositionsProps {
  positions: Position[]
}

export function OptionPositions({ positions }: OptionPositionsProps) {
  const optionPositions = positions.filter(pos => pos.contract_type === "OPT")

  if (optionPositions.length === 0) {
    return null
  }

  return (
    <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
      <CardHeader>
        <CardTitle className="text-white flex items-center gap-2">
          <Target className="h-5 w-5 text-orange-400" />
          Option Positions
        </CardTitle>
        <CardDescription className="text-slate-400">Current option holdings</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow className="border-slate-700 h-8">
                <TableHead className="text-slate-300 text-xs py-2">Contract</TableHead>
                <TableHead className="text-slate-300 text-right text-xs py-2">Qty</TableHead>
                <TableHead className="text-slate-300 text-right text-xs py-2">Avg Cost</TableHead>
                <TableHead className="text-slate-300 text-right text-xs py-2">Price</TableHead>
                <TableHead className="text-slate-300 text-right text-xs py-2">Value</TableHead>
                <TableHead className="text-slate-300 text-right text-xs py-2">P&L</TableHead>
                <TableHead className="text-slate-300 text-right text-xs py-2">%</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {optionPositions.map((position, index) => {
                const percentChange = ((position.market_price - position.average_cost) / position.average_cost) * 100
                return (
                  <TableRow key={`${position.local_symbol}-${index}`} className="border-slate-700 h-10">
                    <TableCell className="font-medium text-white py-2">
                      <div className="text-sm">{position.local_symbol}</div>
                      <div className="text-xs text-slate-500">{position.symbol}</div>
                    </TableCell>
                    <TableCell className="text-right text-white text-sm py-2">{position.position.toFixed(0)}</TableCell>
                    <TableCell className="text-right text-white text-sm py-2">${position.average_cost.toFixed(2)}</TableCell>
                    <TableCell className="text-right text-white text-sm py-2">${position.market_price.toFixed(2)}</TableCell>
                    <TableCell className="text-right text-white text-sm py-2">{formatCurrency(position.market_value)}</TableCell>
                    <TableCell className={`text-right text-sm font-medium py-2 ${position.unrealized_pnl >= 0 ? "text-green-400" : "text-red-400"}`}>
                      {position.unrealized_pnl >= 0 ? "+" : ""}{formatCurrency(Math.abs(position.unrealized_pnl))}
                    </TableCell>
                    <TableCell className={`text-right text-sm font-medium py-2 ${percentChange >= 0 ? "text-green-400" : "text-red-400"}`}>
                      {percentChange >= 0 ? "+" : ""}{percentChange.toFixed(1)}%
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
