/**
 * Format an ISO timestamp as a short "Ns/Nm/Nh/Nd ago" string.
 *
 * Returns an empty string for unparseable input rather than throwing —
 * callers render the result inline alongside other metadata and a
 * blank slot is always preferable to a runtime error from a malformed
 * server payload.
 */
export function relativeTime(iso: string): string {
  const t = new Date(iso).getTime()
  if (Number.isNaN(t)) return ""
  const diffMs = Date.now() - t
  const sec = Math.round(diffMs / 1000)
  if (sec < 60) return `${sec}s ago`
  const min = Math.round(sec / 60)
  if (min < 60) return `${min}m ago`
  const hr = Math.round(min / 60)
  if (hr < 24) return `${hr}h ago`
  const day = Math.round(hr / 24)
  return `${day}d ago`
}
