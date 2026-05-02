import { useCallback } from "react"
import { useWorkspace } from "../context/WorkspaceContext"
import type { WorkspaceTabId } from "../types"

/**
 * Sole sanctioned primitive for routing into the workspace from any list view.
 * Callers pass a symbol and an optional default tab; the hook bumps the nonce
 * (so re-clicking the same symbol re-runs symbol-side fetches) and triggers
 * the host page to swap to the workspace.
 */
export function useTickerNavigate() {
  const { navigate } = useWorkspace()
  return useCallback(
    (symbol: string, tab?: WorkspaceTabId) => {
      navigate(symbol, tab)
    },
    [navigate],
  )
}
