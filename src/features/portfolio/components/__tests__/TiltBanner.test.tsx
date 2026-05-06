import { describe, it, expect, vi, beforeEach } from "vitest"
import { render, screen, waitFor, fireEvent } from "@testing-library/react"

import { TiltBanner } from "../TiltBanner"
import type { TiltStatus } from "../../../../shared/api/tiltGuard"

vi.mock("../../../../shared/api/tiltGuard", async (orig) => {
  const actual = await orig<typeof import("../../../../shared/api/tiltGuard")>()
  return {
    ...actual,
    tiltGuardStatus: vi.fn(),
    tiltGuardOverride: vi.fn(),
  }
})

vi.mock("@tauri-apps/api/event", () => ({
  listen: vi.fn(async () => () => {}),
}))

import { tiltGuardOverride, tiltGuardStatus } from "../../../../shared/api/tiltGuard"

const pausedStatus: TiltStatus = {
  account: "DU1",
  paused: true,
  episode: {
    id: 7,
    account: "DU1",
    triggered_at: "2026-05-06T14:30:00Z",
    trigger_kind: "two_consecutive_losses",
    cumulative_r: -1.2,
    consecutive_losses: 2,
    auto_reset_at: "2026-05-07T13:30:00Z",
    released_at: null,
    release_kind: null,
    release_reason: null,
  },
  day_threshold_cum_r: -3,
  cumulative_r_today: -1.2,
  closed_trade_count_today: 2,
}

const releasedStatus: TiltStatus = {
  account: "DU1",
  paused: false,
  episode: {
    ...pausedStatus.episode!,
    released_at: "2026-05-06T15:00:00Z",
    release_kind: "manual_override",
    release_reason: "Saw the gap, accept lower size",
  },
  day_threshold_cum_r: -3,
  cumulative_r_today: -1.2,
  closed_trade_count_today: 2,
}

describe("TiltBanner", () => {
  beforeEach(() => {
    vi.clearAllMocks()
  })

  it("renders the paused banner with trigger detail and reset hint", async () => {
    vi.mocked(tiltGuardStatus).mockResolvedValueOnce(pausedStatus)
    render(<TiltBanner />)
    await waitFor(() => {
      expect(screen.getByTestId("tilt-banner-paused")).toBeInTheDocument()
    })
    expect(screen.getByText(/Tilt-paused/)).toBeInTheDocument()
    expect(screen.getByText(/Two consecutive losses/)).toBeInTheDocument()
    expect(screen.getByText(/streak 2/)).toBeInTheDocument()
  })

  it("posts override with the typed reason", async () => {
    vi.mocked(tiltGuardStatus).mockResolvedValueOnce(pausedStatus)
    vi.mocked(tiltGuardOverride).mockResolvedValueOnce(releasedStatus)
    render(<TiltBanner />)
    await waitFor(() => {
      expect(screen.getByTestId("tilt-banner-paused")).toBeInTheDocument()
    })
    fireEvent.click(screen.getByTestId("tilt-banner-override-open"))
    const reasonInput = screen.getByTestId("tilt-override-reason") as HTMLTextAreaElement
    fireEvent.change(reasonInput, { target: { value: "I see the gap" } })
    fireEvent.click(screen.getByTestId("tilt-override-submit"))
    await waitFor(() => {
      expect(tiltGuardOverride).toHaveBeenCalledWith("I see the gap")
    })
  })

  it("renders nothing when no recent tilt history exists", async () => {
    vi.mocked(tiltGuardStatus).mockResolvedValueOnce({
      account: "DU1",
      paused: false,
      episode: null,
      day_threshold_cum_r: -3,
      cumulative_r_today: 0,
      closed_trade_count_today: 0,
    })
    const { container } = render(<TiltBanner />)
    await waitFor(() => {
      expect(container.firstChild).toBeNull()
    })
  })

  it("renders the released pill when paused = false but episode is recent", async () => {
    vi.mocked(tiltGuardStatus).mockResolvedValueOnce(releasedStatus)
    render(<TiltBanner />)
    await waitFor(() => {
      expect(screen.getByTestId("tilt-banner-released")).toBeInTheDocument()
    })
    expect(screen.getByText(/Tilt released/)).toBeInTheDocument()
    expect(screen.getByText(/Manual override/)).toBeInTheDocument()
  })
})
