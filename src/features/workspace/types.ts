export type WorkspaceTabId =
  | "overview"
  | "projection"
  | "news"
  | "research"
  | "alerts"
  | "history"
  | "watchlist"

export const WORKSPACE_TAB_ORDER: WorkspaceTabId[] = [
  "overview",
  "projection",
  "news",
  "research",
  "alerts",
  "history",
  "watchlist",
]

export const WORKSPACE_TAB_LABELS: Record<WorkspaceTabId, string> = {
  overview: "Overview",
  projection: "Projection",
  news: "News",
  research: "Research",
  alerts: "Alerts",
  history: "History",
  watchlist: "Watchlist",
}
