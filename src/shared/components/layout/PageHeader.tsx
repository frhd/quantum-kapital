import { ConnectionStatus } from "../../../features/connection/components/ConnectionStatus"
import type { ConnectionStatus as ConnectionStatusType } from "../../types"

interface PageHeaderProps {
  connectionStatus: ConnectionStatusType
  loading: boolean
  disconnecting: boolean
  onConnect: () => void
  onDisconnect: () => void
}

export function PageHeader({
  connectionStatus,
  loading,
  disconnecting,
  onConnect,
  onDisconnect,
}: PageHeaderProps) {
  return (
    <div className="flex items-center justify-between">
      <div>
        <h1 className="text-4xl font-bold text-white">The Road to 1M</h1>
        <p className="mt-1 text-slate-400">Portfolio Dashboard</p>
      </div>
      <ConnectionStatus
        connectionStatus={connectionStatus}
        loading={loading}
        disconnecting={disconnecting}
        onConnect={onConnect}
        onDisconnect={onDisconnect}
      />
    </div>
  )
}
