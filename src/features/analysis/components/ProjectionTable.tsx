import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"
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
            <TableHead className="sticky left-0 z-10 bg-slate-900 font-semibold text-slate-400">
              METRIC
            </TableHead>
            {/* Baseline column */}
            <TableHead className="bg-slate-800/50 text-center font-semibold text-slate-300">
              {baseline.year}
              <br />
              <span className="text-xs font-normal text-slate-500">(Baseline)</span>
            </TableHead>
            {/* Projection year columns */}
            {projections.map((yearProj) => (
              <TableHead
                key={yearProj.year}
                colSpan={3}
                className="border-l-2 border-slate-700 text-center font-semibold text-slate-300"
              >
                {yearProj.year}
              </TableHead>
            ))}
            {/* CAGR column */}
            <TableHead className="border-l-2 border-slate-700 text-center font-semibold text-slate-300">
              CAGR
            </TableHead>
          </TableRow>
          {/* Sub-header for scenarios */}
          <TableRow className="border-slate-700/50">
            <TableHead className="sticky left-0 z-10 bg-slate-900"></TableHead>
            <TableHead className="bg-slate-800/50 text-center text-xs text-slate-500">
              Actual
            </TableHead>
            {projections.map((yearProj) => (
              <>
                <TableHead
                  key={`${yearProj.year}-bear`}
                  className="border-l-2 border-slate-700 text-center text-xs text-red-400"
                >
                  Bear
                </TableHead>
                <TableHead
                  key={`${yearProj.year}-base`}
                  className="text-center text-xs text-blue-400"
                >
                  Base
                </TableHead>
                <TableHead
                  key={`${yearProj.year}-bull`}
                  className="text-center text-xs text-green-400"
                >
                  Bull
                </TableHead>
              </>
            ))}
            <TableHead className="border-l-2 border-slate-700 text-center text-xs text-slate-500">
              %
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {/* Revenue Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="sticky left-0 z-10 bg-slate-900 font-medium text-slate-300">
              Revenue ($B)
            </TableCell>
            <TableCell className="bg-slate-800/30 text-center text-white">
              {formatBillions(baseline.revenue)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-rev`}
                  className="border-l-2 border-slate-700/30 text-center text-white"
                >
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
            <TableCell className="border-l-2 border-slate-700/30 text-center text-slate-500">
              <div className="flex flex-col gap-0.5">
                <span className="text-xs text-red-400">{formatPercent(cagr.bear.revenue)}</span>
                <span className="text-xs text-blue-400">{formatPercent(cagr.base.revenue)}</span>
                <span className="text-xs text-green-400">{formatPercent(cagr.bull.revenue)}</span>
              </div>
            </TableCell>
          </TableRow>

          {/* Net Income Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="sticky left-0 z-10 bg-slate-900 font-medium text-slate-300">
              Net Income ($B)
            </TableCell>
            <TableCell className="bg-slate-800/30 text-center text-white">
              {formatBillions(baseline.netIncome)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-ni`}
                  className="border-l-2 border-slate-700/30 text-center text-white"
                >
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
            <TableCell className="border-l-2 border-slate-700/30 text-center text-slate-500">
              —
            </TableCell>
          </TableRow>

          {/* Net Income Margins Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="sticky left-0 z-10 bg-slate-900 font-medium text-slate-300">
              Net Margins (%)
            </TableCell>
            <TableCell className="bg-slate-800/30 text-center text-slate-400">
              {formatPercent(baseline.netIncomeMargins)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-nim`}
                  className="border-l-2 border-slate-700/30 text-center text-slate-400"
                >
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
            <TableCell className="border-l-2 border-slate-700/30 text-center text-slate-500">
              —
            </TableCell>
          </TableRow>

          {/* EPS Row */}
          <TableRow className="border-slate-700/50 hover:bg-slate-800/30">
            <TableCell className="sticky left-0 z-10 bg-slate-900 font-medium text-slate-300">
              EPS ($)
            </TableCell>
            <TableCell className="bg-slate-800/30 text-center text-white">
              {formatDollars(baseline.eps)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-eps`}
                  className="border-l-2 border-slate-700/30 text-center text-white"
                >
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
            <TableCell className="border-l-2 border-slate-700/30 text-center text-slate-500">
              —
            </TableCell>
          </TableRow>

          {/* Share Price Range Row */}
          <TableRow className="border-slate-700/50 bg-linear-to-r from-blue-500/10 hover:bg-slate-800/30">
            <TableCell className="sticky left-0 z-10 bg-slate-900 font-medium text-slate-300">
              Share Price Range
            </TableCell>
            <TableCell className="bg-slate-800/30 text-center font-semibold text-white">
              {formatDollars(baseline.sharePriceLow)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-price`}
                  className="border-l-2 border-slate-700/30 text-center font-semibold text-red-400"
                >
                  {formatDollars(yearProj.bear.sharePriceLow)}-
                  {formatDollars(yearProj.bear.sharePriceHigh)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-base-price`}
                  className="text-center font-semibold text-blue-400"
                >
                  {formatDollars(yearProj.base.sharePriceLow)}-
                  {formatDollars(yearProj.base.sharePriceHigh)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-bull-price`}
                  className="text-center font-semibold text-green-400"
                >
                  {formatDollars(yearProj.bull.sharePriceLow)}-
                  {formatDollars(yearProj.bull.sharePriceHigh)}
                </TableCell>
              </>
            ))}
            <TableCell className="border-l-2 border-slate-700/30 text-center text-slate-500">
              <div className="flex flex-col gap-0.5">
                <span className="text-xs text-red-400">{formatPercent(cagr.bear.sharePrice)}</span>
                <span className="text-xs text-blue-400">{formatPercent(cagr.base.sharePrice)}</span>
                <span className="text-xs text-green-400">
                  {formatPercent(cagr.bull.sharePrice)}
                </span>
              </div>
            </TableCell>
          </TableRow>
        </TableBody>
      </Table>
    </div>
  )
}
