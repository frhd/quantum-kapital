import { WorkspaceHeader } from "./WorkspaceHeader"
import { WorkspaceTabsNav } from "./WorkspaceTabsNav"
import { OverviewPanel } from "./panels/OverviewPanel"
import { ResearchPanel } from "./panels/ResearchPanel"
import { AlertsPanel } from "./panels/AlertsPanel"
import { WatchlistMetaPanel } from "./panels/WatchlistMetaPanel"
import { NewsPanel } from "./panels/NewsPanel"
import { HistoryPanel } from "./panels/HistoryPanel"
import { PlaceholderPanel } from "./panels/PlaceholderPanel"
import { RecentSymbolChips } from "./RecentSymbolChips"
import { Card, CardContent } from "../../../shared/components/ui/card"
import { useWorkspace } from "../context/WorkspaceContext"

export function WorkspaceTab() {
  const { symbol, tab, nonce, recents } = useWorkspace()
  const panelKey = `${symbol ?? "empty"}#${nonce}`
  const showRecentsSection = !symbol && recents.length > 0

  return (
    <div className="space-y-6">
      <WorkspaceHeader />
      {showRecentsSection && (
        <Card className="border-border bg-card/50">
          <CardContent className="space-y-3 py-4">
            <p className="text-muted-foreground text-xs font-medium tracking-wide uppercase">
              Recent
            </p>
            <RecentSymbolChips />
          </CardContent>
        </Card>
      )}
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
