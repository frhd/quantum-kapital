import { WorkspaceHeader } from "./WorkspaceHeader"
import { WorkspaceTabsNav } from "./WorkspaceTabsNav"
import { OverviewPanel } from "./panels/OverviewPanel"
import { ResearchPanel } from "./panels/ResearchPanel"
import { AlertsPanel } from "./panels/AlertsPanel"
import { WatchlistMetaPanel } from "./panels/WatchlistMetaPanel"
import { NewsPanel } from "./panels/NewsPanel"
import { HistoryPanel } from "./panels/HistoryPanel"
import { PlaceholderPanel } from "./panels/PlaceholderPanel"
import { useWorkspace } from "../context/WorkspaceContext"

export function WorkspaceTab() {
  const { symbol, tab, nonce } = useWorkspace()
  const panelKey = `${symbol ?? "empty"}#${nonce}`

  return (
    <div className="space-y-6">
      <WorkspaceHeader />
      <WorkspaceTabsNav />
      <div key={panelKey}>
        {tab === "overview" && <OverviewPanel />}
        {tab === "research" && <ResearchPanel />}
        {tab === "alerts" && <AlertsPanel />}
        {tab === "watchlist" && <WatchlistMetaPanel />}
        {tab === "news" && <NewsPanel />}
        {tab === "history" && <HistoryPanel />}
        {tab === "projection" && <PlaceholderPanel label="Projection" />}
      </div>
    </div>
  )
}
