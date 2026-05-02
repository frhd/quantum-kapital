# IBKR Reuters Fundamentals — XML structure & capture plan

Phase 2 reference notes for the AV → IBKR Reuters migration. This file
is the single source of truth for the crate-path decision and the
Reuters XML structure Phase 4 will parse.

> **Status of this file (2026-05-02):** doc-only research draft, no
> live-captured fixtures yet. Sections marked **(needs live capture)**
> stay tentative until the user runs the capture script against a TWS
> session with a Reuters Worldwide Fundamentals subscription. See
> `QUESTIONS.md` for the live-capture handoff.

## Crate-path decision

**Decision:** **Fork `ibapi` and expose `req_fundamental_data`** for the
Phase 4 production provider. For the Phase 2 spike capture, use the
**official Python `ibapi` package** (fastest path to fixtures on disk).

### Investigation summary

`ibapi = "2.11.2"` (latest at 2026-05-02: 2.11.3) does not expose any
high-level fundamental-data API. Verified by inspecting both the
locally-resolved 2.11.2 source and the upstream `main` branch of
`wboayue/rust-ibapi`:

- `src/messages.rs` defines `OutgoingMessages::RequestFundamentalData = 52`
  and `IncomingMessages::FundamentalData = 51`.
- `src/server_versions.rs` defines `FUNDAMENTAL_DATA = 40` (the minimum
  TWS server version that supports the request).
- `src/client/sync.rs` exposes ~80 `pub fn` methods on `Client`
  (positions, orders, market data, news, scanner, WSH, …) — none for
  fundamentals.
- The `transport::MessageBus` trait that `Client` uses internally is
  `pub(crate)`, so a downstream crate cannot synthesise an outgoing
  frame and route it through the existing connection.

### Options weighed

| Option | Verdict |
|---|---|
| **(a) Fork `ibapi`** and add a thin `Client::fundamental_data` method | **Chosen.** Smallest patch surface, mirrors existing `news_article` / `historical_news` shape, keeps a single TWS connection. Vendored via `[patch.crates-io]` in `src-tauri/Cargo.toml`; Phase 4 is the consumer. We submit the patch upstream in parallel; if/when it merges, drop the patch and bump the version. |
| (b) Raw TCP wrapper bypassing `ibapi` | Rejected. Re-implements the handshake, reconnect, and message-router logic that `ibapi` already provides. Two parallel TWS connections (one per `clientId`) means we have to allocate a second client ID and risk paper-account collision. |
| (c) Switch crates | Rejected. `wboayue/rust-ibapi` is the only mature Rust crate; alternatives (`twsapi`, hand-rolled) are unmaintained. |

### Spike-capture path (independent of the fork)

Phase 2's exit goal is **fixtures on disk**, not a production code
path. The official IBKR Python `ibapi` package (`pip install ibapi`)
exposes `EClient.reqFundamentalData(reqId, contract, reportType, [])`
and delivers XML in the `EWrapper.fundamentalData(reqId, data)`
callback. A ~80-line script captures all four reportTypes for AAPL
in <30 seconds. Recommended workflow:

1. User starts TWS / IB Gateway on `127.0.0.1:7497` (paper) or `:7496`
   (live), with API access enabled.
2. Verify Reuters Worldwide Fundamentals is active under TWS →
   Account → Market Data Subscriptions.
3. Run the Python capture script (see "Capture script blueprint"
   below) which writes:
   - `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_ReportSnapshot.xml`
   - `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_ReportsFinSummary.xml`
   - `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_ReportsFinStatements.xml`
   - `src-tauri/tests/fixtures/ibkr_fundamentals/AAPL_RESC.xml`
4. Phase 4 parser tests load these files via `include_str!`.

The Rust spike binary (`src-tauri/src/bin/ibkr_fundamentals_spike.rs`)
exists as a feature-gated stub pointing here. Once the fork lands in
Phase 4, the same fork will let us re-implement that binary in Rust if
we want — but it is not on the critical path.

## Reuters reportType reference

Four `reportType` values are passed to `reqFundamentalData`. All four
are XML payloads. The structures below are derived from public
Reuters / IBKR documentation; concrete XPath / element names will be
**re-verified against the captured fixtures** before Phase 4 parsers
are written.

### `ReportSnapshot` — company snapshot

Top-level element: `<ReportSnapshot>` with attributes for `Major`,
`Minor`, `Revision`, `CompanyName`. Key sub-elements:

- `<CoIDs>` — company identifiers (RIC, CIK, ticker, ISIN).
- `<Issues>` / `<Issue>` — listed shares, exchange, primary listing.
- `<CoGeneralInfo>` — company name, country, IPO date.
- `<TextInfo>` / `<Text Type="Business Summary">` — long-form
  description.
