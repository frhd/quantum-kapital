import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../shared/components/ui/table"
import { Info } from "lucide-react"
import type { ScenarioProjections, ProjectionAssumptions } from "../../../shared/types"
import { defaultProjectionAssumptions } from "../../../shared/types"

interface ProjectionSummaryProps {
  projections: ScenarioProjections
  assumptions?: ProjectionAssumptions
}

export function ProjectionSummary({ projections, assumptions = defaultProjectionAssumptions }: ProjectionSummaryProps) {
  // Get baseline data from the first year of base case projections
  const baseline = projections.base[0]
  const baselineYear = baseline.year

  const formatBillions = (value: number) => `$${value.toFixed(2)}B`
  const formatPercent = (value: number) => `${value.toFixed(1)}%`
  const formatNumber = (value: number) => value.toFixed(0)

  return (
    <div className="space-y-4">
      {/* Baseline Metrics */}
      <Card className="bg-slate-800/30 border-slate-700/50">
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-medium text-slate-300 flex items-center gap-2">
            <Info className="h-4 w-4 text-blue-400" />
            Baseline Metrics ({baselineYear})
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 md:grid-cols-4 gap-4 text-sm">
            <div>
              <p className="text-slate-500 text-xs">Revenue</p>
              <p className="text-white font-semibold">{formatBillions(baseline.revenue)}</p>
            </div>
            <div>
              <p className="text-slate-500 text-xs">Net Income</p>
              <p className="text-white font-semibold">{formatBillions(baseline.netIncome)}</p>
            </div>
            <div>
              <p className="text-slate-500 text-xs">Net Margin</p>
              <p className="text-white font-semibold">{formatPercent(baseline.netIncomeMargins)}</p>
            </div>
            <div>
              <p className="text-slate-500 text-xs">EPS</p>
              <p className="text-white font-semibold">${baseline.eps.toFixed(2)}</p>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Scenario Assumptions */}
      <Card className="bg-slate-800/30 border-slate-700/50">
        <CardHeader className="pb-3">
          <CardTitle className="text-sm font-medium text-slate-300">
            Scenario Assumptions
          </CardTitle>
        </CardHeader>
        <CardContent>
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow className="border-slate-700/50">
                  <TableHead className="text-slate-400">Assumption</TableHead>
                  <TableHead className="text-center text-red-400">Bear</TableHead>
                  <TableHead className="text-center text-blue-400">Base</TableHead>
                  <TableHead className="text-center text-green-400">Bull</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                <TableRow className="border-slate-700/50">
                  <TableCell className="text-slate-300 text-sm">Annual Revenue Growth</TableCell>
                  <TableCell className="text-center text-white text-sm">
                    {formatPercent(assumptions.bearRevenueGrowth)}
                  </TableCell>
                  <TableCell className="text-center text-white text-sm">
                    {formatPercent(assumptions.baseRevenueGrowth)}
                  </TableCell>
                  <TableCell className="text-center text-white text-sm">
                    {formatPercent(assumptions.bullRevenueGrowth)}
                  </TableCell>
                </TableRow>
                <TableRow className="border-slate-700/50">
                  <TableCell className="text-slate-300 text-sm">Margin Change (ppts/year)</TableCell>
                  <TableCell className="text-center text-white text-sm">
                    {assumptions.bearMarginChange > 0 ? '+' : ''}{assumptions.bearMarginChange.toFixed(1)}
                  </TableCell>
                  <TableCell className="text-center text-white text-sm">
                    {assumptions.baseMarginChange > 0 ? '+' : ''}{assumptions.baseMarginChange.toFixed(1)}
                  </TableCell>
                  <TableCell className="text-center text-white text-sm">
                    {assumptions.bullMarginChange > 0 ? '+' : ''}{assumptions.bullMarginChange.toFixed(1)}
                  </TableCell>
                </TableRow>
              </TableBody>
            </Table>
          </div>

          {/* Valuation Assumptions */}
          <div className="mt-4 pt-4 border-t border-slate-700/50">
            <div className="grid grid-cols-2 md:grid-cols-3 gap-4 text-sm">
              <div>
                <p className="text-slate-500 text-xs">P/E Range</p>
                <p className="text-white font-semibold">
                  {formatNumber(assumptions.peLow)} - {formatNumber(assumptions.peHigh)}
                </p>
              </div>
              <div>
                <p className="text-slate-500 text-xs">Projection Period</p>
                <p className="text-white font-semibold">{assumptions.years} years</p>
              </div>
              <div>
                <p className="text-slate-500 text-xs">Shares Growth</p>
                <p className="text-white font-semibold">
                  {assumptions.sharesGrowth > 0 ? '+' : ''}{formatPercent(assumptions.sharesGrowth)}
                </p>
              </div>
            </div>
          </div>
        </CardContent>
      </Card>

      {/* Data Sources */}
      <Card className="bg-slate-800/30 border-slate-700/50">
        <CardContent className="pt-4">
          <div className="text-xs text-slate-400 space-y-1">
            <p className="font-medium text-slate-300">Data Sources & Methodology:</p>
            <ul className="list-disc list-inside space-y-0.5 ml-2">
              <li>Historical financials: Based on company filings and reported results</li>
              <li>Baseline metrics: Most recent fiscal year data (FY{baselineYear - 1})</li>
              <li>Growth projections: Scenario-based modeling with varying revenue and margin assumptions</li>
              <li>Valuation: Price targets calculated using projected EPS × estimated P/E multiples</li>
              <li>CAGR: Compound annual growth rate from {baselineYear} to {baselineYear + assumptions.years - 1}</li>
            </ul>
            <p className="mt-2 text-slate-500 italic">
              Note: These projections are illustrative scenarios based on current data and assumptions.
              Actual results may vary significantly. Not investment advice.
            </p>
          </div>
        </CardContent>
      </Card>
    </div>
  )
}
