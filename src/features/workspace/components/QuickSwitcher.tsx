import { useCallback, useEffect, useMemo, useRef, useState } from "react"
import { ibkrApi } from "../../../shared/api/ibkr"
import { useWatchlist } from "../../tracker/hooks/useWatchlist"
import { useWorkspace } from "../context/WorkspaceContext"
import { useTickerNavigate } from "../hooks/useTickerNavigate"

const MAX_RESULTS = 50

interface SwitcherEntry {
  symbol: string
  /** What populated this entry — drives the small badge next to each row. */
  source: "recent" | "watchlist" | "cached"
}

const SOURCE_LABEL: Record<SwitcherEntry["source"], string> = {
  recent: "recent",
  watchlist: "watchlist",
  cached: "cached",
}

/**
 * Phase 5 — global Cmd/Ctrl+K palette. Mounted once at the
 * `WorkspaceProvider` boundary so the keyboard shortcut is live on
 * every page. Data hooks (`useWatchlist`, `getCachedTickers`) are gated
 * behind `open` to honor the master plan's lazy-mount invariant.
 */
export function QuickSwitcher() {
  const [open, setOpen] = useState(false)
  const lastFocused = useRef<HTMLElement | null>(null)

  useEffect(() => {
    const handler = (e: KeyboardEvent) => {
      if ((e.metaKey || e.ctrlKey) && (e.key === "k" || e.key === "K")) {
        e.preventDefault()
        setOpen((prev) => !prev)
      }
    }
    window.addEventListener("keydown", handler)
    return () => window.removeEventListener("keydown", handler)
  }, [])

  const handleOpen = useCallback(() => {
    lastFocused.current = (document.activeElement as HTMLElement | null) ?? null
  }, [])

  const handleClose = useCallback(() => {
    setOpen(false)
    const el = lastFocused.current
    if (el && typeof el.focus === "function") el.focus()
  }, [])

  useEffect(() => {
    if (open) handleOpen()
  }, [open, handleOpen])

  if (!open) return null
  return <QuickSwitcherPanel onClose={handleClose} />
}

interface QuickSwitcherPanelProps {
  onClose: () => void
}

function QuickSwitcherPanel({ onClose }: QuickSwitcherPanelProps) {
  const navigate = useTickerNavigate()
  const { recents } = useWorkspace()
  const { tickers } = useWatchlist()
  const [cached, setCached] = useState<string[]>([])
  const [query, setQuery] = useState("")
  const [highlight, setHighlight] = useState(0)
  const inputRef = useRef<HTMLInputElement | null>(null)
  const listRef = useRef<HTMLUListElement | null>(null)

  useEffect(() => {
    let cancelled = false
    void ibkrApi.getCachedTickers().then(
      (list) => {
        if (!cancelled) setCached(list)
      },
      () => {
        if (!cancelled) setCached([])
      },
    )
    return () => {
      cancelled = true
    }
  }, [])

  useEffect(() => {
    inputRef.current?.focus()
  }, [])

  const universe = useMemo<SwitcherEntry[]>(() => {
    const seen = new Set<string>()
    const out: SwitcherEntry[] = []
    const add = (sym: string, source: SwitcherEntry["source"]) => {
      const upper = sym.toUpperCase()
      if (!upper || seen.has(upper)) return
      seen.add(upper)
      out.push({ symbol: upper, source })
    }
    for (const s of recents) add(s, "recent")
    for (const t of tickers) add(t.symbol, "watchlist")
    for (const s of cached) add(s, "cached")
    return out
  }, [recents, tickers, cached])

  const results = useMemo(() => {
    const q = query.trim().toUpperCase()
    const filtered = q ? universe.filter((entry) => entry.symbol.includes(q)) : universe
    return filtered.slice(0, MAX_RESULTS)
  }, [query, universe])

  useEffect(() => {
    setHighlight((h) => (results.length === 0 ? 0 : Math.min(h, results.length - 1)))
  }, [results.length])

  const commit = useCallback(
    (entry: SwitcherEntry | undefined) => {
      if (!entry) return
      navigate(entry.symbol)
      onClose()
    },
    [navigate, onClose],
  )

  const handleKeyDown = (e: React.KeyboardEvent<HTMLDivElement>) => {
    if (e.key === "Escape") {
      e.preventDefault()
      onClose()
      return
    }
    if (e.key === "ArrowDown") {
      e.preventDefault()
      setHighlight((h) => (results.length === 0 ? 0 : (h + 1) % results.length))
      return
    }
    if (e.key === "ArrowUp") {
      e.preventDefault()
      setHighlight((h) => (results.length === 0 ? 0 : (h - 1 + results.length) % results.length))
      return
    }
    if (e.key === "Enter") {
      e.preventDefault()
      commit(results[highlight])
    }
  }

  return (
    <div
      className="bg-background/70 fixed inset-0 z-[100] flex items-start justify-center pt-24"
      role="dialog"
      aria-modal="true"
      aria-label="Quick symbol switcher"
      onKeyDown={handleKeyDown}
    >
      <div className="bg-background/95 fixed inset-0" aria-hidden="true" onClick={onClose} />
      <div className="border-border bg-card relative z-10 w-[520px] max-w-[90vw] overflow-hidden rounded-md border shadow-2xl">
        <input
          ref={inputRef}
          type="text"
          value={query}
          onChange={(e) => setQuery(e.target.value)}
          placeholder="Type a ticker — ↑↓ to move, Enter to open"
          aria-label="Symbol search"
          className="text-foreground placeholder:text-muted-foreground border-border w-full border-b bg-transparent px-4 py-3 font-mono text-sm focus:outline-none"
          data-testid="quick-switcher-input"
        />
        <ul
          ref={listRef}
          role="listbox"
          aria-label="Symbol results"
          className="max-h-80 overflow-y-auto py-1"
          data-testid="quick-switcher-list"
        >
          {results.length === 0 ? (
            <li className="text-muted-foreground px-4 py-3 text-xs">
              {query ? `No matches for "${query}"` : "Start typing to find a ticker."}
            </li>
          ) : (
            results.map((entry, i) => (
              <li
                key={entry.symbol}
                role="option"
                aria-selected={i === highlight}
                data-testid={`quick-switcher-row-${entry.symbol}`}
                onMouseEnter={() => setHighlight(i)}
                onMouseDown={(e) => {
                  e.preventDefault()
                  commit(entry)
                }}
                className={[
                  "flex cursor-pointer items-center justify-between px-4 py-2 font-mono text-sm",
                  i === highlight ? "bg-accent text-accent-foreground" : "text-foreground",
                ].join(" ")}
              >
                <span>{entry.symbol}</span>
                <span className="text-muted-foreground text-[10px] uppercase">
                  {SOURCE_LABEL[entry.source]}
                </span>
              </li>
            ))
          )}
        </ul>
      </div>
    </div>
  )
}
