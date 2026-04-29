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
    <Card className="border-slate-700 bg-slate-800/50 backdrop-blur-xs">
      <CardHeader>
        <CardTitle className="flex items-center gap-2 text-white">
          <Settings className="h-5 w-5 text-orange-400" />
          Account Details
        </CardTitle>
        <CardDescription className="text-slate-400">Detailed account information</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 gap-4 md:grid-cols-3">
          {/* Key Account Metrics */}
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Net Liquidation</h4>
            <p className="text-2xl font-bold text-white">{formatCurrency(totalEquity)}</p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Available Funds</h4>
            <p className="text-2xl font-bold text-white">{formatCurrency(availableFunds)}</p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Buying Power</h4>
            <p className="text-2xl font-bold text-white">{formatCurrency(buyingPower)}</p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Total Cash</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(
                getAccountValue(["TotalCashValue", "TotalCashBalance", "CashBalance"]),
              )}
            </p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Gross Position Value</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["GrossPositionValue"]))}
            </p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Excess Liquidity</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["ExcessLiquidity", "ExcessLiquidity-S"]))}
            </p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Initial Margin</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["InitMarginReq", "InitMarginReq-S"]))}
            </p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Maintenance Margin</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["MaintMarginReq", "MaintMarginReq-S"]))}
            </p>
          </div>
          <div className="rounded-lg border border-slate-700 bg-slate-900/50 p-4">
            <h4 className="mb-2 text-sm font-medium text-slate-300">Account Type</h4>
            <p className="text-lg text-white">
              {accountSummary.find((s) => s.tag === "AccountType")?.value || "N/A"}
            </p>
          </div>
        </div>

        {/* Account Info */}
        <div className="mt-6 rounded-lg border border-slate-700 bg-slate-900/50 p-4">
          <div className="flex items-center justify-between">
            <div>
              <h4 className="text-sm font-medium text-slate-300">Account ID</h4>
              <p className="font-mono text-lg text-white">
                {accounts[0] ? anonymizeAccountNumber(accounts[0]) : "N/A"}
              </p>
            </div>
            <div className="text-right">
              <h4 className="text-sm font-medium text-slate-300">Server Time</h4>
              <p className="flex items-center gap-2 text-sm text-slate-400">
                <Clock className="h-4 w-4" />
                {connectionStatus.server_time || "N/A"}
              </p>
            </div>
          </div>
        </div>

        {accountSummary.length > 0 && (
          <div className="mt-6">
            <h4 className="mb-4 text-sm font-medium text-slate-300">All Account Values</h4>
            <div className="grid grid-cols-1 gap-3 md:grid-cols-2 lg:grid-cols-3">
              {accountSummary.map((item, index) => (
                <div
                  key={`${item.tag}-${index}`}
                  className="rounded-lg border border-slate-700 bg-slate-900/50 p-3"
                >
                  <p className="text-xs text-slate-400">{item.tag}</p>
                  <p className="text-sm font-medium text-white">
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
