import { ConnectionStatus } from "../../../features/connection/components/ConnectionStatus"
import type { ConnectionStatus as ConnectionStatusType } from "../../types"

interface PageHeaderProps {
  connectionStatus: ConnectionStatusType
  loading: boolean
  onConnect: () => void
  onDisconnect: () => void
}

export function PageHeader({ connectionStatus, loading, onConnect, onDisconnect }: PageHeaderProps) {
  return (
    <div className="flex items-center justify-between">
      <div>
        <h1 className="text-4xl font-bold bg-gradient-to-r from-blue-400 to-purple-400 bg-clip-text text-transparent">
          IBKR Portfolio Dashboard
        </h1>
        <p className="text-slate-400 mt-1">Interactive Brokers API Integration</p>
      </div>
      <ConnectionStatus
        connectionStatus={connectionStatus}
        loading={loading}
        onConnect={onConnect}
        onDisconnect={onDisconnect}
      />
    </div>
  )
}