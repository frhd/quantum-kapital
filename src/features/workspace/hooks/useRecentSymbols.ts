import { useCallback, useEffect, useRef, useState } from "react"

const STORAGE_KEY = "qk:workspace:recent"
const CAPACITY = 10

function readFromStorage(): string[] {
  if (typeof localStorage === "undefined") return []
  try {
    const raw = localStorage.getItem(STORAGE_KEY)
    if (!raw) return []
    const parsed: unknown = JSON.parse(raw)
    if (!Array.isArray(parsed)) return []
    return parsed.filter((x): x is string => typeof x === "string").slice(0, CAPACITY)
  } catch {
    return []
  }
}

function writeToStorage(list: string[]): void {
  if (typeof localStorage === "undefined") return
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(list))
  } catch {
    // Quota exceeded or storage disabled — silently drop persistence.
  }
}

export interface UseRecentSymbolsResult {
  recents: string[]
  push: (symbol: string) => void
  clear: () => void
}

/**
 * localStorage-backed list of recently viewed symbols.
 * - capped at {@link CAPACITY} (10)
 * - move-to-front on duplicate push, never accumulates duplicates
 * - hydrated once from `qk:workspace:recent`; persisted on every change
 *
 * Persists across reloads. The active symbol is deliberately NOT
 * persisted — the master plan defaults to "fresh start on cold open".
 */
export function useRecentSymbols(): UseRecentSymbolsResult {
  const [recents, setRecents] = useState<string[]>(() => readFromStorage())
  const hydrated = useRef(false)

  useEffect(() => {
    if (!hydrated.current) {
      hydrated.current = true
      return
    }
    writeToStorage(recents)
  }, [recents])

  const push = useCallback((symbol: string) => {
    const upper = symbol.trim().toUpperCase()
    if (!upper) return
    setRecents((prev) => {
      const without = prev.filter((s) => s !== upper)
      return [upper, ...without].slice(0, CAPACITY)
    })
  }, [])

  const clear = useCallback(() => {
    setRecents([])
  }, [])

  return { recents, push, clear }
}

export const RECENT_SYMBOLS_STORAGE_KEY = STORAGE_KEY
export const RECENT_SYMBOLS_CAPACITY = CAPACITY
