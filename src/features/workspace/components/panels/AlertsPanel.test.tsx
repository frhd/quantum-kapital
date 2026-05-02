import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor } from "@testing-library/react"
import { useEffect } from "react"
import { AlertsPanel } from "./AlertsPanel"
import { WorkspaceProvider, useWorkspace } from "../../context/WorkspaceContext"
import type { Alert } from "../../../tracker/types"

const useAlertsMock = vi.fn()

vi.mock("../../../tracker/hooks/useAlerts", () => ({
  useAlerts: (args: unknown) => useAlertsMock(args),
}))

vi.mock("../../../tracker/hooks/useTrackerEvents", () => ({
  useTrackerEvents: () => ({
    recentEvents: [],
    lastSetupDetected: null,
    lastInvalidated: null,
    lastStatusChanged: null,
    lastMorningPackReady: null,
    activeSetupBySymbol: {},
  }),
}))

vi.mock("../../../tracker/components/AlertRow", () => ({
  AlertRow: ({ alert }: { alert: Alert }) => (
    <div data-testid="alert-row">
      alert#{alert.id}:{(alert.payload as { symbol?: string }).symbol}
    </div>
  ),
}))

function SetSymbol({ symbol }: { symbol: string | null }) {
  const { setSymbol } = useWorkspace()
  useEffect(() => {
    setSymbol(symbol)
  }, [setSymbol, symbol])
  return null
}

function renderPanel(symbol: string | null) {
  return render(
    <WorkspaceProvider>
      <SetSymbol symbol={symbol} />
      <AlertsPanel />
    </WorkspaceProvider>,
  )
}

const baseHookReturn = {
  alerts: [] as Alert[],
  loading: false,
  error: null as string | null,
  unseenCount: 0,
  hasMore: false,
  refresh: vi.fn(),
  loadMore: vi.fn(),
  markAllSeen: vi.fn(),
  markOneSeen: vi.fn(),
}

const sampleAlert: Alert = {
  id: 42,
  setup_id: 1,
  kind: "detected",
  fired_at: "2026-05-02T00:00:00Z",
  payload: { symbol: "AAPL" },
  seen: false,
}

describe("AlertsPanel", () => {
  beforeEach(() => {
    useAlertsMock.mockReset()
    useAlertsMock.mockReturnValue({ ...baseHookReturn })
  })

  it("accepts no props — reads from workspace context only", () => {
    expect(AlertsPanel.length).toBe(0)
  })

  it("shows the no-symbol empty state when workspace has no active symbol", () => {
    renderPanel(null)
    expect(screen.getByText(/No symbol selected/)).toBeInTheDocument()
  })

  it("passes the active symbol to useAlerts so the backend filters server-side", async () => {
    renderPanel("MSFT")
    await waitFor(() => {
      expect(useAlertsMock).toHaveBeenCalled()
    })
    const calls = useAlertsMock.mock.calls
    const lastArgs = calls[calls.length - 1]?.[0] as { symbol: string | null }
    expect(lastArgs.symbol).toBe("MSFT")
  })

  it("renders symbol-scoped alert rows", async () => {
    useAlertsMock.mockReturnValue({
      ...baseHookReturn,
      alerts: [sampleAlert],
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByTestId("alert-row")).toHaveTextContent("alert#42:AAPL")
    })
  })

  it("renders the empty state when the symbol has no alerts", async () => {
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText(/No alerts for AAPL yet/)).toBeInTheDocument()
    })
  })

  it("renders the error banner when useAlerts surfaces an error", async () => {
    useAlertsMock.mockReturnValue({
      ...baseHookReturn,
      error: "boom",
    })
    renderPanel("AAPL")
    await waitFor(() => {
      expect(screen.getByText("boom")).toBeInTheDocument()
    })
  })
})
