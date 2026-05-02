import { createContext, useContext, type ReactNode } from "react"
import type { AddToTrackerPrefill } from "../../tracker/types"

type OpenAddToTrackerFn = (prefill: AddToTrackerPrefill) => void

const AddToTrackerContext = createContext<OpenAddToTrackerFn | null>(null)

interface AddToTrackerProviderProps {
  children: ReactNode
  open: OpenAddToTrackerFn
}

/**
 * Workspace Phase 2 — exposes the app-level "Add to tracker" dialog
 * opener through context so any deeply-mounted panel can request it
 * without a prop chain. Replaces the per-feature `onAddClick` that
 * Phase 4 will need from many sites.
 */
export function AddToTrackerProvider({ children, open }: AddToTrackerProviderProps) {
  return <AddToTrackerContext.Provider value={open}>{children}</AddToTrackerContext.Provider>
}

/**
 * Returns the opener, or `null` when no provider is mounted (e.g. in
 * a unit test that doesn't need the dialog). Callers gate on the
 * value before invoking.
 */
export function useAddToTrackerOpen(): OpenAddToTrackerFn | null {
  return useContext(AddToTrackerContext)
}
