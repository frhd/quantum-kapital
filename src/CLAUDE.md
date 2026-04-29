# Frontend (src)

React 19 + TypeScript + Tailwind 4 on Vite. Cross-cutting rules in `../CLAUDE.md`.

## Common commands

```bash
pnpm tauri dev    # full app (Vite + Rust backend, hot reload)
pnpm dev          # frontend only, no Tauri shell
pnpm tauri build  # production binaries → src-tauri/target/release/
pnpm typecheck
pnpm lint
pnpm format
```

## Layout

Feature-based. Each `features/<area>/` is self-contained: `components/`, `hooks/`, `types/`. Cross-feature code lives in `shared/`:

- `shared/api/*.ts` — Tauri command wrappers (the only place `invoke()` is called)
- `shared/components/ui/` — shadcn-style primitives
- `shared/lib/`, `shared/types/` — utilities and shared types

Path alias: `@/* → src/*`.

## Rules

- All backend access goes through `shared/api/*.ts` — never call `invoke()` directly from a component.
- Adding a Tauri command means a backend handler **and** a wrapper in `shared/api/`. The wrapper is the only place that names the command string, so feature code never hard-codes it.
