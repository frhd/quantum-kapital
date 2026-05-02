/// <reference types="node" />
import { describe, it, expect } from "vitest"
import { readFileSync, readdirSync, statSync } from "fs"
import { fileURLToPath } from "url"
import { dirname, join } from "path"

const HERE = dirname(fileURLToPath(import.meta.url))
const WORKSPACE_ROOT = HERE

function walk(dir: string, acc: string[] = []): string[] {
  for (const entry of readdirSync(dir)) {
    const full = join(dir, entry)
    const stat = statSync(full)
    if (stat.isDirectory()) {
      walk(full, acc)
    } else if (/\.(ts|tsx)$/.test(entry)) {
      acc.push(full)
    }
  }
  return acc
}

const SOURCE_FILES = walk(WORKSPACE_ROOT).filter((p) => !/\.test\.tsx?$/.test(p))

/**
 * Hard invariants (master plan §Hard invariants):
 *   1. surveillance-only — no order-placement surface in `features/workspace/**`.
 *   4. no direct Tauri access — all backend reads must go through `shared/api/*.ts`.
 */
describe("workspace containment", () => {
  it("includes at least one workspace source file (sanity)", () => {
    expect(SOURCE_FILES.length).toBeGreaterThan(0)
  })

  it("never imports @tauri-apps/api/core or calls invoke()", () => {
    const offenders: string[] = []
    for (const path of SOURCE_FILES) {
      const body = readFileSync(path, "utf8")
      if (/@tauri-apps\/api\/core/.test(body))
        offenders.push(`${path}: imports @tauri-apps/api/core`)
      if (/\binvoke\s*\(/.test(body)) offenders.push(`${path}: calls invoke(...)`)
    }
    expect(offenders, offenders.join("\n")).toEqual([])
  })

  it("contains no order-placement surface (place_order, placeOrder, orderRef, new Order)", () => {
    const offenders: string[] = []
    for (const path of SOURCE_FILES) {
      const body = readFileSync(path, "utf8")
      if (/place_order|placeOrder|orderRef|new\s+Order\b/.test(body)) offenders.push(path)
    }
    expect(offenders, offenders.join("\n")).toEqual([])
  })

  it("does not import any Alpha Vantage adapter or hard-code an AV URL", () => {
    // Workspace Phase 3 (master plan §Open risks): the News panel reads
    // `news_cache` only — never an AV adapter. The producer is upstream
    // of this surface and the AV→IBKR migration in `loop/plan/` should
    // be invisible to the workspace.
    const offenders: string[] = []
    for (const path of SOURCE_FILES) {
      const body = readFileSync(path, "utf8")
      if (
        /alpha[\s_-]?vantage/i.test(body) ||
        /alphavantage\.co/i.test(body) ||
        /\bAlphaVantage\w*/.test(body) ||
        /\bAvAdapter\b/.test(body)
      ) {
        offenders.push(path)
      }
    }
    expect(offenders, offenders.join("\n")).toEqual([])
  })
})
