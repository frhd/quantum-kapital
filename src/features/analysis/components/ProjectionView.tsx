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
      <Card className="border-border bg-card/50 backdrop-blur-xs">
        <CardHeader>
          <CardTitle className="text-foreground text-xl">Forward Analysis - {symbol}</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="text-muted-foreground py-8 text-center">
            Unable to generate projections for {symbol}. This may be due to insufficient historical
            financial data.
          </p>
        </CardContent>
      </Card>
    )
  }

  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="text-foreground text-xl">Forward Analysis - {symbol}</CardTitle>
        <p className="text-muted-foreground mt-2 text-sm">
          Baseline year:{" "}
          <span className="text-foreground font-medium">{results.baseline.year}</span> (Actual) •
          Projection period:{" "}
          <span className="text-foreground font-medium">
            {results.projections[0]?.year} -{" "}
            {results.projections[results.projections.length - 1]?.year}
          </span>
        </p>
      </CardHeader>

      <CardContent className="space-y-6">
        <ProjectionTable results={results} />

        <ProjectionSummary
          projections={{
            base: results.projections.map((p) => p.base),
            bear: results.projections.map((p) => p.bear),
            bull: results.projections.map((p) => p.bull),
            cagr: results.cagr,
          }}
          assumptions={assumptions}
        />
      </CardContent>
    </Card>
  )
}
