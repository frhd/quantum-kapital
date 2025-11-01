export const formatCurrency = (value: number) => {
  // For values over 10k, show as "10.5k", etc
  if (Math.abs(value) >= 10000) {
    return `$${(value / 1000).toFixed(1)}k`
  }
  return new Intl.NumberFormat("en-US", {
    style: "currency",
    currency: "USD",
    minimumFractionDigits: 0,
    maximumFractionDigits: 0,
  }).format(value)
}

export const formatPercent = (value: number) => {
  return `${value >= 0 ? "+" : ""}${value.toFixed(2)}%`
}

export const anonymizeAccountNumber = (accountNumber: string) => {
  if (!accountNumber || accountNumber.length <= 2) {
    return accountNumber
  }
  const lastTwo = accountNumber.slice(-2)
  const masked = "*".repeat(accountNumber.length - 2)
  return masked + lastTwo
}