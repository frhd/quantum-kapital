import { TrendingUp, TrendingDown } from "lucide-react"
import { cn } from "../../../shared/lib/utils"
import type { Setup } from "../types"

interface SetupBadgeProps {
  setup: Setup
}

const STRATEGY_LABELS: Record<string, string> = {
  breakout: "Breakout",
  episodic_pivot: "Episodic Pivot",
  parabolic_short: "Parabolic Short",
}

function formatPrice(price: number): string {
  if (Number.isNaN(price)) return "—"
  if (price >= 1000) return price.toFixed(0)
  if (price >= 100) return price.toFixed(1)
  return price.toFixed(2)
}

export function SetupBadge({ setup }: SetupBadgeProps) {
  const label = STRATEGY_LABELS[setup.strategy] ?? setup.strategy
  const isLong = setup.direction === "long"
  const Icon = isLong ? TrendingUp : TrendingDown
  return (
    <span
      title={`${label} ${setup.direction.toUpperCase()} @ $${formatPrice(setup.trigger_price)} (stop $${formatPrice(setup.stop_price)})`}
      className={cn(
        "inline-flex items-center gap-1 rounded-full border px-2 py-0.5 text-xs font-medium",
        "animate-pulse",
        isLong
          ? "border-emerald-500 bg-emerald-500/10 text-emerald-200"
          : "border-rose-500 bg-rose-500/10 text-rose-200",
      )}
    >
      <Icon className="h-3 w-3" />
      <span>{label}</span>
      <span className="text-[10px] tracking-wide uppercase">{setup.direction}</span>
      <span className="text-foreground">${formatPrice(setup.trigger_price)}</span>
    </span>
  )
}
