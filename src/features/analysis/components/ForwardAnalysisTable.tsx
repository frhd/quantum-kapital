import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"
import { Badge } from "../../../shared/components/ui/badge"
import { TrendingUp, TrendingDown, Minus } from "lucide-react"
import type { FinancialProjection, CagrMetrics } from "../../../shared/types"

interface ForwardAnalysisTableProps {
  projections: FinancialProjection[]
  cagr: CagrMetrics
  scenarioType: "bear" | "base" | "bull"
}

export function ForwardAnalysisTable({
  projections,
  cagr,
  scenarioType,
}: ForwardAnalysisTableProps) {
  // Validate projections array
  if (!projections || projections.length === 0) {
    return (
      <div className="text-muted-foreground py-8 text-center">No projection data available</div>
    )
  }

  const formatBillions = (value: number | null | undefined) =>
    value != null ? `$${value.toFixed(2)}B` : "—"
  const formatPercent = (value: number | null | undefined) =>
    value != null ? `${value.toFixed(1)}%` : "—"
  const formatDollars = (value: number | null | undefined) =>
    value != null ? `$${value.toFixed(2)}` : "—"
  const formatNumber = (value: number | null | undefined) =>
    value != null ? value.toFixed(1) : "—"

  // Flat tint per scenario (emissionwise-style, no gradients)
  const scenarioColors = {
    bear: "bg-destructive/10",
    base: "bg-primary/10",
    bull: "bg-emerald-500/10",
  }

  const textColors = {
    bear: "text-red-400",
    base: "text-blue-400",
    bull: "text-green-400",
  }

  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow className="border-border/50">
            <TableHead className="text-muted-foreground font-semibold">METRIC</TableHead>
            {projections.map((proj) => (
              <TableHead key={proj.year} className="text-foreground text-center font-semibold">
                {proj.year}
              </TableHead>
            ))}
            <TableHead className="text-foreground text-center font-semibold">CAGR</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {/* Revenue Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="text-foreground font-medium">Revenue ($B)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`rev-${proj.year}`} className="text-foreground text-center">
                {formatBillions(proj.revenue)}
              </TableCell>
            ))}
            <TableCell className={`text-center font-semibold ${textColors[scenarioType]}`}>
              {formatPercent(cagr.revenue)}
            </TableCell>
          </TableRow>

          {/* Revenue Growth Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="text-foreground font-medium">Rev Growth</TableCell>
            {projections.map((proj, idx) => (
              <TableCell key={`revg-${proj.year}`} className="text-muted-foreground text-center">
                {idx === 0 ? "—" : formatPercent(proj.revenueGrowth)}
              </TableCell>
            ))}
            <TableCell className="text-muted-foreground text-center">—</TableCell>
          </TableRow>

          {/* Net Income Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="text-foreground font-medium">Net Income ($B)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`ni-${proj.year}`} className="text-foreground text-center">
                {formatBillions(proj.netIncome)}
              </TableCell>
            ))}
            <TableCell className="text-muted-foreground text-center">—</TableCell>
          </TableRow>

          {/* Net Income Growth Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="text-foreground font-medium">Net Inc. Growth</TableCell>
            {projections.map((proj) => (
              <TableCell key={`nig-${proj.year}`} className="text-muted-foreground text-center">
                {formatPercent(proj.netIncomeGrowth)}
              </TableCell>
            ))}
            <TableCell className="text-muted-foreground text-center">—</TableCell>
          </TableRow>

          {/* Net Income Margins Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="text-foreground font-medium">Net Inc. Margins</TableCell>
            {projections.map((proj) => (
              <TableCell key={`nim-${proj.year}`} className="text-muted-foreground text-center">
                {formatPercent(proj.netIncomeMargins)}
              </TableCell>
            ))}
            <TableCell className="text-muted-foreground text-center">—</TableCell>
          </TableRow>

          {/* EPS Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="text-foreground font-medium">EPS ($)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`eps-${proj.year}`} className="text-foreground text-center">
                {formatDollars(proj.eps)}
              </TableCell>
            ))}
            <TableCell className="text-muted-foreground text-center">—</TableCell>
          </TableRow>

          {/* Analyst EPS Estimate Row (if available) */}
          {projections.some((p) => p.analystEpsEstimate != null) && (
            <TableRow className="border-border/50 hover:bg-card/30 bg-blue-500/5">
              <TableCell className="text-foreground flex items-center gap-2 font-medium">
                <span>Analyst Consensus</span>
                <Badge
                  variant="outline"
                  className="h-4 border-blue-400/30 px-1 py-0 text-[10px] text-blue-400"
                >
                  Wall St.
                </Badge>
              </TableCell>
              {projections.map((proj) => {
                if (proj.analystEpsEstimate == null) {
                  return (
                    <TableCell
                      key={`analyst-${proj.year}`}
                      className="text-muted-foreground text-center"
                    >
                      —
                    </TableCell>
                  )
                }

                // Compare projected EPS vs analyst estimate
                const diff = proj.eps - proj.analystEpsEstimate
                const diffPercent = (diff / Math.abs(proj.analystEpsEstimate)) * 100

                // Determine sentiment
                let icon = null
                let badgeVariant: "default" | "secondary" | "destructive" | "outline-solid" =
                  "secondary"
                let badgeText = ""

                if (Math.abs(diffPercent) < 5) {
                  icon = <Minus className="h-3 w-3" />
                  badgeVariant = "secondary"
                  badgeText = "Aligned"
                } else if (diff > 0) {
                  icon = <TrendingUp className="h-3 w-3" />
                  badgeVariant = "default"
                  badgeText = `+${diffPercent.toFixed(0)}%`
                } else {
                  icon = <TrendingDown className="h-3 w-3" />
                  badgeVariant = "destructive"
                  badgeText = `${diffPercent.toFixed(0)}%`
                }

                return (
                  <TableCell key={`analyst-${proj.year}`} className="text-center">
                    <div className="flex flex-col items-center gap-1">
                      <span className="font-medium text-blue-300">
                        {formatDollars(proj.analystEpsEstimate)}
                      </span>
                      <Badge
                        variant={badgeVariant}
                        className="flex h-4 items-center gap-0.5 px-1 py-0 text-[10px]"
                      >
                        {icon}
                        {badgeText}
                      </Badge>
                    </div>
                  </TableCell>
                )
              })}
              <TableCell className="text-muted-foreground text-center">—</TableCell>
            </TableRow>
          )}

          {/* PE Range Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="text-foreground font-medium">PE Range (Low/High)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`pe-${proj.year}`} className="text-muted-foreground text-center">
                {formatNumber(proj.peLowEst)}/{formatNumber(proj.peHighEst)}
              </TableCell>
            ))}
            <TableCell className="text-muted-foreground text-center">—</TableCell>
          </TableRow>

          {/* Share Price Range - Low */}
          <TableRow className={`border-border/50 ${scenarioColors[scenarioType]}`}>
            <TableCell className="text-foreground font-medium">Share Price Low</TableCell>
            {projections.map((proj) => (
              <TableCell
                key={`spl-${proj.year}`}
                className={`text-center font-semibold ${textColors[scenarioType]}`}
              >
                {formatDollars(proj.sharePriceLow)}
              </TableCell>
            ))}
            <TableCell className="text-muted-foreground text-center">—</TableCell>
          </TableRow>

          {/* Share Price Range - High */}
          <TableRow className={`border-border/50 ${scenarioColors[scenarioType]}`}>
            <TableCell className="text-foreground font-medium">Share Price High</TableCell>
            {projections.map((proj) => (
              <TableCell
                key={`sph-${proj.year}`}
                className={`text-center font-semibold ${textColors[scenarioType]}`}
              >
                {formatDollars(proj.sharePriceHigh)}
              </TableCell>
            ))}
            <TableCell className={`text-center font-semibold ${textColors[scenarioType]}`}>
              {formatPercent(cagr.sharePrice)}
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>
  )
}
