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
import { BarChart3 } from "lucide-react"
import { formatCurrency } from "../utils"
import type { Position } from "../../../shared/types"

interface StockPositionsProps {
  positions: Position[]
}

export function StockPositions({ positions }: StockPositionsProps) {
  const stockPositions = positions.filter((pos) => pos.contract_type === "STK")

  if (stockPositions.length === 0) {
    return null
  }

  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="text-foreground flex items-center gap-2">
          <BarChart3 className="h-5 w-5 text-blue-400" />
          Stock Positions
        </CardTitle>
        <CardDescription className="text-muted-foreground">Current stock holdings</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="overflow-x-auto">
          <Table>
            <TableHeader>
              <TableRow className="border-border h-8">
                <TableHead className="text-foreground py-2 text-xs">Symbol</TableHead>
                <TableHead className="text-foreground py-2 text-right text-xs">Qty</TableHead>
                <TableHead className="text-foreground py-2 text-right text-xs">Avg Cost</TableHead>
                <TableHead className="text-foreground py-2 text-right text-xs">Price</TableHead>
                <TableHead className="text-foreground py-2 text-right text-xs">Value</TableHead>
                <TableHead className="text-foreground py-2 text-right text-xs">P&L</TableHead>
                <TableHead className="text-foreground py-2 text-right text-xs">%</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {stockPositions.map((position, index) => {
                const percentChange =
                  ((position.market_price - position.average_cost) / position.average_cost) * 100
                return (
                  <TableRow key={`${position.symbol}-${index}`} className="border-border h-10">
                    <TableCell className="text-foreground py-2 font-medium">
                      <div className="text-sm">{position.symbol}</div>
                      <div className="text-muted-foreground text-xs">{position.exchange}</div>
                    </TableCell>
                    <TableCell className="text-foreground py-2 text-right text-sm">
                      {position.position.toFixed(0)}
                    </TableCell>
                    <TableCell className="text-foreground py-2 text-right text-sm">
                      ${position.average_cost.toFixed(2)}
                    </TableCell>
                    <TableCell className="text-foreground py-2 text-right text-sm">
                      ${position.market_price.toFixed(2)}
                    </TableCell>
                    <TableCell className="text-foreground py-2 text-right text-sm">
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
