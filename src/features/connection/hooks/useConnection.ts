import { useState } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import type { ConnectionConfig, ConnectionStatus } from "../../../shared/types"

export function useConnection() {
  const [connectionStatus, setConnectionStatus] = useState<ConnectionStatus>({
    connected: false,
    server_time: null,
    client_id: 100,
  })
  const [connectionSettings, setConnectionSettings] = useState<ConnectionConfig>({
    host: "127.0.0.1",
    port: 4004,
    client_id: 100,
  })
  const [loading, setLoading] = useState(false)
  const [error, setError] = useState<string | null>(null)

  const connect = async () => {
    setLoading(true)
    setError(null)
    try {
      await ibkrApi.connect(connectionSettings)
      const status = await ibkrApi.getConnectionStatus()
      setConnectionStatus(status)
      return status
    } catch (err) {
      console.error("Failed to connect to IBKR:", err)
      setError(typeof err === 'string' ? err : 'Failed to connect to IBKR. Please ensure TWS/Gateway is running.')
      throw err
    } finally {
      setLoading(false)
    }
  }

  const disconnect = async () => {
    try {
      await ibkrApi.disconnect()
      setConnectionStatus({
        connected: false,
        server_time: null,
        client_id: connectionSettings.client_id,
      })
      setError(null)
    } catch (err) {
      console.error("Failed to disconnect:", err)
      setError(typeof err === 'string' ? err : 'Failed to disconnect from IBKR')
      throw err
    }
  }

  return {
    connectionStatus,
    connectionSettings,
    setConnectionSettings,
    loading,
    error,
    connect,
    disconnect,
  }
}