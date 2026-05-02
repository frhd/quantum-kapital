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
import { Button } from "../../../shared/components/ui/button"
import { TrendingUp, AlertCircle, LineChart, Plus } from "lucide-react"
import type { ScannerData } from "../../../shared/types"
import { useTickerNavigate } from "../../workspace/hooks/useTickerNavigate"

interface ScannerResultsProps {
  results: ScannerData[]
  lastUpdate: Date | null
  isRunning: boolean
  error: string | null
  onAddToTracker: (row: ScannerData) => void
}

export function ScannerResults({
  results,
  lastUpdate,
  isRunning,
  error,
  onAddToTracker,
}: ScannerResultsProps) {
  const navigate = useTickerNavigate()
  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="text-foreground flex items-center gap-2">
          <TrendingUp className="h-5 w-5 text-blue-400" />
          Scanner Results
        </CardTitle>
        <CardDescription className="text-muted-foreground">
          {isRunning
            ? `Updates ~every 30 seconds${lastUpdate ? ` — last update ${lastUpdate.toLocaleTimeString()}` : " — waiting for first batch…"}`
            : "Use Analyze or Add to tracker on a row."}
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
              <Skeleton className="bg-secondary/50 h-8 w-full" />
              <Skeleton className="bg-secondary/50 h-8 w-full" />
              <Skeleton className="bg-secondary/50 h-8 w-full" />
            </div>
          ) : (
            <p className="text-muted-foreground py-6 text-center text-sm">
              No results yet. Configure filters and click Start Scan.
            </p>
          )
        ) : (
          <div className="overflow-x-auto">
            <Table>
              <TableHeader>
                <TableRow className="border-border h-8">
                  <TableHead className="text-foreground w-16 py-2 text-right text-xs">
                    Rank
                  </TableHead>
                  <TableHead className="text-foreground py-2 text-xs">Symbol</TableHead>
                  <TableHead className="text-foreground py-2 text-xs">Exchange</TableHead>
                  <TableHead className="text-foreground py-2 text-xs">Currency</TableHead>
                  <TableHead className="text-foreground w-48 py-2 text-right text-xs">
                    Actions
                  </TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {results.map((row) => (
                  <TableRow
                    key={`${row.rank}-${row.contract.contract_id}`}
                    className="border-border h-10"
                  >
                    <TableCell className="text-foreground py-2 text-right text-sm">
                      {row.rank}
                    </TableCell>
                    <TableCell className="text-foreground py-2 font-medium">
                      <div className="text-sm">{row.contract.symbol}</div>
                      {row.contract.local_symbol &&
                        row.contract.local_symbol !== row.contract.symbol && (
                          <div className="text-muted-foreground text-xs">
                            {row.contract.local_symbol}
                          </div>
                        )}
                    </TableCell>
                    <TableCell className="text-foreground py-2 text-sm">
                      {row.contract.primary_exchange || row.contract.exchange}
                    </TableCell>
                    <TableCell className="text-foreground py-2 text-sm">
                      {row.contract.currency}
                    </TableCell>
                    <TableCell className="py-2 text-right">
                      <div className="flex justify-end gap-1">
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-foreground hover:text-foreground h-7 px-2 text-xs"
                          onClick={() => navigate(row.contract.symbol, "overview")}
                          title="Open in analysis"
                        >
                          <LineChart className="h-4 w-4" />
                          Analyze
                        </Button>
                        <Button
                          variant="ghost"
                          size="sm"
                          className="text-foreground hover:text-foreground h-7 px-2 text-xs"
                          onClick={() => onAddToTracker(row)}
                          title="Add to tracker"
                        >
                          <Plus className="h-4 w-4" />
                          Track
                        </Button>
                      </div>
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
