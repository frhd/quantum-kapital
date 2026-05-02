import { describe, it, expect, beforeEach } from "vitest"
import { act, renderHook } from "@testing-library/react"
import {
  RECENT_SYMBOLS_CAPACITY,
  RECENT_SYMBOLS_STORAGE_KEY,
  useRecentSymbols,
} from "./useRecentSymbols"

describe("useRecentSymbols", () => {
  beforeEach(() => {
    localStorage.clear()
  })

  it("starts empty when localStorage has no entry", () => {
    const { result } = renderHook(() => useRecentSymbols())
    expect(result.current.recents).toEqual([])
  })

  it("uppercases on push and prepends new entries", () => {
    const { result } = renderHook(() => useRecentSymbols())

    act(() => result.current.push("aapl"))
    expect(result.current.recents).toEqual(["AAPL"])

    act(() => result.current.push("msft"))
    expect(result.current.recents).toEqual(["MSFT", "AAPL"])
  })

  it("moves a duplicate to the front instead of appending", () => {
    const { result } = renderHook(() => useRecentSymbols())

    act(() => result.current.push("AAPL"))
    act(() => result.current.push("MSFT"))
    act(() => result.current.push("TSLA"))
    act(() => result.current.push("MSFT"))

    expect(result.current.recents).toEqual(["MSFT", "TSLA", "AAPL"])
  })

  it("ignores empty / whitespace pushes", () => {
    const { result } = renderHook(() => useRecentSymbols())

    act(() => result.current.push(""))
    act(() => result.current.push("   "))

    expect(result.current.recents).toEqual([])
  })

  it("caps the list at CAPACITY entries (most recent first)", () => {
    const { result } = renderHook(() => useRecentSymbols())

    const symbols = Array.from({ length: RECENT_SYMBOLS_CAPACITY + 5 }, (_, i) => `SYM${i}`)
    act(() => {
      for (const s of symbols) result.current.push(s)
    })

    expect(result.current.recents).toHaveLength(RECENT_SYMBOLS_CAPACITY)
    expect(result.current.recents[0]).toBe(symbols[symbols.length - 1])
    // The earliest pushes have been evicted.
    expect(result.current.recents).not.toContain("SYM0")
  })

  it("persists writes to localStorage and rehydrates on a fresh hook instance", () => {
    const first = renderHook(() => useRecentSymbols())
    act(() => first.result.current.push("AAPL"))
    act(() => first.result.current.push("MSFT"))

    const stored = localStorage.getItem(RECENT_SYMBOLS_STORAGE_KEY)
    expect(stored).not.toBeNull()
    expect(JSON.parse(stored ?? "[]")).toEqual(["MSFT", "AAPL"])

    first.unmount()

    const second = renderHook(() => useRecentSymbols())
    expect(second.result.current.recents).toEqual(["MSFT", "AAPL"])
  })

  it("clear() empties the list and persists the empty state", () => {
    const { result, unmount } = renderHook(() => useRecentSymbols())

    act(() => result.current.push("AAPL"))
    act(() => result.current.clear())

    expect(result.current.recents).toEqual([])
    expect(JSON.parse(localStorage.getItem(RECENT_SYMBOLS_STORAGE_KEY) ?? "null")).toEqual([])

    unmount()
    const second = renderHook(() => useRecentSymbols())
    expect(second.result.current.recents).toEqual([])
  })

  it("ignores corrupt localStorage payloads", () => {
    localStorage.setItem(RECENT_SYMBOLS_STORAGE_KEY, "not json {")
    const { result } = renderHook(() => useRecentSymbols())
    expect(result.current.recents).toEqual([])
  })

  it("filters non-string entries from a stored array", () => {
    localStorage.setItem(RECENT_SYMBOLS_STORAGE_KEY, JSON.stringify(["AAPL", 42, null, "MSFT"]))
    const { result } = renderHook(() => useRecentSymbols())
    expect(result.current.recents).toEqual(["AAPL", "MSFT"])
  })
})
