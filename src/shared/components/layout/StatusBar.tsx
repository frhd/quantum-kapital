import { Wifi, WifiOff } from "lucide-react"
import { cn } from "../../lib/utils"
import type { ConnectionStatus as ConnectionStatusType } from "../../types"
import { PAGE_LABELS, type PageId } from "./Sidebar"

interface StatusBarProps {
  currentPage: PageId
  connectionStatus: ConnectionStatusType
  loading: boolean
  disconnecting: boolean
  onConnect: () => void
  onDisconnect: () => void
}

export function StatusBar({
  currentPage,
  connectionStatus,
  loading,
  disconnecting,
  onConnect,
  onDisconnect,
}: StatusBarProps) {
  const connected = connectionStatus.connected
  return (
    <footer
      role="status"
      aria-label="Application status"
      className="border-border bg-card text-muted-foreground flex h-7 shrink-0 items-center justify-between border-t px-3 text-xs"
    >
      <div className="flex items-center gap-2 font-medium tracking-wide uppercase">
        <span>{PAGE_LABELS[currentPage]}</span>
      </div>

      <div className="flex items-center gap-3">
        <div className="flex items-center gap-1.5">
          <span
            aria-hidden="true"
            className={cn(
              "h-1.5 w-1.5 rounded-full",
              connected ? "bg-emerald-400" : "bg-destructive",
            )}
          />
          <span
            className={cn(
              "font-mono text-[11px] tabular-nums",
              connected ? "text-emerald-400" : "text-destructive",
            )}
          >
            {connected ? "CONNECTED" : "DISCONNECTED"}
          </span>
        </div>

        {connected ? (
          <button
            type="button"
            onClick={onDisconnect}
            disabled={disconnecting}
            className="border-border bg-background hover:bg-secondary flex items-center gap-1 rounded-sm border px-2 py-0.5 text-[11px] disabled:opacity-50"
          >
            <WifiOff className="h-3 w-3" />
            {disconnecting ? "Disconnecting…" : "Disconnect"}
          </button>
        ) : (
          <button
            type="button"
            onClick={onConnect}
            disabled={loading}
            className="border-primary/40 bg-primary/10 text-primary hover:bg-primary/20 flex items-center gap-1 rounded-sm border px-2 py-0.5 text-[11px] disabled:opacity-50"
          >
            <Wifi className="h-3 w-3" />
            {loading ? "Connecting…" : "Connect"}
          </button>
        )}
      </div>
    </footer>
  )
}
