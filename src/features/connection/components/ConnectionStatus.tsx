import { Button } from "../../../shared/components/ui/button"
import { CheckCircle, AlertCircle, WifiOff, Wifi } from "lucide-react"
import type { ConnectionStatus as ConnectionStatusType } from "../../../shared/types"

interface ConnectionStatusProps {
  connectionStatus: ConnectionStatusType
  loading: boolean
  disconnecting: boolean
  onConnect: () => void
  onDisconnect: () => void
}

export function ConnectionStatus({
  connectionStatus,
  loading,
  disconnecting,
  onConnect,
  onDisconnect,
}: ConnectionStatusProps) {
  return (
    <div className="flex items-center gap-4">
      <div className="flex items-center gap-2 rounded-lg border border-slate-700 bg-slate-800/50 px-3 py-2">
        {connectionStatus.connected ? (
          <>
            <div className="h-2 w-2 animate-pulse rounded-full bg-green-400"></div>
            <CheckCircle className="h-4 w-4 text-green-400" />
            <span className="text-sm font-medium text-green-400">Connected</span>
          </>
        ) : (
          <>
            <div className="h-2 w-2 rounded-full bg-red-400"></div>
            <AlertCircle className="h-4 w-4 text-red-400" />
            <span className="text-sm font-medium text-red-400">Disconnected</span>
          </>
        )}
      </div>
      {connectionStatus.connected ? (
        <Button
          onClick={onDisconnect}
          disabled={disconnecting}
          variant="outline"
          className="border-slate-600 bg-transparent hover:bg-slate-800"
        >
          <WifiOff className="mr-2 h-4 w-4" />
          {disconnecting ? "Disconnecting..." : "Disconnect"}
        </Button>
      ) : (
        <Button
          onClick={onConnect}
          disabled={loading}
          className="bg-linear-to-r from-blue-600 to-purple-600 hover:from-blue-700 hover:to-purple-700"
        >
          <Wifi className="mr-2 h-4 w-4" />
          {loading ? "Connecting..." : "Connect"}
        </Button>
      )}
    </div>
  )
}
