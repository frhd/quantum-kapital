import { useState } from "react"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { ForwardAnalysisTable } from "./ForwardAnalysisTable"
import { ProjectionSummary } from "./ProjectionSummary"
import { TrendingDown, Minus, TrendingUp } from "lucide-react"
import type { ScenarioProjections, ScenarioType, ProjectionAssumptions } from "../../../shared/types"
import { cn } from "../../../shared/lib/utils"

interface ScenarioTabsProps {
  projections: ScenarioProjections
  symbol: string
  assumptions?: ProjectionAssumptions
}

export function ScenarioTabs({ projections, symbol, assumptions }: ScenarioTabsProps) {
  const [activeScenario, setActiveScenario] = useState<ScenarioType>('base')

  // Validate projections data
  if (!projections || !projections.base || !projections.bear || !projections.bull ||
      projections.base.length === 0 || projections.bear.length === 0 || projections.bull.length === 0) {
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

  const scenarios = [
    {
      key: 'bear' as ScenarioType,
      label: 'Bear Case',
      icon: TrendingDown,
      color: 'text-red-400',
      bgColor: 'bg-red-500/10',
      activeBg: 'bg-red-500/20',
      borderColor: 'border-red-500/50',
    },
    {
      key: 'base' as ScenarioType,
      label: 'Base Case',
      icon: Minus,
      color: 'text-blue-400',
      bgColor: 'bg-blue-500/10',
      activeBg: 'bg-blue-500/20',
      borderColor: 'border-blue-500/50',
    },
    {
      key: 'bull' as ScenarioType,
      label: 'Bull Case',
      icon: TrendingUp,
      color: 'text-green-400',
      bgColor: 'bg-green-500/10',
      activeBg: 'bg-green-500/20',
      borderColor: 'border-green-500/50',
    },
  ]

  const activeProjection = projections[activeScenario]
  const activeCagr = projections.cagr[activeScenario]

  return (
    <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
      <CardHeader>
        <div className="flex flex-col sm:flex-row sm:items-center sm:justify-between gap-4">
          <CardTitle className="text-xl text-white">
            Forward Analysis - {symbol}
          </CardTitle>

          {/* Scenario Tabs */}
          <div className="flex gap-2">
            {scenarios.map((scenario) => {
              const Icon = scenario.icon
              const isActive = activeScenario === scenario.key

              return (
                <button
                  key={scenario.key}
                  onClick={() => setActiveScenario(scenario.key)}
                  className={cn(
                    "flex items-center gap-2 px-4 py-2 rounded-lg transition-all",
                    "border backdrop-blur-sm",
                    isActive
                      ? `${scenario.activeBg} ${scenario.borderColor} ${scenario.color}`
                      : `${scenario.bgColor} border-slate-700 text-slate-400 hover:${scenario.color}`,
                  )}
                >
                  <Icon className="h-4 w-4" />
                  <span className="font-medium text-sm">{scenario.label}</span>
                </button>
              )
            })}
          </div>
        </div>
      </CardHeader>

      <CardContent className="space-y-6">
        <ForwardAnalysisTable
          projections={activeProjection}
          cagr={activeCagr}
          scenarioType={activeScenario}
        />

        <ProjectionSummary
          projections={projections}
          assumptions={assumptions}
        />
      </CardContent>
    </Card>
  )
}
