import { WorkspaceHeader } from "./WorkspaceHeader"
import { WorkspaceTabsNav } from "./WorkspaceTabsNav"
import { OverviewPanel } from "./panels/OverviewPanel"
import { PlaceholderPanel } from "./panels/PlaceholderPanel"
import { useWorkspace } from "../context/WorkspaceContext"
import { WORKSPACE_TAB_LABELS, type WorkspaceTabId } from "../types"

const PLACEHOLDER_PHASE: Partial<Record<WorkspaceTabId, number>> = {
  research: 2,
  alerts: 2,
  watchlist: 2,
  news: 3,
  history: 3,
}

export function WorkspaceTab() {
  const { symbol, tab, nonce } = useWorkspace()
  const panelKey = `${symbol ?? "empty"}#${nonce}`

  return (
    <div className="space-y-6">
      <WorkspaceHeader />
      <WorkspaceTabsNav />
      <div key={panelKey}>
        {tab === "overview" ? (
          <OverviewPanel />
        ) : (
          <PlaceholderPanel label={WORKSPACE_TAB_LABELS[tab]} phase={PLACEHOLDER_PHASE[tab]} />
        )}
      </div>
    </div>
  )
}
