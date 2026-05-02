import { mockIPC } from "@tauri-apps/api/mocks"
import { fixtures } from "./fixtures"

mockIPC(
  (cmd, args) => {
    const fixture = fixtures[cmd]
    if (!fixture) {
      console.warn(`[browser-mocks] no fixture for "${cmd}"; returning null`, args)
      return null
    }
    return fixture((args as Record<string, unknown>) ?? {})
  },
  { shouldMockEvents: true },
)

console.info(
  "[browser-mocks] window.__TAURI_INTERNALS__ shimmed; running outside Tauri (VITE_BROWSER_DEV=1)",
)
