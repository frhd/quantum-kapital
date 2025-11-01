import { Card, CardContent, CardDescription, CardHeader, CardTitle } from "../../../shared/components/ui/card"
import { Settings, Clock } from "lucide-react"
import { formatCurrency, anonymizeAccountNumber } from "../utils"
import type { AccountSummary as AccountSummaryType, ConnectionStatus } from "../../../shared/types"

interface AccountDetailsProps {
  accounts: string[]
  accountSummary: AccountSummaryType[]
  connectionStatus: ConnectionStatus
}

export function AccountDetails({ accounts, accountSummary, connectionStatus }: AccountDetailsProps) {
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

  const totalEquity = getAccountValue(["NetLiquidation", "NetLiquidationByCurrency", "TotalNetLiquidation"])
  const availableFunds = getAccountValue(["AvailableFunds", "AvailableFunds-S", "AvailableFunds-C"])
  const buyingPower = getAccountValue(["BuyingPower", "BuyingPower-S"])

  return (
    <Card className="bg-slate-800/50 border-slate-700 backdrop-blur-sm">
      <CardHeader>
        <CardTitle className="text-white flex items-center gap-2">
          <Settings className="h-5 w-5 text-orange-400" />
          Account Details
        </CardTitle>
        <CardDescription className="text-slate-400">Detailed account information</CardDescription>
      </CardHeader>
      <CardContent>
        <div className="grid grid-cols-1 md:grid-cols-3 gap-4">
          {/* Key Account Metrics */}
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Net Liquidation</h4>
            <p className="text-2xl font-bold text-white">
              {formatCurrency(totalEquity)}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Available Funds</h4>
            <p className="text-2xl font-bold text-white">
              {formatCurrency(availableFunds)}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Buying Power</h4>
            <p className="text-2xl font-bold text-white">
              {formatCurrency(buyingPower)}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Total Cash</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["TotalCashValue", "TotalCashBalance", "CashBalance"]))}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Gross Position Value</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["GrossPositionValue"]))}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Excess Liquidity</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["ExcessLiquidity", "ExcessLiquidity-S"]))}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Initial Margin</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["InitMarginReq", "InitMarginReq-S"]))}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Maintenance Margin</h4>
            <p className="text-xl font-bold text-white">
              {formatCurrency(getAccountValue(["MaintMarginReq", "MaintMarginReq-S"]))}
            </p>
          </div>
          <div className="p-4 bg-slate-900/50 rounded-lg border border-slate-700">
            <h4 className="text-sm font-medium text-slate-300 mb-2">Account Type</h4>
            <p className="text-lg text-white">
              {accountSummary.find(s => s.tag === "AccountType")?.value || "N/A"}
            </p>
          </div>
        </div>
        
        {/* Account Info */}
        <div className="mt-6 p-4 bg-slate-900/50 rounded-lg border border-slate-700">
          <div className="flex justify-between items-center">
            <div>
              <h4 className="text-sm font-medium text-slate-300">Account ID</h4>
              <p className="text-lg font-mono text-white">{accounts[0] ? anonymizeAccountNumber(accounts[0]) : "N/A"}</p>
            </div>
            <div className="text-right">
              <h4 className="text-sm font-medium text-slate-300">Server Time</h4>
              <p className="text-sm text-slate-400 flex items-center gap-2">
                <Clock className="h-4 w-4" />
                {connectionStatus.server_time || "N/A"}
              </p>
            </div>
          </div>
        </div>
        
        {accountSummary.length > 0 && (
          <div className="mt-6">
            <h4 className="text-sm font-medium text-slate-300 mb-4">All Account Values</h4>
            <div className="grid grid-cols-1 md:grid-cols-2 lg:grid-cols-3 gap-3">
              {accountSummary.map((item, index) => (
                <div
                  key={`${item.tag}-${index}`}
                  className="p-3 bg-slate-900/50 rounded-lg border border-slate-700"
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