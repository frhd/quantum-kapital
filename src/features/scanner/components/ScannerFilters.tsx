import { useState } from "react"
import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Button } from "../../../shared/components/ui/button"
import { Input } from "../../../shared/components/ui/input"
import { Label } from "../../../shared/components/ui/label"
import { Search, Square } from "lucide-react"
import type { ScannerSubscription } from "../../../shared/types"

const SCAN_CODES = [
  { value: "TOP_PERC_GAIN", label: "Top % Gainers" },
  { value: "TOP_PERC_LOSE", label: "Top % Losers" },
  { value: "MOST_ACTIVE", label: "Most Active" },
  { value: "HOT_BY_VOLUME", label: "Hot by Volume" },
  { value: "HOT_BY_PRICE", label: "Hot by Price" },
  { value: "TOP_TRADE_COUNT", label: "Top Trade Count" },
  { value: "HIGH_OPT_IMP_VOLAT", label: "High Implied Volatility" },
  { value: "LOW_OPT_IMP_VOLAT", label: "Low Implied Volatility" },
  { value: "TOP_OPEN_PERC_GAIN", label: "Top % Gain From Open" },
  { value: "HIGH_DIVIDEND_YIELD", label: "High Dividend Yield" },
] as const

const LOCATION_CODES = [
  { value: "STK.US.MAJOR", label: "US Major Exchanges" },
  { value: "STK.US", label: "All US Stocks" },
  { value: "STK.NASDAQ", label: "NASDAQ" },
  { value: "STK.NYSE", label: "NYSE" },
  { value: "STK.HK", label: "Hong Kong" },
] as const

type ScanCode = (typeof SCAN_CODES)[number]["value"]
type LocationCode = (typeof LOCATION_CODES)[number]["value"]

interface ScannerFiltersProps {
  isRunning: boolean
  onStart: (subscription: ScannerSubscription) => void
  onStop: () => void
}

const selectClass =
  "h-9 w-full rounded-md border border-slate-700 bg-slate-800/50 px-3 text-sm text-white focus:outline-hidden focus:ring-2 focus:ring-blue-500"

export function ScannerFilters({ isRunning, onStart, onStop }: ScannerFiltersProps) {
  const [scanCode, setScanCode] = useState<ScanCode>("TOP_PERC_GAIN")
  const [locationCode, setLocationCode] = useState<LocationCode>("STK.US.MAJOR")
  const [numberOfRows, setNumberOfRows] = useState<string>("25")
  const [abovePrice, setAbovePrice] = useState<string>("")
  const [belowPrice, setBelowPrice] = useState<string>("")
  const [aboveVolume, setAboveVolume] = useState<string>("")
  const [marketCapAbove, setMarketCapAbove] = useState<string>("")
  const [marketCapBelow, setMarketCapBelow] = useState<string>("")

  const parseOptional = (s: string): number | undefined => {
    const trimmed = s.trim()
    if (!trimmed) return undefined
    const n = Number(trimmed)
    return Number.isFinite(n) ? n : undefined
  }

  const handleStart = () => {
    const rows = parseInt(numberOfRows, 10)
    onStart({
      number_of_rows: Number.isFinite(rows) && rows > 0 ? Math.min(rows, 50) : 25,
      instrument: "STK",
      location_code: locationCode,
      scan_code: scanCode,
      above_price: parseOptional(abovePrice),
      below_price: parseOptional(belowPrice),
      above_volume: parseOptional(aboveVolume),
      market_cap_above: parseOptional(marketCapAbove),
      market_cap_below: parseOptional(marketCapBelow),
    })
  }

  return (
    <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="text-white flex items-center gap-2">
          <Search className="h-5 w-5 text-blue-400" />
          Scanner Filters
        </CardTitle>
        <CardDescription className="text-slate-400">
          Pick a scan and location, then start streaming results from IBKR.
        </CardDescription>
      </CardHeader>
      <CardContent className="space-y-4">
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          <div className="space-y-2">
            <Label htmlFor="scan-code" className="text-slate-300">Scan</Label>
            <select
              id="scan-code"
              className={selectClass}
              value={scanCode}
              onChange={(e) => setScanCode(e.target.value as ScanCode)}
              disabled={isRunning}
            >
              {SCAN_CODES.map((s) => (
                <option key={s.value} value={s.value}>{s.label}</option>
              ))}
            </select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="location-code" className="text-slate-300">Location</Label>
            <select
              id="location-code"
              className={selectClass}
              value={locationCode}
              onChange={(e) => setLocationCode(e.target.value as LocationCode)}
              disabled={isRunning}
            >
              {LOCATION_CODES.map((l) => (
                <option key={l.value} value={l.value}>{l.label}</option>
              ))}
            </select>
          </div>
          <div className="space-y-2">
            <Label htmlFor="row-count" className="text-slate-300">Rows</Label>
            <Input
              id="row-count"
              type="number"
              min={1}
              max={50}
              value={numberOfRows}
              onChange={(e) => setNumberOfRows(e.target.value)}
              disabled={isRunning}
            />
          </div>
        </div>

        <div className="grid grid-cols-2 md:grid-cols-5 gap-4">
          <div className="space-y-2">
            <Label htmlFor="above-price" className="text-slate-300">Min Price</Label>
            <Input id="above-price" type="number" placeholder="—" value={abovePrice}
              onChange={(e) => setAbovePrice(e.target.value)} disabled={isRunning} />
          </div>
          <div className="space-y-2">
            <Label htmlFor="below-price" className="text-slate-300">Max Price</Label>
            <Input id="below-price" type="number" placeholder="—" value={belowPrice}
              onChange={(e) => setBelowPrice(e.target.value)} disabled={isRunning} />
          </div>
          <div className="space-y-2">
            <Label htmlFor="above-volume" className="text-slate-300">Min Volume</Label>
            <Input id="above-volume" type="number" placeholder="—" value={aboveVolume}
              onChange={(e) => setAboveVolume(e.target.value)} disabled={isRunning} />
          </div>
          <div className="space-y-2">
            <Label htmlFor="mcap-above" className="text-slate-300">Min Mkt Cap</Label>
            <Input id="mcap-above" type="number" placeholder="—" value={marketCapAbove}
              onChange={(e) => setMarketCapAbove(e.target.value)} disabled={isRunning} />
          </div>
          <div className="space-y-2">
            <Label htmlFor="mcap-below" className="text-slate-300">Max Mkt Cap</Label>
            <Input id="mcap-below" type="number" placeholder="—" value={marketCapBelow}
              onChange={(e) => setMarketCapBelow(e.target.value)} disabled={isRunning} />
          </div>
        </div>

        <div className="flex gap-2">
          {!isRunning ? (
            <Button onClick={handleStart} className="bg-blue-600 hover:bg-blue-700">
              <Search className="h-4 w-4 mr-2" /> Start Scan
            </Button>
          ) : (
            <Button onClick={onStop} variant="outline" className="border-slate-600 text-slate-200">
              <Square className="h-4 w-4 mr-2" /> Stop
            </Button>
          )}
        </div>
      </CardContent>
    </Card>
  )
}
