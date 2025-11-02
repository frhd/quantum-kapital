import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { ProjectionTable } from "./ProjectionTable"
import { ProjectionSummary } from "./ProjectionSummary"
import type { ProjectionResults, ProjectionAssumptions } from "../../../shared/types"

interface ProjectionViewProps {
  results: ProjectionResults
  symbol: string
  assumptions?: ProjectionAssumptions
}

export function ProjectionView({ results, symbol, assumptions }: ProjectionViewProps) {
  // Validate results data
  if (!results || !results.baseline || !results.projections || results.projections.length === 0) {
    return (
      <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
        <CardHeader>
          <CardTitle className="text-xl text-white">
            Forward Analysis - {symbol}
          </CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-slate-400 text-center py-8">
            Unable to generate projections for {symbol}. This may be due to insufficient historical financial data.
          </p>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
      <CardHeader>
        <CardTitle className="text-xl text-white">
          Forward Analysis - {symbol}
        </CardTitle>
        <p className="text-sm text-slate-400 mt-2">
          Baseline year: <span className="text-white font-medium">{results.baseline.year}</span> (Actual) •{" "}
          Projection period: <span className="text-white font-medium">{results.projections[0]?.year} - {results.projections[results.projections.length - 1]?.year}</span>
        </p>
      </CardHeader>

      <CardContent className="space-y-6">
        <ProjectionTable results={results} />

        <ProjectionSummary
          projections={{
            base: results.projections.map(p => p.base),
            bear: results.projections.map(p => p.bear),
            bull: results.projections.map(p => p.bull),
            cagr: results.cagr,
          }}
          assumptions={assumptions}
        />
      </CardContent>
    </Card>
  )
}
