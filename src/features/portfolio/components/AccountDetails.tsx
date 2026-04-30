import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "../../../shared/components/ui/card"
import { Settings, Clock } from "lucide-react"
import { formatCurrency, anonymizeAccountNumber } from "../utils"
import type { AccountSummary as AccountSummaryType, ConnectionStatus } from "../../../shared/types"

interface AccountDetailsProps {
  accounts: string[]
  accountSummary: AccountSummaryType[]
  connectionStatus: ConnectionStatus
}

export function AccountDetails({
  accounts,
  accountSummary,
  connectionStatus,
}: AccountDetailsProps) {
  const getAccountValue = (tags: string[]): number => {
    for (const tag of tags) {
      const item = accountSummary.find((s) => s.tag === tag)
      if (item) {
        const value = parseFloat(item.value)
        return value
      }
    }
    return 0
  }

  const totalEquity = getAccountValue([
    "NetLiquidation",
    "NetLiquidationByCurrency",
    "TotalNetLiquidation",
  ])
  const availableFunds = getAccountValue(["AvailableFunds", "AvailableFunds-S", "AvailableFunds-C"])
  const buyingPower = getAccountValue(["BuyingPower", "BuyingPower-S"])

  return (
    <Card className="border-border bg-card/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="text-foreground flex items-center gap-2">
          <Settings className="h-5 w-5 text-orange-400" />
          Account Details
        </CardTitle>
        <CardDescription className="text-muted-foreground">
          Detailed account information
        </CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          {/* Key Account Metrics */}
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Net Liquidation</h4>
            <p className="text-foreground text-2xl font-bold">{formatCurrency(totalEquity)}</p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Available Funds</h4>
            <p className="text-foreground text-2xl font-bold">{formatCurrency(availableFunds)}</p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Buying Power</h4>
            <p className="text-foreground text-2xl font-bold">{formatCurrency(buyingPower)}</p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Total Cash</h4>
            <p className="text-foreground text-xl font-bold">
              {formatCurrency(
                getAccountValue(["TotalCashValue", "TotalCashBalance", "CashBalance"]),
              )}
            </p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Gross Position Value</h4>
            <p className="text-foreground text-xl font-bold">
              {formatCurrency(getAccountValue(["GrossPositionValue"]))}
            </p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Excess Liquidity</h4>
            <p className="text-foreground text-xl font-bold">
              {formatCurrency(getAccountValue(["ExcessLiquidity", "ExcessLiquidity-S"]))}
            </p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Initial Margin</h4>
            <p className="text-foreground text-xl font-bold">
              {formatCurrency(getAccountValue(["InitMarginReq", "InitMarginReq-S"]))}
            </p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Maintenance Margin</h4>
            <p className="text-foreground text-xl font-bold">
              {formatCurrency(getAccountValue(["MaintMarginReq", "MaintMarginReq-S"]))}
            </p>
          </div>
          <div className="border-border bg-background/50 rounded-lg border p-4">
            <h4 className="text-foreground mb-2 text-sm font-medium">Account Type</h4>
            <p className="text-foreground text-lg">
              {accountSummary.find((s) => s.tag === "AccountType")?.value || "N/A"}
            </p>
          </div>
        </div>

        {/* Account Info */}
        <div className="border-border bg-background/50 mt-6 rounded-lg border p-4">
          <div className="flex items-center justify-between">
            <div>
              <h4 className="text-foreground text-sm font-medium">Account ID</h4>
              <p className="text-foreground font-mono text-lg">
                {accounts[0] ? anonymizeAccountNumber(accounts[0]) : "N/A"}
              </p>
            </div>
            <div className="text-right">
              <h4 className="text-foreground text-sm font-medium">Server Time</h4>
              <p className="text-muted-foreground flex items-center gap-2 text-sm">
                <Clock className="h-4 w-4" />
                {connectionStatus.server_time || "N/A"}
              </p>
            </div>
          </div>
        </div>

        {accountSummary.length > 0 && (
          <div className="mt-6">
            <h4 className="text-foreground mb-4 text-sm font-medium">All Account Values</h4>
            <div className="grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-3">
              {accountSummary.map((item, index) => (
                <div
                  key={`${item.tag}-${index}`}
                  className="border-border bg-background/50 rounded-lg border p-3"
                >
                  <p className="text-muted-foreground text-xs">{item.tag}</p>
                  <p className="text-foreground text-sm font-medium">
                    {item.value} {item.currency}
                  </p>
                </div>
              ))}
            </div>
          </div>
        )}
      </CardContent>
    </Card>
  )
}
