# IBKR Flex backfill — token + query setup

How to generate an IBKR Flex Web Service token and configure the Activity Flex Query that the `flex_backfill` binary (`src-tauri/src/bin/flex_backfill.rs`) consumes to seed the executions store with pre-Phase-1 fill history.

> **Scope note.** `loop/plan/master.md:161` lists pre-Phase-1 backfill as "out of scope for v1, separate program if ever needed." This binary _is_ that separate program — it's not part of any phase's tracer-bullet, just an opt-in importer for users who want historical reviews.

## Why a Flex token, not the TWS API

The TWS API's `reqExecutions` endpoint is **current-TWS-day-only** — it can't return prior days' fills under any flag. The Flex Web Service is the parallel reporting surface that *can* return arbitrary historical ranges. Flex requires a token + a pre-defined Query ID; both are managed in IBKR's Client Portal.

## Step 1 — Create the Flex Query

1. Log in to https://www.interactivebrokers.com/sso/Login → user menu → **Performance & Reports** → **Flex Queries**.
2. **Activity Flex Query** → **+ Create**.
3. Name it (e.g. `qk_backfill_trades`).
4. Sections: enable **Trades** only.
5. Trades section options:
   - **Executions** ✅ (per-fill granularity — not the default per-order rollup).
   - **Include Canceled Trades** ❌
6. Format: **XML**.
7. Date format: `yyyy-MM-dd`. Time format: `HH:mm:ss`. Date/Time separator: single space.
8. Save → note the **Query ID** (numeric, e.g. `1234567`).

## Step 2 — Trades field selection

Tick exactly these in the field picker (the picker presents fields in this order; mark each row as you go):

```
✅ Account ID                       — required
❌ Account Alias
❌ Model
✅ Currency                         — instrument currency (USD/EUR/...)
⚪ FX Rate To Base                  — useful only for non-USD trades
✅ Asset Class                      — STK / OPT / FUT / CASH (the schema's contract_type)
⚪ Sub Category                     — diagnostic
✅ Symbol
❌ Description
⚪ Conid                            — instrument fingerprint
❌ Security ID
❌ Security ID Type
❌ CUSIP
❌ ISIN
❌ FIGI
❌ Listing Exchange
❌ Underlying Conid
❌ Underlying Symbol                — redundant with OCC-format Symbol for options
❌ Underlying Security ID
❌ Underlying Listing Exchange
❌ Issuer
❌ Issuer Country Code
✅ Trade ID                         — primary idempotency key for backfilled rows
✅ Multiplier                       — required for options ("100" typical)
❌ Related Trade ID                 — combo-leg linkage; not used in v1
✅ Strike                           — required for options
❌ Report Date
✅ Expiry                           — required for options
✅ Date/Time                        — exec_time
✅ Put/Call                         — option right
⚪ Trade Date                       — belt-and-suspenders for partial Date/Time
❌ Principal Adjust Factor
❌ Settle Date Target
⚪ Transaction Type                 — distinguishes ExchTrade / FracShare / Adjustment
⚪ Exchange                         — diagnostic
✅ Quantity                         — signed in Flex; abs() during ingest
✅ TradePrice
⚪ Trade Money                      — sanity check (qty*price)
❌ Proceeds
❌ Taxes
✅ IB Commission                    — signed negative in Flex; flip sign during ingest
✅ IB Commission Currency
⚪ Net Cash                         — sanity check
❌ Close Price
⚪ Open/Close Indicator             — cross-check FIFO matcher's open/close split
⚪ Notes/Codes                      — IBKR flags (P=partial, Ex=cross, ...)
❌ Cost Basis
⚪ Realized P/L                     — IBKR's own realized P&L; cross-check FIFO to ±$0.01
❌ MTM P/L
❌ Orig Trade Price
❌ Orig Trade Date
❌ Orig Trade ID
❌ Orig Order ID
❌ Orig Transaction ID
✅ Buy/Sell                         — side ("BUY"/"SELL")
❌ Clearing Firm ID
✅ IB Order ID                      — order_id (NOT Brokerage/Exch Order ID)
⚪ Transaction ID                   — secondary key
⚪ IB Execution ID                  — matches live execId; helps dedup vs live-ingested rows
❌ Related Transaction ID
❌ RTN
❌ Brokerage Order ID
❌ Order Reference
❌ Volatility Order Link
❌ Exch Order ID
❌ External Execution ID
⚪ Order Time                       — when order was placed (vs filled); diagnostic
❌ Open Date Time
❌ Holding Period Date Time (Wash Sales)
❌ When Realized (Wash Sales)
❌ When Reopened (Wash Sales)
⚪ Level Of Detail                  — verifies execution-granularity rows
❌ Change In Price
❌ Change In Quantity
⚪ Order Type                       — diagnostic
❌ Trader ID
❌ Is API Order
❌ Accrued Interest
❌ Initial Investment
❌ Position Action ID
❌ Serial Number
❌ Delivery Type
❌ Commodity Type
❌ Fineness
❌ Weight
```

