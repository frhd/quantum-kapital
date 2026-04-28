import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Table, TableBody, TableCell, TableHead, TableHeader, TableRow } from "../../../shared/components/ui/table"
import { Skeleton } from "../../../shared/components/ui/skeleton"
import { Alert, AlertDescription } from "../../../shared/components/ui/alert"
import { TrendingUp, AlertCircle } from "lucide-react"
import type { ScannerData } from "../../../shared/types"

interface ScannerResultsProps {
  results: ScannerData[]
  lastUpdate: Date | null
  isRunning: boolean
  error: string | null
  onSelectSymbol: (symbol: string) => void
}

export function ScannerResults({ results, lastUpdate, isRunning, error, onSelectSymbol }: ScannerResultsProps) {
  return (
    <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="text-white flex items-center gap-2">
          <TrendingUp className="h-5 w-5 text-blue-400" />
          Scanner Results
        </CardTitle>
        <CardDescription className="text-slate-400">
          {isRunning
            ? `Updates ~every 30 seconds${lastUpdate ? ` — last update ${lastUpdate.toLocaleTimeString()}` : " — waiting for first batch…"}`
            : "Click a row to open the symbol in Analysis."}
        </CardDescription>
      </CardHeader>
      <CardContent>
        {error && (
          <Alert variant="destructive" className="mb-4">
            <AlertCircle className="h-4 w-4" />
            <AlertDescription>{error}</AlertDescription>
          </Alert>
        )}

        {results.length === 0 ? (
          isRunning ? (
            <div className="space-y-2">
              <Skeleton className="h-8 w-full bg-slate-700/50" />
              <Skeleton className="h-8 w-full bg-slate-700/50" />
              <Skeleton className="h-8 w-full bg-slate-700/50" />
            </div>
          ) : (
            <p className="text-slate-400 text-sm py-6 text-center">No results yet. Configure filters and click Start Scan.</p>
          )
        ) : (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow className="border-slate-700 h-8">
                  <TableHead className="text-slate-300 text-right text-xs py-2 w-16">Rank</TableHead>
                  <TableHead className="text-slate-300 text-xs py-2">Symbol</TableHead>
                  <TableHead className="text-slate-300 text-xs py-2">Exchange</TableHead>
                  <TableHead className="text-slate-300 text-xs py-2">Currency</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {results.map((row) => (
                  <TableRow
                    key={`${row.rank}-${row.contract.contract_id}`}
                    className="border-slate-700 h-10 cursor-pointer hover:bg-slate-700/50 transition-colors"
                    onClick={() => onSelectSymbol(row.contract.symbol)}
                  >
                    <TableCell className="text-right text-white text-sm py-2">{row.rank}</TableCell>
                    <TableCell className="font-medium text-white py-2">
                      <div className="text-sm">{row.contract.symbol}</div>
                      {row.contract.local_symbol && row.contract.local_symbol !== row.contract.symbol && (
                        <div className="text-xs text-slate-500">{row.contract.local_symbol}</div>
                      )}
                    </TableCell>
                    <TableCell className="text-slate-300 text-sm py-2">
                      {row.contract.primary_exchange || row.contract.exchange}
                    </TableCell>
                    <TableCell className="text-slate-300 text-sm py-2">{row.contract.currency}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
