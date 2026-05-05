---
name: trade-summary
description: Use when the user asks for a quick at-a-glance summary of how they traded today (or another date) — e.g. "how did I trade today", "summarize today's trading", "trade summary as a table", "today's P&L". Renders three compact tables (day metrics, by contract, untouched book) inline in chat. Lighter than /eod-review and /journal.
---

# Trade Summary

Inline tabular view of a trading day. Three tables:

1. **Day metrics** — net/gross P&L, commissions, round-trips, symbols, avg hold time
2. **By contract** — one row per traded contract with leg count, net P&L, terse notes
3. **Untouched book** — current open positions NOT traded today, with unrealized P&L

Surveillance-only. No order placement, no sizing recommendations, no grading.

## When to use

- User asks: "how did I trade today?", "summarize today", "trade table", "today's P&L"
- Quick post-close sanity check
- Lighter than `/eod-review` (writes a structured review row + grade) and `/journal` (writes a full markdown file). This skill renders **inline** and does NOT persist.

## Prerequisites

- Tauri app running so the `quantum-kapital` MCP server is reachable.
- For today's date, TWS/Gateway connected (fills stream live from IBKR).
- Past dates are served from the executions store.

## Procedure

1. **Resolve the date.** Default: today (ET). Parse user-supplied `YYYY-MM-DD` if given.

2. **Fetch in parallel via MCP:**
   - `mcp__quantum-kapital__get_trade_legs({ date })` — FIFO-matched legs + totals.
   - `mcp__quantum-kapital__get_positions()` — current open book.

3. **If `legs.length == 0`:** report `"No fills for {date}."` and stop. Do not render empty tables.

4. **Compute the "untouched book":** filter `positions` to rows where `position != 0` AND the position's contract key `(symbol, contract_type, expiry, strike, right)` does NOT appear in any of today's legs. Closed-flat options from today (`position: 0`) are excluded automatically.

5. **Render the three tables** as below. Round all dollar amounts to 2 decimals. Hold time = average of `hold_minutes` across legs (note range too).

## Output format

```markdown
## Trading day — {YYYY-MM-DD}

| Metric | Value |
|---|---|
| Net P&L | **${net_pnl:+.2f}** |
| Gross P&L | ${gross_pnl:+.2f} |
| Commissions | -${commissions:.2f} |
| Round-trips | {n_round_trips} |
| Carryover legs | {n_carryover}            ← omit row if 0
| Symbols traded | {comma-separated} |
| Avg hold | ~{avg} min (range {min}–{max}) |

### By contract

| Contract | Legs | Net P&L | Notes |
|---|---|---|---|
| {label} | {n} | {±$x.xx} | {≤12 words} |

### Untouched book

| Position | Qty | Unrealized |
|---|---|---|
| {label} | {qty} | {±$x} |
```

**Contract label format:**
- Option: `{SYM} {M/D exp} ${strike}{C|P}` — e.g. `TSLA 5/6 $400C`
- Stock: `{SYM} (STK)` — e.g. `RDDT (STK)`

**Untouched-book section:** omit entirely if empty. Sort by abs(unrealized) desc so biggest exposures lead.

## "Notes" column rules

Choose 1–3 of the most informative observations from each contract's legs:

- **Best/worst legs:** cite leg_id + signed net_pnl for the largest winner and largest loser when both exist (e.g. `leg_003 +$81.76`).
- **Scaling pattern:** if multiple legs carry `scaled_in` AND avg buy price *decreases* while size *increases*, write `scaled into weakness`. If `scaled_in` AND avg buy price increases on a winner, write `scaled into strength`.
- **Re-entry:** if the same contract has an early `partial_close` winner followed by a later loser, write `re-entered after partial close`.
- **Single big winner:** if one leg carries the contract, just cite that leg.

Keep each cell ≤ 12 words. No emoji.

## Edge cases

- **Stock-only day:** still render `By contract`; just no strike/right columns in the labels.
- **Past date with `n_carryover > 0`:** the "Open at close" picture is approximate — `get_positions` is *current* state, not historical. Note this in a one-line caveat under the day-metrics table.
- **Multi-account:** the MCP layer is single-account today. If `get_positions` errors with multi-account guidance, ask the user which account, then re-call both tools with `account: "<id>"`.
- **TWS/Gateway down (today only):** `get_trade_legs` will error. Surface the connection error verbatim and stop — do not fabricate data.

## What this does NOT do

- Does NOT call `write_trade_review` — that's `/eod-review`.
- Does NOT write to `journal/` — that's `/journal`.
- Does NOT consume the `LlmService` budget ledger — pure data formatting, no LLM call needed for the tables themselves.
- Does NOT issue a grade — `/eod-review`'s server-side rubric does that.
- Does NOT place orders. Surveillance only.
