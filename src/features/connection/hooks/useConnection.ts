import { useState, useEffect } from "react"
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
  const [disconnecting, setDisconnecting] = useState(false)
  const [error, setError] = useState<string | null>(null)
  const [hasCheckedConnection, setHasCheckedConnection] = useState(false)

  // Check for existing connection on mount (only once)
  useEffect(() => {
    if (!hasCheckedConnection) {
      const checkExistingConnection = async () => {
        try {
          const status = await ibkrApi.getConnectionStatus()
          if (status.connected) {
            console.log("Found existing connection, reusing it")
            setConnectionStatus(status)
          }
        } catch (err) {
          console.error("Failed to check existing connection:", err)
        } finally {
          setHasCheckedConnection(true)
        }
      }

      checkExistingConnection()
    }
  }, [hasCheckedConnection])

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
    console.log("🔴 FRONTEND: Disconnect function called")
    setDisconnecting(true)
    console.log("🔴 FRONTEND: Set disconnecting = true")
    setError(null)
    try {
      console.log("🔴 FRONTEND: Calling ibkrApi.disconnect()")
      const result = await ibkrApi.disconnect()
      console.log("🔴 FRONTEND: ibkrApi.disconnect() returned:", result)
      setConnectionStatus({
        connected: false,
        server_time: null,
        client_id: connectionSettings.client_id,
      })
      console.log("🔴 FRONTEND: Successfully disconnected from IBKR")
    } catch (err) {
      console.error("🔴 FRONTEND: Failed to disconnect:", err)
      setError(typeof err === 'string' ? err : 'Failed to disconnect from IBKR')
      throw err
    } finally {
      console.log("🔴 FRONTEND: Setting disconnecting = false")
      setDisconnecting(false)
    }
  }

  return {
    connectionStatus,
    connectionSettings,
    setConnectionSettings,
    loading,
    disconnecting,
    error,
    connect,
    disconnect,
  }
}
