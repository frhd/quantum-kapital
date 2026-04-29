import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "../../../shared/components/ui/table"
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

export function ScannerResults({
  results,
  lastUpdate,
  isRunning,
  error,
  onSelectSymbol,
}: ScannerResultsProps) {
  return (
    <Card className="border-slate-700 bg-slate-800/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-white">
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
            <p className="py-6 text-center text-sm text-slate-400">
              No results yet. Configure filters and click Start Scan.
            </p>
          )
        ) : (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow className="h-8 border-slate-700">
                  <TableHead className="w-16 py-2 text-right text-xs text-slate-300">
                    Rank
                  </TableHead>
                  <TableHead className="py-2 text-xs text-slate-300">Symbol</TableHead>
                  <TableHead className="py-2 text-xs text-slate-300">Exchange</TableHead>
                  <TableHead className="py-2 text-xs text-slate-300">Currency</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {results.map((row) => (
                  <TableRow
                    key={`${row.rank}-${row.contract.contract_id}`}
                    className="h-10 cursor-pointer border-slate-700 transition-colors hover:bg-slate-700/50"
                    onClick={() => onSelectSymbol(row.contract.symbol)}
                  >
                    <TableCell className="py-2 text-right text-sm text-white">{row.rank}</TableCell>
                    <TableCell className="py-2 font-medium text-white">
                      <div className="text-sm">{row.contract.symbol}</div>
                      {row.contract.local_symbol &&
                        row.contract.local_symbol !== row.contract.symbol && (
                          <div className="text-xs text-slate-500">{row.contract.local_symbol}</div>
                        )}
                    </TableCell>
                    <TableCell className="py-2 text-sm text-slate-300">
                      {row.contract.primary_exchange || row.contract.exchange}
                    </TableCell>
                    <TableCell className="py-2 text-sm text-slate-300">
                      {row.contract.currency}
                    </TableCell>
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
