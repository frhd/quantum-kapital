import { describe, it, expect, vi } from "vitest"
import { renderHook, act } from "@testing-library/react"
import { createElement, type ReactNode } from "react"
import { WorkspaceProvider, useWorkspace } from "../context/WorkspaceContext"
import { useTickerNavigate } from "./useTickerNavigate"

function makeWrapper(onNavigatePage?: () => void) {
  const Wrapper = ({ children }: { children: ReactNode }) =>
    createElement(WorkspaceProvider, { onNavigatePage, children })
  return Wrapper
}

describe("useTickerNavigate", () => {
  it("sets context symbol (uppercased) on first call", () => {
    const { result } = renderHook(
      () => ({ navigate: useTickerNavigate(), state: useWorkspace() }),
      { wrapper: makeWrapper() },
    )

    expect(result.current.state.symbol).toBeNull()
    expect(result.current.state.tab).toBe("overview")

    act(() => result.current.navigate("aapl"))

    expect(result.current.state.symbol).toBe("AAPL")
    expect(result.current.state.tab).toBe("overview")
  })

  it("increments nonce on repeat-call with the same symbol", () => {
    const { result } = renderHook(
      () => ({ navigate: useTickerNavigate(), state: useWorkspace() }),
      { wrapper: makeWrapper() },
    )

    act(() => result.current.navigate("AAPL"))
    const firstNonce = result.current.state.nonce

    act(() => result.current.navigate("AAPL"))
    expect(result.current.state.nonce).toBeGreaterThan(firstNonce)
    expect(result.current.state.symbol).toBe("AAPL")
  })

  it("honors an explicit tab override", () => {
    const { result } = renderHook(
      () => ({ navigate: useTickerNavigate(), state: useWorkspace() }),
      { wrapper: makeWrapper() },
    )

    act(() => result.current.navigate("MSFT", "alerts"))

    expect(result.current.state.symbol).toBe("MSFT")
    expect(result.current.state.tab).toBe("alerts")
  })

  it("invokes onNavigatePage so the host page can switch to the workspace", () => {
    const onNavigatePage = vi.fn()
    const { result } = renderHook(() => useTickerNavigate(), {
      wrapper: makeWrapper(onNavigatePage),
    })

    act(() => result.current("TSLA"))
    act(() => result.current("TSLA"))

    expect(onNavigatePage).toHaveBeenCalledTimes(2)
  })
})