- `<contactInfo>` — address, phone, website.
- `<webLinks>`.
- `<Industry>` and `<peerInfo>`.
- `<Ratios>` — **the primary structured payload for `CurrentMetrics`**:
  - `Group ID="Income Statement"` with rows like `Item coaCode="TTMREV"`,
    `coaCode="EBITDA"`, etc.
  - `Group ID="Profitability"` — gross/net margins.
  - `Group ID="Per Share Data"` — EPS, BVPS, dividends per share.
  - `Group ID="Price and Volume"` — `coaCode="LFY"` (last fiscal-year
    close), `MARKETCAP`.
  - `Group ID="Valuation Ratios"` — `coaCode="PEEXCLXOR"` (trailing
    P/E), `PRICE2SAL`, `PRICE2BK`, `DIVYIELD`.

Mapping into `FundamentalData` (`src-tauri/src/ibkr/types/fundamentals.rs`):

| `FundamentalData` field | Reuters source |
|---|---|
| `current_metrics.pe_ratio` | `Ratios/Group[@ID="Valuation Ratios"]/Item[@coaCode="PEEXCLXOR"]` |
| `current_metrics.shares_outstanding` (millions) | `Ratios/Group[@ID="Per Share Data"]/Item[@coaCode="TTMNIPEREM"]` (or `CoGeneralInfo`/`Issues`) — **(needs live capture to disambiguate)** |
| `current_metrics.name` | `ReportSnapshot/CoIDs/CoID[@Type="CompanyName"]` |
| `current_metrics.exchange` | `Issues/Issue/Exchange` |
| `current_metrics.market_cap` | `Ratios/Group[@ID="Price and Volume"]/Item[@coaCode="MARKETCAP"]` |
| `current_metrics.dividend_yield` | `Ratios/Group[@ID="Valuation Ratios"]/Item[@coaCode="DIVYIELD"]` |

Live price (`current_metrics.price`) is not in `ReportSnapshot` — it
comes from the existing market-data path; keep it `Option<f64>` as
today.

### `ReportsFinSummary` — annual / interim summary

Top-level element: `<FinancialSummary>`. Used for the
**`historical: Vec<HistoricalFinancial>`** field (revenue, net income,
EPS history). Structure:

- `<TotalRevenues>` containing `<TotalRevenue asOfDate="..." reportType="A|Q" period="...">` rows.
- `<NetIncome>` containing `<NetIncome asOfDate="..." reportType="A|Q">` rows.
- `<EPSs>` containing `<EPS asOfDate="..." reportType="A|Q">` with
  `<EPSBasic>` and `<EPSDiluted>` children.
- `<DividendPerShares>`.

Filter `reportType="A"` (annual) and group by fiscal year. Numeric
values are in millions (USD, per `currency="USD"` attribute on the
parent). Convert to billions in the parser to match
`HistoricalFinancial::revenue` and `net_income` (already in billions
per AV semantics).

| `HistoricalFinancial` field | Reuters source |
|---|---|
| `year` | year extracted from `asOfDate` |
| `revenue` (billions) | `TotalRevenue` value / 1000.0 |
| `net_income` (billions) | `NetIncome` value / 1000.0 |
| `eps` | `EPSDiluted` value (already per share) |

### `ReportsFinStatements` — full statements (income, balance, cash flow)

Top-level element: `<ReportFinancialStatements>`. Heaviest payload of
the four reportTypes. Useful for back-up validation of `FinSummary`
revenue/income numbers and for fields not in the summary (e.g.
margin reconstruction, share-count history). For Phase 4 v1 this
report is **read-but-not-parsed** unless `FinSummary` is missing
fields. Capture it now so we never have to re-fixture later.

Structure (top-down):
- `<CoIDs>` and `<Issues>` (same as `ReportSnapshot`).
- `<StatementInfo>` with `Statements` containing per-period
  `<Statement Type="INC|BAL|CAS">` blocks.
- Each `<Statement>` has `<lineItem coaCode="...">` rows, e.g.
  `coaCode="RTLR"` (total revenue), `NINC` (net income), `SDED`
  (selling/general/admin), `EPSDIL` (diluted EPS).
- Annual rows reportable via `Type="Annual"` on the parent
  `<APeriods>` block; interim via `<IPeriods>`.

### `RESC` — analyst estimates (Reuters Estimates)

Top-level element: `<REResearchAndAnalysis>`. Source for
**`AnalystEstimates`**. Structure:

- `<Issues>` / `<Issue>` — primary issue.
- `<Estimates>` containing per-`<Period>` blocks keyed by
  `periodTypeId` (`A` = annual, `Q` = quarterly) and `endMonth`/`fYear`.
- Each period has `<ConsEstimate>` rows for each measure: `SALES`,
  `EPS`, `EBITDA`, `OPR`, `NET`. Each `<ConsEstimate>` has a
  `<ConsValue>` with `Mean`, `High`, `Low`, `StdDev`,
  `NumOfEstimates` children.

Map FY+1 / FY+2 annual rows into `AnalystEstimate`:

| `AnalystEstimate` field | Reuters source |
|---|---|
| `year` | `<Period fYear="...">` (annual only) |
| `estimate` (revenue list) | `ConsEstimate type="SALES"/ConsValue/Mean` |
| `estimate` (eps list) | `ConsEstimate type="EPS"/ConsValue/Mean` |

