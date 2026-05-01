import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor, act } from "@testing-library/react"
import { DataTierBanner } from "./DataTierBanner"

const getDataTierMock = vi.fn()
const getConnectionStatusMock = vi.fn()
const listenMock = vi.fn()

vi.mock("../api/ibkr", () => ({
  ibkrApi: {
    getDataTier: () => getDataTierMock(),
    getConnectionStatus: () => getConnectionStatusMock(),
  },
}))

vi.mock("@tauri-apps/api/event", () => ({
  listen: (event: string, handler: unknown) => listenMock(event, handler),
}))

describe("DataTierBanner", () => {
  beforeEach(() => {
    getDataTierMock.mockReset()
    getConnectionStatusMock.mockReset()
    listenMock.mockReset()
    listenMock.mockResolvedValue(() => {})
    getConnectionStatusMock.mockResolvedValue({ connected: true, server_time: null, client_id: 1 })
  })

  it("renders the delayed banner when tier is delayed", async () => {
    getDataTierMock.mockResolvedValue("delayed")
    render(<DataTierBanner />)

    const banner = await screen.findByTestId("data-tier-banner")
    expect(banner).toHaveTextContent("Delayed (15-min)")
  })

  it("renders nothing when tier is real_time", async () => {
    getDataTierMock.mockResolvedValue("real_time")
    render(<DataTierBanner />)

    // Give the hydration a beat to finish.
    await act(async () => {
      await Promise.resolve()
    })
    expect(screen.queryByTestId("data-tier-banner")).toBeNull()
  })

  it("renders the detecting banner when tier is unknown", async () => {
    getDataTierMock.mockResolvedValue("unknown")
    render(<DataTierBanner />)

    const banner = await screen.findByTestId("data-tier-banner")
    expect(banner).toHaveTextContent("Detecting market data tier")
  })

  it("hides the banner while disconnected", async () => {
    getConnectionStatusMock.mockResolvedValue({ connected: false, server_time: null, client_id: 1 })
    getDataTierMock.mockResolvedValue("delayed")
    render(<DataTierBanner />)

    await act(async () => {
      await Promise.resolve()
    })
    expect(screen.queryByTestId("data-tier-banner")).toBeNull()
  })

  it("flips when a data-tier-detected event arrives", async () => {
    let tierHandler: ((event: { payload: unknown }) => void) | null = null
    listenMock.mockImplementation((eventName, handler) => {
      if (eventName === "data-tier-detected") {
        tierHandler = handler as (event: { payload: unknown }) => void
      }
      return Promise.resolve(() => {})
    })
    getDataTierMock.mockResolvedValue("real_time")
    render(<DataTierBanner />)

    await act(async () => {
      await Promise.resolve()
    })
    expect(screen.queryByTestId("data-tier-banner")).toBeNull()

    await waitFor(() => expect(tierHandler).not.toBeNull())
    act(() => {
      tierHandler!({ payload: { type: "DataTierDetected", data: { tier: "delayed" } } })
    })

    const banner = await screen.findByTestId("data-tier-banner")
    expect(banner).toHaveTextContent("Delayed")
  })
})