Tally: **12 required (✅)**, 14 recommended extras (⚪), rest off (❌).

### Mapping to `IbkrExecution` (`src-tauri/src/ibkr/types/orders.rs:12`)

| Flex field | `IbkrExecution` field | Notes |
|---|---|---|
| `Account ID` | `account` | Multi-account safety. |
| `Symbol` | `symbol` | |
| `Asset Class` | `contract_type` | `STK` / `OPT` / `FUT` / `CASH`. |
| `Buy/Sell` | `side` | Normalize `"BUY"`/`"SELL"` → `ExecutionSide`. |
| `Quantity` | `qty` | Signed in Flex; `abs()` during ingest. |
| `TradePrice` | `avg_price` | |
| `Date/Time` (+ `Trade Date`) | `exec_time` | Combine if separate. |
| `Trade ID` | `exec_id` | **Backfill primary key.** Not byte-identical to live `execId` — see "Idempotency" below. |
| `IB Order ID` | `order_id` | i32. |
| `IB Commission` | `commission` | Signed negative in Flex; flip sign per existing convention. |
| `IB Commission Currency` | `commission_currency` | |
| `Currency` | `currency` | Contract currency. |
| `Expiry` | `expiry` | Options only. |
| `Strike` | `strike` | Options only. |
| `Put/Call` | `right` | Options only. Normalize `"C"`/`"P"` (or `"CALL"`/`"PUT"`). |
| `Multiplier` | `multiplier` | Options only. Stringified ("100" typical). |

## Step 3 — Generate the Web Service token

1. Same Flex Queries page → **Flex Web Service** section → **Configure** → **Enable**.
2. Click the regenerate / display icon → copy the token (long alphanumeric string).
3. Whitelist your IP if IBKR prompts.
4. Tokens are valid for ~1 year and can be revoked at any time.

You now have two values:

- `IBKR_FLEX_TOKEN` — the secret string (treat as a credential).
- `IBKR_FLEX_QUERY_ID` — the numeric Query ID from Step 1.

## Step 4 — Where to put the token

Two options, **B preferred for one-shot backfill**.

### Option A — persist in `src-tauri/.env`

Use this only if you'll re-run the backfill periodically. The file is already gitignored alongside `ANTHROPIC_API_KEY` etc.

```ini
# src-tauri/.env
IBKR_FLEX_TOKEN=<paste token>
IBKR_FLEX_QUERY_ID=<paste query id>
```

### Option B — pass at invocation, never persist

Token only lives in the shell's process env for that one run; nothing on disk to clean up.

```sh
IBKR_FLEX_TOKEN=<paste> IBKR_FLEX_QUERY_ID=<paste> \
  cargo run --manifest-path src-tauri/Cargo.toml --bin flex_backfill -- \
  --from 2025-01-01 --to 2026-05-04 --dry-run
```

Drop `--dry-run` once the diff looks right. Revoke the token in IBKR after the import finishes.

**Never paste the token into chat / commits / tickets.**

## Step 5 — Run the backfill

```sh
# dry-run first (parse + stats, no DB writes)
IBKR_FLEX_TOKEN=<paste> IBKR_FLEX_QUERY_ID=<paste> \
  cargo run --manifest-path src-tauri/Cargo.toml --bin flex_backfill -- \
  --from 2025-01-01 --to 2026-05-04 --dry-run

# real run (writes to the executions store)
IBKR_FLEX_TOKEN=<paste> IBKR_FLEX_QUERY_ID=<paste> \
  cargo run --manifest-path src-tauri/Cargo.toml --bin flex_backfill -- \
  --from 2025-01-01 --to 2026-05-04
```

Flags:

