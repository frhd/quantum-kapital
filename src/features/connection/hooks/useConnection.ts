import { useState, useEffect, useRef } from "react"
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
  // Ref-guarded so StrictMode's double-mount doesn't fire two connect attempts.
  const hasBootstrappedRef = useRef(false)

  useEffect(() => {
    if (hasBootstrappedRef.current) return
    hasBootstrappedRef.current = true

    const checkAndAutoConnect = async () => {
      try {
        const status = await ibkrApi.getConnectionStatus()
        if (status.connected) {
          console.log("Found existing connection, reusing it")
          setConnectionStatus(status)
          return
        }
      } catch (err) {
        console.error("Failed to check existing connection:", err)
      }

      setLoading(true)
      try {
        await ibkrApi.connect(connectionSettings)
        const next = await ibkrApi.getConnectionStatus()
        setConnectionStatus(next)
      } catch (err) {
        console.error("Auto-connect to IBKR failed:", err)
        setError(
          typeof err === "string"
            ? err
            : "Failed to auto-connect to IBKR. Please ensure TWS/Gateway is running.",
        )
      } finally {
        setLoading(false)
      }
    }

    checkAndAutoConnect()
    // Intentional one-shot: bootstraps with whatever connectionSettings is at
    // mount time. User-edited settings flow through the manual Connect button.
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [])

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
      setError(
        typeof err === "string"
          ? err
          : "Failed to connect to IBKR. Please ensure TWS/Gateway is running.",
      )
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
      setError(typeof err === "string" ? err : "Failed to disconnect from IBKR")
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
