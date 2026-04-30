import "@testing-library/jest-dom/vitest"
import { afterEach } from "vitest"
import { cleanup } from "@testing-library/react"

// Vitest config has `globals: false`, so RTL's auto-cleanup hook is not
// registered. Register it manually so unmount runs between tests and we
// don't leak rendered components / hooks / effects across the suite.
afterEach(() => {
  cleanup()
})
