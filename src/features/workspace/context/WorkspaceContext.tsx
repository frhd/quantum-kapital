import {
  createContext,
  useCallback,
  useContext,
  useEffect,
  useMemo,
  useRef,
  useState,
  type ReactNode,
} from "react"
import type { WorkspaceTabId } from "../types"

interface WorkspaceState {
  symbol: string | null
  tab: WorkspaceTabId
  /** Bumps every time `navigate` is called, even with the same symbol — drives re-fetch effects. */
  nonce: number
}

interface WorkspaceContextValue extends WorkspaceState {
  setSymbol: (symbol: string | null) => void
  setTab: (tab: WorkspaceTabId) => void
  navigate: (symbol: string, tab?: WorkspaceTabId) => void
  /** Optional page-router hook so navigate can flip the host page to "ticker". */
  onNavigatePage?: () => void
}

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null)

interface WorkspaceProviderProps {
  children: ReactNode
  initialTab?: WorkspaceTabId
  /** Called from `navigate` so the workspace page becomes active app-wide. */
  onNavigatePage?: () => void
}

export function WorkspaceProvider({
  children,
  initialTab = "overview",
  onNavigatePage,
}: WorkspaceProviderProps) {
  const [symbol, setSymbolState] = useState<string | null>(null)
  const [tab, setTabState] = useState<WorkspaceTabId>(initialTab)
  const [nonce, setNonce] = useState(0)
  const nonceRef = useRef(0)

  const onNavigatePageRef = useRef(onNavigatePage)
  useEffect(() => {
    onNavigatePageRef.current = onNavigatePage
  }, [onNavigatePage])

  const setSymbol = useCallback((next: string | null) => {
    setSymbolState(next ? next.toUpperCase() : null)
  }, [])

  const setTab = useCallback((next: WorkspaceTabId) => {
    setTabState(next)
  }, [])

  const navigate = useCallback((nextSymbol: string, nextTab?: WorkspaceTabId) => {
    nonceRef.current += 1
    setNonce(nonceRef.current)
    setSymbolState(nextSymbol.toUpperCase())
    if (nextTab) {
      setTabState(nextTab)
    }
    onNavigatePageRef.current?.()
  }, [])

  const value = useMemo<WorkspaceContextValue>(
    () => ({ symbol, tab, nonce, setSymbol, setTab, navigate, onNavigatePage }),
    [symbol, tab, nonce, setSymbol, setTab, navigate, onNavigatePage],
  )

  return <WorkspaceContext.Provider value={value}>{children}</WorkspaceContext.Provider>
}

export function useWorkspace(): WorkspaceContextValue {
  const ctx = useContext(WorkspaceContext)
  if (!ctx) {
    throw new Error("useWorkspace must be used within a WorkspaceProvider")
  }
  return ctx
}
