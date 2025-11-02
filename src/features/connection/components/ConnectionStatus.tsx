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

export function ConnectionStatus({ connectionStatus, loading, disconnecting, onConnect, onDisconnect }: ConnectionStatusProps) {
  return (
    <div className="flex items-center gap-4">
      <div className="flex items-center gap-2 px-3 py-2 rounded-lg bg-slate-800/50 border border-slate-700">
        {connectionStatus.connected ? (
          <>
            <div className="w-2 h-2 bg-green-400 rounded-full animate-pulse"></div>
            <CheckCircle className="h-4 w-4 text-green-400" />
            <span className="text-sm text-green-400 font-medium">Connected</span>
          </>
        ) : (
          <>
            <div className="w-2 h-2 bg-red-400 rounded-full"></div>
            <AlertCircle className="h-4 w-4 text-red-400" />
            <span className="text-sm text-red-400 font-medium">Disconnected</span>
          </>
        )}
      </div>
      {connectionStatus.connected ? (
        <Button
          onClick={onDisconnect}
          disabled={disconnecting}
          variant="outline"
          className="border-slate-600 hover:bg-slate-800 bg-transparent"
        >
          <WifiOff className="h-4 w-4 mr-2" />
          {disconnecting ? "Disconnecting..." : "Disconnect"}
        </Button>
      ) : (
        <Button
          onClick={onConnect}
          disabled={loading}
          className="bg-gradient-to-r from-blue-600 to-purple-600 hover:from-blue-700 hover:to-purple-700"
        >
          <Wifi className="h-4 w-4 mr-2" />
          {loading ? "Connecting..." : "Connect"}
        </Button>
      )}
    </div>
  )
}
