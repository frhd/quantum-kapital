import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, act } from "@testing-library/react"
import { useEffect } from "react"
import { WorkspaceProvider, useWorkspace } from "../context/WorkspaceContext"
import { useTickerNavigate } from "../hooks/useTickerNavigate"
import { WorkspaceTab } from "./WorkspaceTab"
import type { WorkspaceTabId } from "../types"

const overviewMountSpy = vi.fn()
const overviewUnmountSpy = vi.fn()

vi.mock("./WorkspaceHeader", () => ({
  WorkspaceHeader: () => <div data-testid="header" />,
}))

vi.mock("./panels/OverviewPanel", () => ({
  OverviewPanel: () => {
    useEffect(() => {
      overviewMountSpy()
      return () => overviewUnmountSpy()
    }, [])
    const { symbol } = useWorkspace()
    return <div data-testid="overview-panel">overview:{symbol ?? "none"}</div>
  },
}))

vi.mock("./panels/ResearchPanel", () => ({
  ResearchPanel: () => <div data-testid="research-panel" />,
}))

vi.mock("./panels/AlertsPanel", () => ({
  AlertsPanel: () => <div data-testid="alerts-panel" />,
}))

vi.mock("./panels/WatchlistMetaPanel", () => ({
  WatchlistMetaPanel: () => <div data-testid="watchlist-meta-panel" />,
}))

interface DriverProps {
  navigateTo?: { symbol: string; tab?: WorkspaceTabId }
  setTabTo?: WorkspaceTabId
}

function Driver({ navigateTo, setTabTo }: DriverProps) {
  const navigate = useTickerNavigate()
  const { setTab } = useWorkspace()
  useEffect(() => {
    if (navigateTo) navigate(navigateTo.symbol, navigateTo.tab)
  }, [navigateTo, navigate])
  useEffect(() => {
    if (setTabTo) setTab(setTabTo)
  }, [setTabTo, setTab])
  return null
}

describe("WorkspaceTab", () => {
  beforeEach(() => {
    overviewMountSpy.mockReset()
    overviewUnmountSpy.mockReset()
  })

  it("renders the Overview panel by default", () => {
    render(
      <WorkspaceProvider>
        <WorkspaceTab />
      </WorkspaceProvider>,
    )

    expect(screen.getByTestId("header")).toBeInTheDocument()
    expect(screen.getByTestId("overview-panel")).toHaveTextContent("overview:none")
  })

  it("switches the active panel when navigate(symbol, tab) is called", () => {
    const { rerender } = render(
      <WorkspaceProvider>
        <Driver />
        <WorkspaceTab />
      </WorkspaceProvider>,
    )

    expect(screen.getByTestId("overview-panel")).toBeInTheDocument()

    rerender(
      <WorkspaceProvider>
        <Driver navigateTo={{ symbol: "AAPL", tab: "alerts" }} />
        <WorkspaceTab />
      </WorkspaceProvider>,
    )

    // Alerts panel rendered, overview unmounted
    expect(screen.queryByTestId("overview-panel")).toBeNull()
    expect(screen.getByTestId("alerts-panel")).toBeInTheDocument()
  })

  it("unmounts the active panel when switching tabs (lazy panel mount invariant)", () => {
    const setTabRef: { fn: ((tab: WorkspaceTabId) => void) | null } = { fn: null }
    function CaptureSetTab() {
      const { setTab } = useWorkspace()
      useEffect(() => {
        setTabRef.fn = setTab
      }, [setTab])
      return null
    }

    render(
      <WorkspaceProvider>
        <CaptureSetTab />
        <WorkspaceTab />
      </WorkspaceProvider>,
    )

    expect(overviewMountSpy).toHaveBeenCalledTimes(1)
    expect(overviewUnmountSpy).not.toHaveBeenCalled()

    act(() => {
      setTabRef.fn!("research")
    })

    expect(overviewUnmountSpy).toHaveBeenCalledTimes(1)
    expect(screen.queryByTestId("overview-panel")).toBeNull()
  })

  it("re-mounts the Overview panel when navigate is called twice with the same symbol (nonce semantics)", () => {
    const navRef: { fn: ((s: string, t?: WorkspaceTabId) => void) | null } = { fn: null }
    function CaptureNav() {
      const navigate = useTickerNavigate()
      useEffect(() => {
        navRef.fn = navigate
      }, [navigate])
      return null
    }

    render(
      <WorkspaceProvider>
        <CaptureNav />
        <WorkspaceTab />
      </WorkspaceProvider>,
    )

    expect(overviewMountSpy).toHaveBeenCalledTimes(1)

    act(() => navRef.fn!("AAPL"))
    expect(overviewMountSpy).toHaveBeenCalledTimes(2)
    expect(overviewUnmountSpy).toHaveBeenCalledTimes(1)

    act(() => navRef.fn!("AAPL"))
    expect(overviewMountSpy).toHaveBeenCalledTimes(3)
    expect(overviewUnmountSpy).toHaveBeenCalledTimes(2)
  })
})
