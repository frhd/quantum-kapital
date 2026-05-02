# browser-mocks

Vite-side shim that lets the React app run in a plain browser (no Tauri shell)
by routing every `invoke()` call through `mockIPC` from `@tauri-apps/api/mocks`.

## When this loads

Only when `VITE_BROWSER_DEV=1` is set, which the `pnpm dev:browser` script does.
`pnpm tauri dev` and `pnpm tauri build` never load this code.

## When to add a fixture

Add (or update) an entry in `fixtures.ts` only when a screen you are designing
or debugging renders blank / errors against the shim. Keep fixtures realistic
enough that layouts render with believable content (real shapes, plausible
numbers) — but resist the urge to grow this into a backend simulator. Unknown
commands log a warning and return `null`; that is the intended degraded state.

If a fixture's shape drifts from the Rust backend, the screen will look wrong.
Refresh the fixture; do not patch the screen.

## Why `mockIPC`

It is the official Tauri 2 helper for replacing `window.__TAURI_INTERNALS__`,
including the `shouldMockEvents` flag that makes `listen()` work without a real
backend. Rolling our own shim would duplicate code that Tauri already maintains.
