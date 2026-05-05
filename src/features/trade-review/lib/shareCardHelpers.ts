export function formatHumanDate(dateStr: string): string {
  const [y, m, d] = dateStr.split("-").map(Number)
  if (!y || !m || !d) return dateStr
  const dt = new Date(y, m - 1, d)
  const weekday = dt.toLocaleDateString("en-US", { weekday: "short" })
  const month = dt.toLocaleDateString("en-US", { month: "short" })
  return `${weekday} ${month} ${dt.getDate()}`
}

export function formatUsd(value: number): string {
  const sign = value < 0 ? "-" : ""
  return `${sign}$${Math.abs(value).toFixed(2)}`
}

export function pnlColor(value: number): string {
  if (value > 0) return "#4ade80"
  if (value < 0) return "#f87171"
  return "#a1a1aa"
}

export function firstSentence(md: string): string | null {
  const stripped = md
    .replace(/\*\*(.*?)\*\*/g, "$1")
    .replace(/\*(.*?)\*/g, "$1")
    .replace(/__(.*?)__/g, "$1")
    .replace(/_(.*?)_/g, "$1")
    .replace(/`([^`]*)`/g, "$1")
    .trim()
  if (!stripped) return null
  const match = stripped.match(/^([^.!?\n]+[.!?])(?:\s|$)/)
  return match ? match[1].trim() : stripped.split("\n")[0].trim()
}
