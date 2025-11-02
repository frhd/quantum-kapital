import { useState, useCallback } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { AccountSummary, Position } from "../../../shared/types"

export function useAccountData() {
  const [accounts, setAccounts] = useState<string[]>([])
  const [accountSummary, setAccountSummary] = useState<AccountSummary[]>([])
  const [positions, setPositions] = useState<Position[]>([])

  const fetchAccountData = useCallback(async () => {
    try {
      // Fetch accounts
      const accountList = await ibkrApi.getAccounts()
      setAccounts(accountList)

      if (accountList.length > 0) {
        // Get account summary for the first account
        const summary = await ibkrApi.getAccountSummary(accountList[0])
        console.log("Account summary received:", summary)
        setAccountSummary(summary)

        // Get all positions
        const pos = await ibkrApi.getPositions()
        console.log("Positions received:", pos)
        setPositions(pos)

        // Subscribe to market data for each position
        for (const position of pos) {
          await ibkrApi.subscribeMarketData(position.symbol)
        }
      } else {
        console.warn("No accounts found")
      }
    } catch (err) {
      console.error("Failed to fetch account data:", err)
      throw err
    }
  }, [])

  const clearAccountData = useCallback(() => {
    setAccounts([])
    setAccountSummary([])
    setPositions([])
  }, [])

  return {
    accounts,
    accountSummary,
    positions,
    fetchAccountData,
    clearAccountData,
  }
}