`AnalystEstimates` is `Option<>` because some symbols (sparse
coverage) won't have an `RESC` payload — in that case TWS returns
error code 200 ("no data"), not a parse-empty payload.

## Capture script blueprint (Python, `ibapi` pip package)

```python
# capture_ibkr_fundamentals.py — throwaway, not committed to the repo
# Run AFTER: pip install ibapi  AND  TWS / Gateway running on :7497
# Writes 4 XML files into src-tauri/tests/fixtures/ibkr_fundamentals/.
import threading, time, pathlib
from ibapi.client import EClient
from ibapi.contract import Contract
from ibapi.wrapper import EWrapper

OUT = pathlib.Path("src-tauri/tests/fixtures/ibkr_fundamentals")
OUT.mkdir(parents=True, exist_ok=True)

class App(EWrapper, EClient):
    def __init__(self):
        EClient.__init__(self, self)
        self.pending = {}    # reqId -> reportType
        self.done = threading.Event()

    def fundamentalData(self, reqId, data):
        rt = self.pending.pop(reqId, f"unknown_{reqId}")
        path = OUT / f"AAPL_{rt}.xml"
        path.write_text(data)
        print(f"wrote {path}  ({len(data)} bytes)")
        if not self.pending:
            self.done.set()

    def error(self, reqId, code, msg, advancedOrderRejectJson=""):
        print(f"err reqId={reqId} code={code} msg={msg}")
        if code == 430:
            print("=> No Reuters Worldwide Fundamentals subscription on this account.")
        if code in (200, 430, 504, 162):
            self.pending.pop(reqId, None)
            if not self.pending:
                self.done.set()

def aapl():
    c = Contract()
    c.symbol, c.secType, c.exchange, c.currency = "AAPL", "STK", "SMART", "USD"
    return c

app = App()
app.connect("127.0.0.1", 7497, clientId=999)
threading.Thread(target=app.run, daemon=True).start()
time.sleep(1.0)  # let handshake settle

reports = ["ReportSnapshot", "ReportsFinSummary",
           "ReportsFinStatements", "RESC"]
for i, rt in enumerate(reports, start=1):
    app.pending[i] = rt
    app.reqFundamentalData(i, aapl(), rt, [])
    time.sleep(2.0)  # respect TWS pacing (~60 / 10 min for fundamentals)

app.done.wait(timeout=60)
app.disconnect()
```

Expected error codes from TWS:

- **200** — "No security definition has been found for the request" or
  "fundamentals not available for this contract". Treat as a typed
  "no data" response (becomes `FundamentalsError::NotAvailable` in
  Phase 3).
- **430** — "We requested the news subscription, however we do not
  have it from the FA". Despite the message wording, this is the
  subscription-missing error for fundamentals too. Becomes
  `FundamentalsError::NotSubscribed` in Phase 3.
- **504** — "Not connected" / TWS connection lost. Becomes
  `FundamentalsError::Disconnected`.
- **162** — historical-data pacing exceeded (rare for fundamentals
  but the same message family). Becomes `FundamentalsError::Pacing`.

## TWS request frame (for the eventual fork)

For the upstream-PR-or-fork patch, the outgoing wire frame for
`reqFundamentalData` (per `EClient.reqFundamentalData` in the
official Python client) is:

```
field 0:  "52"                    # OutgoingMessages::RequestFundamentalData
field 1:  "2"                     # version
field 2:  reqId
field 3:  contract.conId          # 0 if unknown
field 4:  contract.symbol
field 5:  contract.secType        # "STK"
field 6:  contract.lastTradeDateOrContractMonth   # "" for STK
field 7:  contract.strike         # 0 for STK
field 8:  contract.right          # "" for STK
field 9:  contract.multiplier     # ""
field 10: contract.exchange       # "SMART"
field 11: contract.primaryExchange # "" (or "NASDAQ" for AAPL)
field 12: contract.currency       # "USD"
field 13: contract.localSymbol    # ""
field 14: contract.tradingClass   # ""
field 15: reportType              # one of the four strings above
field 16: tagValuesCount          # "0" (no fundamental_data_options)
```

Each field NUL-terminated, then the whole buffer length-prefixed (4
bytes BigEndian) per `messages::encode_length`. Server requires
`server_version >= FUNDAMENTAL_DATA (40)`, which any modern TWS
satisfies.

Incoming response (message type 51):

```
field 0: "51"
field 1: version (currently 1)
field 2: reqId
field 3: xmlData         (single field, can be very large; sometimes
                          delivered across multiple frames — the
                          existing ibapi MessageBus already handles
                          frame reassembly)
```

## Open questions

- (Tracked in `QUESTIONS.md`.) None of the structure details above
  have been verified against captured fixtures yet — once the user
  runs the Python capture, we may discover (a) different XPath
  shapes, (b) units in thousands vs. millions, (c) `coaCode`
  variations across non-US issuers. Phase 4 parsers are written
  *after* the fixtures land.
