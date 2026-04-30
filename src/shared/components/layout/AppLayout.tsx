import { type ReactNode, useState } from "react"
import { Sidebar, type PageId } from "./Sidebar"
import { StatusBar } from "./StatusBar"
import type { ConnectionStatus as ConnectionStatusType } from "../../types"

interface AppLayoutProps {
  currentPage: PageId
  onNavigate: (page: PageId) => void
  connectionStatus: ConnectionStatusType
  loading: boolean
  disconnecting: boolean
  onConnect: () => void
  onDisconnect: () => void
  badges?: Partial<Record<PageId, number>>
  children: ReactNode
}

export function AppLayout({
  currentPage,
  onNavigate,
  connectionStatus,
  loading,
  disconnecting,
  onConnect,
  onDisconnect,
  badges,
  children,
}: AppLayoutProps) {
  const [expanded, setExpanded] = useState(true)

  return (
    <div className="bg-background text-foreground flex h-screen flex-col">
      <div className="flex flex-1 overflow-hidden">
        <Sidebar
          currentPage={currentPage}
          onNavigate={onNavigate}
          expanded={expanded}
          onToggleExpand={() => setExpanded((v) => !v)}
          badges={badges}
        />
        <main className="flex-1 overflow-y-auto p-6">{children}</main>
      </div>
      <StatusBar
        currentPage={currentPage}
        connectionStatus={connectionStatus}
        loading={loading}
        disconnecting={disconnecting}
        onConnect={onConnect}
        onDisconnect={onDisconnect}
      />
    </div>
  )
}
