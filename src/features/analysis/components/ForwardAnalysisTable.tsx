import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../shared/components/ui/table"
import { Badge } from "../../../shared/components/ui/badge"
import { TrendingUp, TrendingDown, Minus } from "lucide-react"
import type { FinancialProjection, CagrMetrics } from "../../../shared/types"

interface ForwardAnalysisTableProps {
  projections: FinancialProjection[]
  cagr: CagrMetrics
  scenarioType: 'bear' | 'base' | 'bull'
}

export function ForwardAnalysisTable({ projections, cagr, scenarioType }: ForwardAnalysisTableProps) {
  // Validate projections array
  if (!projections || projections.length === 0) {
    return (
      <div className="text-slate-400 text-center py-8">
        No projection data available
      </div>
    )
  }

  const formatBillions = (value: number | null | undefined) =>
    value != null ? `$${value.toFixed(2)}B` : '—'
  const formatPercent = (value: number | null | undefined) =>
    value != null ? `${value.toFixed(1)}%` : '—'
  const formatDollars = (value: number | null | undefined) =>
    value != null ? `$${value.toFixed(2)}` : '—'
  const formatNumber = (value: number | null | undefined) =>
    value != null ? value.toFixed(1) : '—'

  // Color scheme based on scenario
  const scenarioColors = {
    bear: 'from-red-500/20 to-orange-500/20',
    base: 'from-blue-500/20 to-cyan-500/20',
    bull: 'from-green-500/20 to-emerald-500/20',
  }

  const textColors = {
    bear: 'text-red-400',
    base: 'text-blue-400',
    bull: 'text-green-400',
  }

  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow className="border-slate-700/50">
            <TableHead className="text-slate-400 font-semibold">METRIC</TableHead>
            {projections.map((proj) => (
              <TableHead key={proj.year} className="text-center text-slate-300 font-semibold">
                {proj.year}
              </TableHead>
            ))}
            <TableHead className="text-center text-slate-300 font-semibold">CAGR</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {/* Revenue Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300">Revenue ($B)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`rev-${proj.year}`} className="text-center text-white">
                {formatBillions(proj.revenue)}
              </TableCell>
            ))}
            <TableCell className={`text-center font-semibold ${textColors[scenarioType]}`}>
              {formatPercent(cagr.revenue)}
            </TableCell>
          </TableRow>

          {/* Revenue Growth Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300">Rev Growth</TableCell>
            {projections.map((proj, idx) => (
              <TableCell key={`revg-${proj.year}`} className="text-center text-slate-400">
                {idx === 0 ? '—' : formatPercent(proj.revenueGrowth)}
              </TableCell>
            ))}
            <TableCell className="text-center text-slate-500">—</TableCell>
          </TableRow>

          {/* Net Income Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300">Net Income ($B)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`ni-${proj.year}`} className="text-center text-white">
                {formatBillions(proj.netIncome)}
              </TableCell>
            ))}
            <TableCell className="text-center text-slate-500">—</TableCell>
          </TableRow>

          {/* Net Income Growth Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300">Net Inc. Growth</TableCell>
            {projections.map((proj) => (
              <TableCell key={`nig-${proj.year}`} className="text-center text-slate-400">
                {formatPercent(proj.netIncomeGrowth)}
              </TableCell>
            ))}
            <TableCell className="text-center text-slate-500">—</TableCell>
          </TableRow>

          {/* Net Income Margins Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300">Net Inc. Margins</TableCell>
            {projections.map((proj) => (
              <TableCell key={`nim-${proj.year}`} className="text-center text-slate-400">
                {formatPercent(proj.netIncomeMargins)}
              </TableCell>
            ))}
            <TableCell className="text-center text-slate-500">—</TableCell>
          </TableRow>

          {/* EPS Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300">EPS ($)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`eps-${proj.year}`} className="text-center text-white">
                {formatDollars(proj.eps)}
              </TableCell>
            ))}
            <TableCell className="text-center text-slate-500">—</TableCell>
          </TableRow>

          {/* Analyst EPS Estimate Row (if available) */}
          {projections.some(p => p.analystEpsEstimate != null) && (
            <TableRow className="border-slate-700/50 hover:bg-slate-800/30 bg-blue-500/5">
              <TableCell className="font-medium text-slate-300 flex items-center gap-2">
                <span>Analyst Consensus</span>
                <Badge variant="outline" className="text-[10px] px-1 py-0 h-4 border-blue-400/30 text-blue-400">
                  Wall St.
                </Badge>
              </TableCell>
              {projections.map((proj) => {
                if (proj.analystEpsEstimate == null) {
                  return (
                    <TableCell key={`analyst-${proj.year}`} className="text-center text-slate-500">
                      —
                    </TableCell>
                  )
                }

                // Compare projected EPS vs analyst estimate
                const diff = proj.eps - proj.analystEpsEstimate
                const diffPercent = (diff / Math.abs(proj.analystEpsEstimate)) * 100

                // Determine sentiment
                let icon = null
                let badgeVariant: "default" | "secondary" | "destructive" | "outline" = "secondary"
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
                      <span className="text-blue-300 font-medium">
                        {formatDollars(proj.analystEpsEstimate)}
                      </span>
                      <Badge variant={badgeVariant} className="text-[10px] px-1 py-0 h-4 flex items-center gap-0.5">
                        {icon}
                        {badgeText}
                      </Badge>
                    </div>
                  </TableCell>
                )
              })}
              <TableCell className="text-center text-slate-500">—</TableCell>
            </TableRow>
          )}

          {/* PE Range Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300">PE Range (Low/High)</TableCell>
            {projections.map((proj) => (
              <TableCell key={`pe-${proj.year}`} className="text-center text-slate-400">
                {formatNumber(proj.peLowEst)}/{formatNumber(proj.peHighEst)}
              </TableCell>
            ))}
            <TableCell className="text-center text-slate-500">—</TableCell>
          </TableRow>

          {/* Share Price Range - Low */}
          <TableRow className={`border-slate-700/50 bg-gradient-to-r ${scenarioColors[scenarioType]}`}>
            <TableCell className="font-medium text-slate-300">Share Price Low</TableCell>
            {projections.map((proj) => (
              <TableCell key={`spl-${proj.year}`} className={`text-center font-semibold ${textColors[scenarioType]}`}>
                {formatDollars(proj.sharePriceLow)}
              </TableCell>
            ))}
            <TableCell className="text-center text-slate-500">—</TableCell>
          </TableRow>

          {/* Share Price Range - High */}
          <TableRow className={`border-slate-700/50 bg-gradient-to-r ${scenarioColors[scenarioType]}`}>
            <TableCell className="font-medium text-slate-300">Share Price High</TableCell>
            {projections.map((proj) => (
              <TableCell key={`sph-${proj.year}`} className={`text-center font-semibold ${textColors[scenarioType]}`}>
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
