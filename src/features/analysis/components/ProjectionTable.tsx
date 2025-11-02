import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../shared/components/ui/table"
import { Badge } from "../../../shared/components/ui/badge"
import { TrendingUp, TrendingDown, Minus } from "lucide-react"
import type { ProjectionResults } from "../../../shared/types"

interface ProjectionTableProps {
  results: ProjectionResults
}

export function ProjectionTable({ results }: ProjectionTableProps) {
  const formatBillions = (value: number) => `$${value.toFixed(2)}B`
  const formatPercent = (value: number) => `${value.toFixed(1)}%`
  const formatDollars = (value: number) => `$${value.toFixed(2)}`

  const { baseline, projections, cagr } = results

  return (
    <div className="overflow-x-auto">
      <Table>
        <TableHeader>
          <TableRow className="border-slate-700/50">
            <TableHead className="text-slate-400 font-semibold sticky left-0 bg-slate-900 z-10">
              METRIC
            </TableHead>
            {/* Baseline column */}
            <TableHead className="text-center text-slate-300 font-semibold bg-slate-800/50">
              {baseline.year}
              <br />
              <span className="text-xs text-slate-500 font-normal">(Baseline)</span>
            </TableHead>
            {/* Projection year columns */}
            {projections.map((yearProj) => (
              <TableHead
                key={yearProj.year}
                colSpan={3}
                className="text-center text-slate-300 font-semibold border-l-2 border-slate-700"
              >
                {yearProj.year}
              </TableHead>
            ))}
            {/* CAGR column */}
            <TableHead className="text-center text-slate-300 font-semibold border-l-2 border-slate-700">
              CAGR
            </TableHead>
          </TableRow>
          {/* Sub-header for scenarios */}
          <TableRow className="border-slate-700/50">
            <TableHead className="sticky left-0 bg-slate-900 z-10"></TableHead>
            <TableHead className="text-center text-xs text-slate-500 bg-slate-800/50">Actual</TableHead>
            {projections.map((yearProj) => (
              <>
                <TableHead
                  key={`${yearProj.year}-bear`}
                  className="text-center text-xs text-red-400 border-l-2 border-slate-700"
                >
                  Bear
                </TableHead>
                <TableHead key={`${yearProj.year}-base`} className="text-center text-xs text-blue-400">
                  Base
                </TableHead>
                <TableHead key={`${yearProj.year}-bull`} className="text-center text-xs text-green-400">
                  Bull
                </TableHead>
              </>
            ))}
            <TableHead className="text-center text-xs text-slate-500 border-l-2 border-slate-700">%</TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {/* Revenue Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300 sticky left-0 bg-slate-900 z-10">
              Revenue ($B)
            </TableCell>
            <TableCell className="text-center text-white bg-slate-800/30">
              {formatBillions(baseline.revenue)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell key={`${yearProj.year}-bear-rev`} className="text-center text-white border-l-2 border-slate-700/30">
                  {formatBillions(yearProj.bear.revenue)}
                </TableCell>
                <TableCell key={`${yearProj.year}-base-rev`} className="text-center text-white">
                  {formatBillions(yearProj.base.revenue)}
                </TableCell>
                <TableCell key={`${yearProj.year}-bull-rev`} className="text-center text-white">
                  {formatBillions(yearProj.bull.revenue)}
                </TableCell>
              </>
            ))}
            <TableCell className="text-center text-slate-500 border-l-2 border-slate-700/30">
              <div className="flex flex-col gap-0.5">
                <span className="text-xs text-red-400">{formatPercent(cagr.bear.revenue)}</span>
                <span className="text-xs text-blue-400">{formatPercent(cagr.base.revenue)}</span>
                <span className="text-xs text-green-400">{formatPercent(cagr.bull.revenue)}</span>
              </div>
            </TableCell>
          </TableRow>

          {/* Net Income Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300 sticky left-0 bg-slate-900 z-10">
              Net Income ($B)
            </TableCell>
            <TableCell className="text-center text-white bg-slate-800/30">
              {formatBillions(baseline.netIncome)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell key={`${yearProj.year}-bear-ni`} className="text-center text-white border-l-2 border-slate-700/30">
                  {formatBillions(yearProj.bear.netIncome)}
                </TableCell>
                <TableCell key={`${yearProj.year}-base-ni`} className="text-center text-white">
                  {formatBillions(yearProj.base.netIncome)}
                </TableCell>
                <TableCell key={`${yearProj.year}-bull-ni`} className="text-center text-white">
                  {formatBillions(yearProj.bull.netIncome)}
                </TableCell>
              </>
            ))}
            <TableCell className="text-center text-slate-500 border-l-2 border-slate-700/30">—</TableCell>
          </TableRow>

          {/* Net Income Margins Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300 sticky left-0 bg-slate-900 z-10">
              Net Margins (%)
            </TableCell>
            <TableCell className="text-center text-slate-400 bg-slate-800/30">
              {formatPercent(baseline.netIncomeMargins)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell key={`${yearProj.year}-bear-nim`} className="text-center text-slate-400 border-l-2 border-slate-700/30">
                  {formatPercent(yearProj.bear.netIncomeMargins)}
                </TableCell>
                <TableCell key={`${yearProj.year}-base-nim`} className="text-center text-slate-400">
                  {formatPercent(yearProj.base.netIncomeMargins)}
                </TableCell>
                <TableCell key={`${yearProj.year}-bull-nim`} className="text-center text-slate-400">
                  {formatPercent(yearProj.bull.netIncomeMargins)}
                </TableCell>
              </>
            ))}
            <TableCell className="text-center text-slate-500 border-l-2 border-slate-700/30">—</TableCell>
          </TableRow>

          {/* EPS Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="font-medium text-slate-300 sticky left-0 bg-slate-900 z-10">
              EPS ($)
            </TableCell>
            <TableCell className="text-center text-white bg-slate-800/30">
              {formatDollars(baseline.eps)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell key={`${yearProj.year}-bear-eps`} className="text-center text-white border-l-2 border-slate-700/30">
                  {formatDollars(yearProj.bear.eps)}
                </TableCell>
                <TableCell key={`${yearProj.year}-base-eps`} className="text-center text-white">
                  {formatDollars(yearProj.base.eps)}
                </TableCell>
                <TableCell key={`${yearProj.year}-bull-eps`} className="text-center text-white">
                  {formatDollars(yearProj.bull.eps)}
                </TableCell>
              </>
            ))}
            <TableCell className="text-center text-slate-500 border-l-2 border-slate-700/30">—</TableCell>
          </TableRow>

          {/* Share Price Range Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30 bg-gradient-to-r from-blue-500/10">
            <TableCell className="font-medium text-slate-300 sticky left-0 bg-slate-900 z-10">
              Share Price Range
            </TableCell>
            <TableCell className="text-center text-white font-semibold bg-slate-800/30">
              {formatDollars(baseline.sharePriceLow)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell key={`${yearProj.year}-bear-price`} className="text-center text-red-400 font-semibold border-l-2 border-slate-700/30">
                  {formatDollars(yearProj.bear.sharePriceLow)}-{formatDollars(yearProj.bear.sharePriceHigh)}
                </TableCell>
                <TableCell key={`${yearProj.year}-base-price`} className="text-center text-blue-400 font-semibold">
                  {formatDollars(yearProj.base.sharePriceLow)}-{formatDollars(yearProj.base.sharePriceHigh)}
                </TableCell>
                <TableCell key={`${yearProj.year}-bull-price`} className="text-center text-green-400 font-semibold">
                  {formatDollars(yearProj.bull.sharePriceLow)}-{formatDollars(yearProj.bull.sharePriceHigh)}
                </TableCell>
              </>
            ))}
            <TableCell className="text-center text-slate-500 border-l-2 border-slate-700/30">
              <div className="flex flex-col gap-0.5">
                <span className="text-xs text-red-400">{formatPercent(cagr.bear.sharePrice)}</span>
                <span className="text-xs text-blue-400">{formatPercent(cagr.base.sharePrice)}</span>
                <span className="text-xs text-green-400">{formatPercent(cagr.bull.sharePrice)}</span>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>
  )
}
