import { useCallback, useEffect, useRef, useState, type ReactElement } from "react"
import { createPortal } from "react-dom"
import { CheckCircle2, AlertTriangle, Info, X } from "lucide-react"
import { cn } from "../../lib/utils"

export type ToastVariant = "info" | "success" | "warning"

export interface ToastInput {
  id?: string
  title: string
  description?: string
  variant?: ToastVariant
  durationMs?: number
}

interface ToastEntry extends Required<Omit<ToastInput, "description" | "id">> {
  id: string
  description?: string
}

const DEFAULT_DURATION = 5000

export function useToasts() {
  const [toasts, setToasts] = useState<ToastEntry[]>([])
  const seqRef = useRef(0)

  const dismiss = useCallback((id: string) => {
    setToasts((prev) => prev.filter((t) => t.id !== id))
  }, [])

  const push = useCallback((input: ToastInput) => {
    const id = input.id ?? `toast-${++seqRef.current}`
    const entry: ToastEntry = {
      id,
      title: input.title,
      description: input.description,
      variant: input.variant ?? "info",
      durationMs: input.durationMs ?? DEFAULT_DURATION,
    }
    setToasts((prev) => {
      const exists = prev.some((t) => t.id === id)
      return exists ? prev : [...prev, entry]
    })
    return id
  }, [])

  return { toasts, push, dismiss }
}

interface ToastViewportProps {
  toasts: ToastEntry[]
  onDismiss: (id: string) => void
}

const VARIANT_STYLES: Record<ToastVariant, { wrapper: string; icon: ReactElement }> = {
  info: {
    wrapper: "border-slate-700 bg-slate-800/95",
    icon: <Info className="h-4 w-4 text-blue-400" />,
  },
  success: {
    wrapper: "border-emerald-700 bg-emerald-900/40",
    icon: <CheckCircle2 className="h-4 w-4 text-emerald-300" />,
  },
  warning: {
    wrapper: "border-amber-700 bg-amber-900/40",
    icon: <AlertTriangle className="h-4 w-4 text-amber-300" />,
  },
}

export function ToastViewport({ toasts, onDismiss }: ToastViewportProps) {
  if (typeof document === "undefined") return null
  return createPortal(
    <div className="pointer-events-none fixed right-4 bottom-4 z-50 flex w-80 flex-col gap-2">
      {toasts.map((t) => (
        <ToastItem key={t.id} toast={t} onDismiss={onDismiss} />
      ))}
    </div>,
    document.body,
  )
}

interface ToastItemProps {
  toast: ToastEntry
  onDismiss: (id: string) => void
}

function ToastItem({ toast, onDismiss }: ToastItemProps) {
  const { id, title, description, variant, durationMs } = toast
  const styles = VARIANT_STYLES[variant]

  useEffect(() => {
    if (durationMs <= 0) return
    const handle = window.setTimeout(() => onDismiss(id), durationMs)
    return () => window.clearTimeout(handle)
  }, [id, durationMs, onDismiss])

  return (
    <div
      role="status"
      className={cn(
        "pointer-events-auto flex items-start gap-2 rounded-md border px-3 py-2 text-sm text-white shadow-lg backdrop-blur-sm transition-opacity",
        styles.wrapper,
      )}
    >
      <span className="mt-0.5">{styles.icon}</span>
      <div className="flex-1">
        <p className="leading-tight font-medium">{title}</p>
        {description && <p className="mt-0.5 text-xs text-slate-300">{description}</p>}
      </div>
      <button
        type="button"
        onClick={() => onDismiss(id)}
        className="text-slate-400 hover:text-white"
        aria-label="Dismiss"
      >
        <X className="h-3.5 w-3.5" />
      </button>
    </div>
  )
}
