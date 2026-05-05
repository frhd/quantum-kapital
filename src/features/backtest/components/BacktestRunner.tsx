import { useState } from "react"

import {
  backtestRun,
  defaultBacktestSpec,
  type BacktestResult,
  type BacktestSpec,
  type FillModelKind,
} from "../../../shared/api/backtest"
import { Button } from "../../../shared/components/ui/button"
import { Card, CardContent, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Input } from "../../../shared/components/ui/input"
import { Label } from "../../../shared/components/ui/label"

interface BacktestRunnerProps {
  onResult?: (result: BacktestResult) => void
}

/**
 * Minimal spec form. Heavy / overnight runs go through `qk-backtest`
 * CLI; this is the in-app preview surface.
 */
export function BacktestRunner({ onResult }: BacktestRunnerProps) {
  const [spec, setSpec] = useState<BacktestSpec>(defaultBacktestSpec())
  const [symbolInput, setSymbolInput] = useState("")
  const [running, setRunning] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const updateSpec = (patch: Partial<BacktestSpec>) => setSpec((prev) => ({ ...prev, ...patch }))
  const updateFillModel = (fill: FillModelKind) => updateSpec({ fill_model: fill })

  const handleSymbolsBlur = () => {
    const symbols = symbolInput
      .split(/[\s,]+/)
      .map((s) => s.trim().toUpperCase())
      .filter(Boolean)
    updateSpec({ symbols })
  }

  const onRun = async () => {
    setRunning(true)
    setError(null)
    try {
      const result = await backtestRun(spec)
      onResult?.(result)
    } catch (e) {
      setError(String(e))
    } finally {
      setRunning(false)
    }
  }

  return (
    <Card className="border-border/50 bg-card/30">
      <CardHeader>
        <CardTitle className="text-base">Backtest spec</CardTitle>
      </CardHeader>
      <CardContent className="space-y-3">
        <div className="grid grid-cols-2 gap-3">
          <div>
            <Label htmlFor="bt-from">From (YYYY-MM-DD)</Label>
            <Input
              id="bt-from"
              value={spec.date_from}
              onChange={(e) => updateSpec({ date_from: e.target.value })}
            />
          </div>
          <div>
            <Label htmlFor="bt-to">To (inclusive)</Label>
            <Input
              id="bt-to"
              value={spec.date_to_inclusive}
              onChange={(e) => updateSpec({ date_to_inclusive: e.target.value })}
            />
          </div>
        </div>
        <div>
          <Label htmlFor="bt-symbols">Symbols (space- or comma-separated)</Label>
          <Input
            id="bt-symbols"
            placeholder="AAPL MSFT NVDA"
            value={symbolInput}
            onChange={(e) => setSymbolInput(e.target.value)}
            onBlur={handleSymbolsBlur}
          />
          <p className="text-muted-foreground mt-1 text-xs">
            {spec.symbols.length === 0
              ? "Enter at least one symbol"
              : `${spec.symbols.length} symbol${spec.symbols.length === 1 ? "" : "s"}`}
          </p>
        </div>
        <div className="grid grid-cols-3 gap-3">
          <div>
            <Label htmlFor="bt-slippage">Slippage (bps)</Label>
            <Input
              id="bt-slippage"
              type="number"
              min={0}
              max={500}
              value={spec.fill_model.kind === "naive_next_open" ? spec.fill_model.slippage_bps : 0}
              onChange={(e) =>
                updateFillModel({
                  kind: "naive_next_open",
                  slippage_bps: Number(e.target.value) || 0,
                })
              }
            />
          </div>
          <div>
            <Label htmlFor="bt-equity">Starting equity</Label>
            <Input
              id="bt-equity"
              type="number"
              min={1000}
              step={1000}
              value={spec.starting_equity_usd}
              onChange={(e) => updateSpec({ starting_equity_usd: Number(e.target.value) || 0 })}
            />
          </div>
          <div>
            <Label htmlFor="bt-hold">Max hold bars</Label>
            <Input
              id="bt-hold"
              type="number"
              min={1}
              max={60}
              value={spec.max_hold_bars}
              onChange={(e) => updateSpec({ max_hold_bars: Number(e.target.value) || 1 })}
            />
          </div>
        </div>
        <div className="flex items-center gap-2">
          <input
            id="bt-blackouts"
            type="checkbox"
            checked={spec.event_blackouts_enabled}
            onChange={(e) => updateSpec({ event_blackouts_enabled: e.target.checked })}
          />
          <Label htmlFor="bt-blackouts" className="text-sm">
            Honor event blackouts (P5)
          </Label>
        </div>
        <div className="flex items-center justify-between">
          <Button onClick={onRun} disabled={running || spec.symbols.length === 0}>
            {running ? "Running…" : "Run backtest"}
          </Button>
          {error && (
            <p className="text-destructive text-sm" role="alert">
              {error}
            </p>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