- `--from YYYY-MM-DD` / `--to YYYY-MM-DD` (required) — ET trading-day window. Rows outside the window are dropped after parsing, regardless of what the Flex Query template's Period says.
- `--dry-run` — parse + stats only; no DB writes. Run this first.
- `--keep-cash` — don't drop CASH rows. Default is to drop them; the FIFO matcher has no forex support, and most CASH rows are auto-FX residuals from currency conversions of dividends/commissions.
- `--db PATH` — override the SQLite path. Defaults to the OS app-data dir (Linux: `~/.local/share/com.quantyc.qqk/tracker.sqlite`).
- `--fixture PATH` — read XML from a local file instead of fetching from IBKR. Used by the test suite and for re-runs against a saved smoke-test report.

The dry-run prints:

- `parsed N <Trade> rows from XML` — total count.
- `dropped N CASH rows ...` — auto-FX residuals filter.
- `date-range filter: K kept, M outside [from..=to]`.
- `mapping errors: ...` — rows that failed the schema mapping (with the first 10 reasons).
- `--- stats ---`: row count, ET date range, accounts, currencies, asset categories, trading days touched.

If currencies includes anything other than `USD`, **stop**: the FIFO aggregator (`services/trade_legs/fifo.rs`, `LegSummary`) is currency-blind and will silently sum mixed-currency P&L as if it were one unit. This is a pre-existing limitation, not a backfill bug.

If asset categories includes anything beyond `STK` and `OPT` (e.g. `FUT`), the FIFO matcher hasn't been tested against those types — proceed with caution.

## Idempotency

The binary writes rows with `source='flex'` and prefixes their `exec_id` with `flex:` (e.g. `flex:8849853516`). Three guarantees:

1. **Re-runs are safe.** If a `flex:<tradeID>` already exists in the store, it's skipped (`skipped_existing` in the summary).
2. **Live rows always win.** Before inserting a flex row, the binary checks for a `source='live'` row matching `(account, symbol, side, qty, avg_price)` within ±60s of `exec_time`. If matched, the flex row is skipped (`skipped_live_match`).
3. **Cross-source collisions are impossible.** The `flex:` prefix means a Flex `tradeID` can never UPSERT-overwrite a live `reqExecutions` `execId`.

This makes it safe to run the backfill on a date range that overlaps with days the live ingestor already covered — you'll see `skipped_live_match` increment for the overlap, no double-counting.

## Schema notes

- **`executions.source` column** (`V16__executions_source.sql`) — `'live'` (default for existing rows) or `'flex'` (backfill rows).
- **`order_id` overflow.** Flex `ibOrderID`s come from a different subsystem than live `reqExecutions` order IDs and routinely exceed `i32::MAX` (≈2.1B). The binary saturates overflow to `0`. The only consumer of `order_id` is the `complex_strategy` heuristic in `fifo.rs:72`; it just won't fire on saturated rows. Acceptable for historical backfill.
- **Commission sign.** Flex reports commissions as signed-negative (`ibCommission="-1.0459"`). The binary flips the sign so backfill rows match the live ingestor's "magnitude" convention.
- **Option `symbol`.** Flex emits OCC-padded option symbols like `"AMD   251010P00225000"`. The binary takes the leading non-space token (`"AMD"`) and trusts the explicit `expiry` / `strike` / `putCall` / `multiplier` attributes for the rest.

## Other caveats

- **Multi-leg combo orders** show as separate rows in Flex (same as `reqExecutions`); the FIFO leg-matcher treats them as independent legs in v1, flagged `complex_strategy` only if the same (live) `order_id` spans multiple legs. Backfill rows with saturated `order_id=0` won't trigger that flag.
- **Commissions sometimes differ from live ±$0.01** due to Flex's occasional per-fill rounding. Cross-check `Realized P/L` (Flex's own number) against the FIFO matcher's `net_pnl` after backfill to catch any drift.

## Multi-currency note

The store is currency-aware (`currency` + `commission_currency` columns); aggregation downstream (`trade_legs/fifo.rs`, `LegSummary`) is **not**. Mixing currencies sums raw `f64`s — silently wrong on multi-currency days. This is a pre-existing limitation, not a backfill issue. Single-currency accounts are fine; multi-currency accounts need the aggregator fixed first.

The dry-run's `unique currencies seen` line tells you which case you're in.

## Token hygiene

- **Rotate** after the backfill if you used Option A. Regenerate in Client Portal; the old token is immediately invalidated.
- **Revoke** if you suspect the token was exposed (committed, pasted in chat, sent in an email, etc.).
- IBKR logs Flex Web Service requests; `unusual` patterns may trigger account review.
