/// <reference types="vite/client" />

interface ImportMetaEnv {
  readonly VITE_BROWSER_DEV?: string
}

interface ImportMeta {
  readonly env: ImportMetaEnv
}
