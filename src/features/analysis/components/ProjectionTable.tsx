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
          <TableRow className="border-border/50">
            <TableHead className="bg-background text-muted-foreground sticky left-0 z-10 font-semibold">
              METRIC
            </TableHead>
            {/* Baseline column */}
            <TableHead className="bg-card/50 text-foreground text-center font-semibold">
              {baseline.year}
              <br />
              <span className="text-muted-foreground text-xs font-normal">(Baseline)</span>
            </TableHead>
            {/* Projection year columns */}
            {projections.map((yearProj) => (
              <TableHead
                key={yearProj.year}
                colSpan={3}
                className="border-border text-foreground border-l-2 text-center font-semibold"
              >
                {yearProj.year}
              </TableHead>
            ))}
            {/* CAGR column */}
            <TableHead className="border-border text-foreground border-l-2 text-center font-semibold">
              CAGR
            </TableHead>
          </TableRow>
          {/* Sub-header for scenarios */}
          <TableRow className="border-border/50">
            <TableHead className="bg-background sticky left-0 z-10"></TableHead>
            <TableHead className="bg-card/50 text-muted-foreground text-center text-xs">
              Actual
            </TableHead>
            {projections.map((yearProj) => (
              <>
                <TableHead
                  key={`${yearProj.year}-bear`}
                  className="border-border border-l-2 text-center text-xs text-red-400"
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
            <TableHead className="border-border text-muted-foreground border-l-2 text-center text-xs">
              %
            </TableHead>
          </TableRow>
        </TableHeader>
        <TableBody>
          {/* Revenue Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="bg-background text-foreground sticky left-0 z-10 font-medium">
              Revenue ($B)
            </TableCell>
            <TableCell className="bg-card/30 text-foreground text-center">
              {formatBillions(baseline.revenue)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-rev`}
                  className="border-border/30 text-foreground border-l-2 text-center"
                >
                  {formatBillions(yearProj.bear.revenue)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-base-rev`}
                  className="text-foreground text-center"
                >
                  {formatBillions(yearProj.base.revenue)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-bull-rev`}
                  className="text-foreground text-center"
                >
                  {formatBillions(yearProj.bull.revenue)}
                </TableCell>
              </>
            ))}
            <TableCell className="border-border/30 text-muted-foreground border-l-2 text-center">
              <div className="flex flex-col gap-0.5">
                <span className="text-xs text-red-400">{formatPercent(cagr.bear.revenue)}</span>
                <span className="text-xs text-blue-400">{formatPercent(cagr.base.revenue)}</span>
                <span className="text-xs text-green-400">{formatPercent(cagr.bull.revenue)}</span>
              </div>
            </TableCell>
          </TableRow>

          {/* Net Income Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="bg-background text-foreground sticky left-0 z-10 font-medium">
              Net Income ($B)
            </TableCell>
            <TableCell className="bg-card/30 text-foreground text-center">
              {formatBillions(baseline.netIncome)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-ni`}
                  className="border-border/30 text-foreground border-l-2 text-center"
                >
                  {formatBillions(yearProj.bear.netIncome)}
                </TableCell>
                <TableCell key={`${yearProj.year}-base-ni`} className="text-foreground text-center">
                  {formatBillions(yearProj.base.netIncome)}
                </TableCell>
                <TableCell key={`${yearProj.year}-bull-ni`} className="text-foreground text-center">
                  {formatBillions(yearProj.bull.netIncome)}
                </TableCell>
              </>
            ))}
            <TableCell className="border-border/30 text-muted-foreground border-l-2 text-center">
              —
            </TableCell>
          </TableRow>

          {/* Net Income Margins Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="bg-background text-foreground sticky left-0 z-10 font-medium">
              Net Margins (%)
            </TableCell>
            <TableCell className="bg-card/30 text-muted-foreground text-center">
              {formatPercent(baseline.netIncomeMargins)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-nim`}
                  className="border-border/30 text-muted-foreground border-l-2 text-center"
                >
                  {formatPercent(yearProj.bear.netIncomeMargins)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-base-nim`}
                  className="text-muted-foreground text-center"
                >
                  {formatPercent(yearProj.base.netIncomeMargins)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-bull-nim`}
                  className="text-muted-foreground text-center"
                >
                  {formatPercent(yearProj.bull.netIncomeMargins)}
                </TableCell>
              </>
            ))}
            <TableCell className="border-border/30 text-muted-foreground border-l-2 text-center">
              —
            </TableCell>
          </TableRow>

          {/* EPS Row */}
          <TableRow className="border-border/50 hover:bg-card/30">
            <TableCell className="bg-background text-foreground sticky left-0 z-10 font-medium">
              EPS ($)
            </TableCell>
            <TableCell className="bg-card/30 text-foreground text-center">
              {formatDollars(baseline.eps)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-eps`}
                  className="border-border/30 text-foreground border-l-2 text-center"
                >
                  {formatDollars(yearProj.bear.eps)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-base-eps`}
                  className="text-foreground text-center"
                >
                  {formatDollars(yearProj.base.eps)}
                </TableCell>
                <TableCell
                  key={`${yearProj.year}-bull-eps`}
                  className="text-foreground text-center"
                >
                  {formatDollars(yearProj.bull.eps)}
                </TableCell>
              </>
            ))}
            <TableCell className="border-border/30 text-muted-foreground border-l-2 text-center">
              —
            </TableCell>
          </TableRow>

          {/* Share Price Range Row */}
          <TableRow className="border-border/50 hover:bg-card/30 bg-primary/5">
            <TableCell className="bg-background text-foreground sticky left-0 z-10 font-medium">
              Share Price Range
            </TableCell>
            <TableCell className="bg-card/30 text-foreground text-center font-semibold">
              {formatDollars(baseline.sharePriceLow)}
            </TableCell>
            {projections.map((yearProj) => (
              <>
                <TableCell
                  key={`${yearProj.year}-bear-price`}
                  className="border-border/30 border-l-2 text-center font-semibold text-red-400"
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
            <TableCell className="border-border/30 text-muted-foreground border-l-2 text-center">
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
