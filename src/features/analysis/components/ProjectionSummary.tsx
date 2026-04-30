import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"
import { Info } from "lucide-react"
import type { ScenarioProjections, ProjectionAssumptions } from "../../../shared/types"
import { defaultProjectionAssumptions } from "../../../shared/types"

interface ProjectionSummaryProps {
  projections: ScenarioProjections
  assumptions?: ProjectionAssumptions
}

export function ProjectionSummary({
  projections,
  assumptions = defaultProjectionAssumptions,
}: ProjectionSummaryProps) {
  // Get baseline data from the first year of base case projections
  const baseline = projections.base[0]

  // Safety check: ensure baseline exists
  if (!baseline) {
    return (
      <Card className="border-border/50 bg-card/30">
        <CardContent className="pt-6">
          <p className="text-muted-foreground text-center">No projection data available</p>
        </CardContent>
      </Card>
    )
  }

  const baselineYear = baseline.year

  const formatBillions = (value: number) => `$${value.toFixed(2)}B`
  const formatPercent = (value: number) => `${value.toFixed(1)}%`
  const formatNumber = (value: number) => value.toFixed(0)

  return (
    <div className="space-y-4">
      {/* Baseline Metrics */}
      <Card className="border-border/50 bg-card/30">
        <CardHeader className="pb-3">
          <CardTitle className="text-foreground flex items-center gap-2 text-sm font-medium">
            <Info className="h-4 w-4 text-blue-400" />
            Baseline Metrics ({baselineYear})
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-4 text-sm md:grid-cols-4">
            <div>
              <p className="text-muted-foreground text-xs">Revenue</p>
              <p className="text-foreground font-semibold">{formatBillions(baseline.revenue)}</p>
            </div>
            <div>
              <p className="text-muted-foreground text-xs">Net Income</p>
              <p className="text-foreground font-semibold">{formatBillions(baseline.netIncome)}</p>
            </div>
            <div>
              <p className="text-muted-foreground text-xs">Net Margin</p>
              <p className="text-foreground font-semibold">
                {formatPercent(baseline.netIncomeMargins)}
              </p>
            </div>
            <div>
              <p className="text-muted-foreground text-xs">EPS</p>
              <p className="text-foreground font-semibold">${baseline.eps.toFixed(2)}</p>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Scenario Assumptions */}
      <Card className="border-border/50 bg-card/30">
        <CardHeader className="pb-3">
          <CardTitle className="text-foreground text-sm font-medium">
            Scenario Assumptions
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow className="border-border/50">
                  <TableHead className="text-muted-foreground">Assumption</TableHead>
                  <TableHead className="text-center text-red-400">Bear</TableHead>
                  <TableHead className="text-center text-blue-400">Base</TableHead>
                  <TableHead className="text-center text-green-400">Bull</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow className="border-border/50">
                  <TableCell className="text-foreground text-sm">Annual Revenue Growth</TableCell>
                  <TableCell className="text-foreground text-center text-sm">
                    {formatPercent(assumptions.bearRevenueGrowth)}
                  </TableCell>
                  <TableCell className="text-foreground text-center text-sm">
                    {formatPercent(assumptions.baseRevenueGrowth)}
                  </TableCell>
                  <TableCell className="text-foreground text-center text-sm">
                    {formatPercent(assumptions.bullRevenueGrowth)}
                  </TableCell>
                </TableRow>
                <TableRow className="border-border/50">
                  <TableCell className="text-foreground text-sm">
                    Margin Change (ppts/year)
                  </TableCell>
                  <TableCell className="text-foreground text-center text-sm">
                    {assumptions.bearMarginChange > 0 ? "+" : ""}
                    {assumptions.bearMarginChange.toFixed(1)}
                  </TableCell>
                  <TableCell className="text-foreground text-center text-sm">
                    {assumptions.baseMarginChange > 0 ? "+" : ""}
                    {assumptions.baseMarginChange.toFixed(1)}
                  </TableCell>
                  <TableCell className="text-foreground text-center text-sm">
                    {assumptions.bullMarginChange > 0 ? "+" : ""}
                    {assumptions.bullMarginChange.toFixed(1)}
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>

          {/* Valuation Assumptions */}
          <div className="border-border/50 mt-4 border-t pt-4">
            <div className="grid grid-cols-2 gap-4 text-sm md:grid-cols-4">
              <div>
                <p className="text-muted-foreground text-xs">P/E Range (Profitable)</p>
                <p className="text-foreground font-semibold">
                  {formatNumber(assumptions.peLow)} - {formatNumber(assumptions.peHigh)}
                </p>
              </div>
              <div>
                <p className="text-muted-foreground text-xs">P/S Range (Unprofitable)</p>
                <p className="text-foreground font-semibold">
                  {formatNumber(assumptions.psLow)} - {formatNumber(assumptions.psHigh)}
                </p>
              </div>
              <div>
                <p className="text-muted-foreground text-xs">Projection Period</p>
                <p className="text-foreground font-semibold">{assumptions.years} years</p>
              </div>
              <div>
                <p className="text-muted-foreground text-xs">Shares Growth</p>
                <p className="text-foreground font-semibold">
                  {assumptions.sharesGrowth > 0 ? "+" : ""}
                  {formatPercent(assumptions.sharesGrowth)}
                </p>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Data Sources */}
      <Card className="border-border/50 bg-card/30">
        <CardContent className="pt-4">
          <div className="text-muted-foreground space-y-1 text-xs">
            <p className="text-foreground font-medium">Data Sources & Methodology:</p>
            <ul className="ml-2 list-inside list-disc space-y-0.5">
              <li>Historical financials: Based on company filings and reported results</li>
              <li>Baseline metrics: Most recent fiscal year data (FY{baselineYear - 1})</li>
              <li>
                Analyst estimates: Consensus EPS forecasts from Wall Street analysts (via Alpha
                Vantage)
              </li>
              <li>
                Growth projections: Scenario-based modeling with varying revenue and margin
                assumptions
              </li>
              <li>
                Valuation: Hybrid approach - P/E multiples for profitable companies, P/S multiples
                for unprofitable companies
              </li>
              <li>
                CAGR: Compound annual growth rate from {baselineYear} to{" "}
                {baselineYear + assumptions.years - 1}
              </li>
            </ul>
            <p className="text-muted-foreground mt-2 italic">
              Note: These projections are illustrative scenarios based on current data and
              assumptions. Actual results may vary significantly. Not investment advice.
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
