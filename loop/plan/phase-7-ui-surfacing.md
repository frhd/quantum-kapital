# Phase 7 — UI surfacing: Trade Review card, Today's Playbook panel, Trader Profile dashboard

> Part of [Behavioral assessment via MCP](master.md). See master for invariants.

**Status:** done (commit d1bddb5, 2026-05-05)

**Depends on:** 4 (`day_reviews` exists), 5 (`playbooks` exists), 6 (`trader_profile` exists)

**Goal:** Bring all three artifacts to the desktop UI so the user can see them without opening a Claude Code session. Ship three feature folders following `src/CLAUDE.md`'s convention: `src/features/trade-review/`, `src/features/playbook/`, `src/features/trader-profile/`. Each backed by a Tauri command wrapping the same Rust service the corresponding MCP tool calls — no duplicate logic.

**Why this matters:** the assessment stack is invisible until it's in the app. Without this phase the artifacts exist but the user only sees them via LLM-mediated chat. Phase 7 makes the moat visible: open the app, see today's playbook, yesterday's review, the trailing 30-day behavioral dashboard.

This phase is mostly mechanical (frontend feature folders + Tauri command wrappers), so the TDD discipline is lighter than Phases 1/2/4. The interesting work is shape: each panel must read cleanly, distinguish "no data yet" from "no review for this date," and surface freshness markers.

## End-state for this phase

- Three new Tauri commands:
  - `get_trade_review(date: String, account?: String, prompt_version?: i32) -> TradeReviewDto`
  - `get_today_playbook(date: String, account?: String, generation_id?: i32) -> PlaybookDto`
  - `get_trader_profile(window_days?: u32, account?: String) -> TraderProfileDto`
- Three new feature folders, each with `components/`, `hooks/`, `types.ts`.
- Three new routes in the existing router (or new tabs in the main shell):
  - `/review/:date` (default `/review/today`) — Trade Review card.
  - `/playbook/:date` (default `/playbook/today`) — Today's Playbook panel.
  - `/profile` — Trader Profile dashboard.
- The existing Trades panel (shipped in the retired plan's Phase 3) gains a "Review" link in its summary banner that navigates to `/review/:date` for the displayed date.
- Loading, empty, and error states for all three views.

## Files

**Create (Rust — Tauri commands):**
- `src-tauri/src/ibkr/commands/assessments.rs` — three Tauri commands wrapping the existing services.
  - Each command pulls the relevant `Arc<Service>` via `State<...>` and returns the same DTO shape as the MCP tool.
  - These are NOT MCP tools (already shipped in Phases 4/5/6). They're FE-facing Tauri commands.
- Modify: `src-tauri/src/lib.rs` — register the three new `#[tauri::command]` handlers via `.invoke_handler(...)`.

**Create (Frontend — `src/features/trade-review/`):**
- `index.ts` — re-exports.
- `types.ts` — TypeScript mirrors of the Rust DTOs.
- `hooks/useTradeReview.ts` — calls the Tauri command via `shared/api/`.
- `components/TradeReviewCard.tsx` — main card.
- `components/GradeBadge.tsx` — A/B/C/D/F badge with color coding.
- `components/BehavioralTagChip.tsx` — chip for each tag, color-coded by weight (positive = green, negative = red).
- `components/LegObservationsList.tsx` — collapsible list.
- `components/EmptyTradeReview.tsx` — empty state ("no review yet for {date}").

**Create (Frontend — `src/features/playbook/`):**
- `index.ts`, `types.ts`
- `hooks/usePlaybook.ts`
- `components/TodaysPlaybook.tsx` — main panel.
- `components/RankedSetupCard.tsx` — one card per setup with Trigger/Entry/Invalidation/Target.
- `components/SkipListSection.tsx` — collapsed list with reasons.
- `components/EmptyPlaybook.tsx` — empty state.

**Create (Frontend — `src/features/trader-profile/`):**
- `index.ts`, `types.ts`
- `hooks/useTraderProfile.ts`
- `components/TraderProfilePage.tsx` — page-level layout.
- `components/TagFrequencyChart.tsx` — horizontal bar chart (use `recharts` if already in deps; else a simple CSS-bar component).
- `components/PnlByTagHeatmap.tsx` — table with color-coded P&L cells.
- `components/TrendlineCard.tsx` — last-7d vs prior-21d comparison.
- `components/RecentIncidentsList.tsx` — list with date / symbol / tag / observation.

**Create (Frontend — `src/shared/api/`):**
- `assessments.ts` — three command wrappers (`getTradeReview`, `getPlaybook`, `getTraderProfile`).

**Modify (Frontend):**
- Existing router (likely in `src/App.tsx` or a `routes/` dir — locate first) to register the three new routes.
- Existing main shell sidebar / tab bar to add the new entries.
- The existing Trades panel (`src/features/trades/`) — add a "Review →" link in its summary banner.

**Tests (Frontend):**
- `src/features/trade-review/components/TradeReviewCard.test.tsx` — vitest + testing-library, renders all states.
- `src/features/playbook/components/TodaysPlaybook.test.tsx`
- `src/features/trader-profile/components/TraderProfilePage.test.tsx`
- Use fixture data — no real Tauri calls during vitest (per `src/CLAUDE.md`).

## Reuse

- `src/features/trades/` (shipped in retired Phase 3) is the closest peer; mirror its file layout, import patterns, and useTradesForDate hook shape.
- `src/shared/components/ui/` for primitives (Card, Badge, Button, etc.).
- `src/shared/api/` is the single boundary for Tauri command calls. Never call `invoke()` from a component.

## Tasks

### Task 1: Tauri commands

**Files:** `src-tauri/src/ibkr/commands/assessments.rs`, `src-tauri/src/lib.rs`

- [ ] **Step 1: Create the file with three commands**

```rust
//! Tauri commands for the assessment stack. These are the FE-facing
//! counterparts to the MCP tools shipped in Phases 4/5/6 — same shape,
//! same underlying services. Frontend code only ever talks to the
//! backend via these wrappers (and `shared/api/assessments.ts`).

use chrono::NaiveDate;
use std::sync::Arc;
use tauri::State;

use crate::services::playbooks::{Playbook, PlaybookStore};
use crate::services::trade_reviews::store::TradeReviewStore;
use crate::services::trade_reviews::types::TradeReview;
use crate::services::trader_profile::{aggregate, types::TraderProfile};
use crate::storage::Db;
use crate::ibkr::state::IbkrState;

#[tauri::command]
pub async fn get_trade_review(
    date: String,
    account: Option<String>,
    prompt_version: Option<i32>,
    store: State<'_, Arc<TradeReviewStore>>,
    state: State<'_, Arc<IbkrState>>,
) -> Result<Option<TradeReview>, String> {
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let account = resolve_account_for_state(state.inner(), account.as_deref()).await?;
    store
        .read(date, &account, prompt_version)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_today_playbook(
    date: String,
    account: Option<String>,
    generation_id: Option<i32>,
    store: State<'_, Arc<PlaybookStore>>,
    state: State<'_, Arc<IbkrState>>,
) -> Result<Option<Playbook>, String> {
    let date = NaiveDate::parse_from_str(&date, "%Y-%m-%d").map_err(|e| e.to_string())?;
    let account = resolve_account_for_state(state.inner(), account.as_deref()).await?;
    match generation_id {
        Some(g) => store.read_generation(date, &account, g).await,
        None => store.read_latest(date, &account).await,
    }
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn get_trader_profile(
    window_days: Option<u32>,
    account: Option<String>,
    db: State<'_, Arc<Db>>,
    state: State<'_, Arc<IbkrState>>,
) -> Result<TraderProfile, String> {
    let account = resolve_account_for_state(state.inner(), account.as_deref()).await?;
    aggregate(db.inner(), &account, window_days.unwrap_or(30))
        .await
        .map_err(|e| e.to_string())
}

async fn resolve_account_for_state(
    _state: &IbkrState,
    explicit: Option<&str>,
) -> Result<String, String> {
    // Mirror the existing `resolve_account` helper used by trading commands —
    // returns the explicit arg if given, else the sole managed account, else
    // an error listing the available IDs. Look at peer Tauri commands.
    todo!("mirror existing resolve_account in ibkr/commands/")
}
```

- [ ] **Step 2: Register in `lib.rs`**

In `run()`, append to the existing `.invoke_handler(tauri::generate_handler![...])` macro the three new command names: `get_trade_review`, `get_today_playbook`, `get_trader_profile`.

- [ ] **Step 3: cargo check, commit.**

```bash
cd src-tauri && cargo check
```

```bash
git add src-tauri/src/ibkr/commands/assessments.rs src-tauri/src/lib.rs
git commit -m "feat(tauri): assessment Tauri commands wrap MCP-tool services"
```

### Task 2: Frontend API wrappers

**Files:** `src/shared/api/assessments.ts`

- [ ] **Step 1: Implement**

```ts
import { invoke } from "@tauri-apps/api/core";
import type {
  TradeReviewDto,
  PlaybookDto,
  TraderProfileDto,
} from "@/features/trade-review/types";
// (Or import each from its own feature module — pick whichever pattern peer
// `shared/api/*.ts` files use.)

export async function getTradeReview(
  date: string,
  opts?: { account?: string; promptVersion?: number },
): Promise<TradeReviewDto | null> {
  return await invoke("get_trade_review", {
    date,
    account: opts?.account,
    promptVersion: opts?.promptVersion,
  });
}

export async function getPlaybook(
  date: string,
  opts?: { account?: string; generationId?: number },
): Promise<PlaybookDto | null> {
  return await invoke("get_today_playbook", {
    date,
    account: opts?.account,
    generationId: opts?.generationId,
  });
}

export async function getTraderProfile(
  opts?: { windowDays?: number; account?: string },
): Promise<TraderProfileDto> {
  return await invoke("get_trader_profile", {
    windowDays: opts?.windowDays,
    account: opts?.account,
  });
}
```

- [ ] **Step 2: Commit.**

### Task 3: Trade Review feature folder

**Files:** `src/features/trade-review/...`

- [ ] **Step 1: Types** — mirror `TradeReview` Rust DTO field-for-field. The serde format is `snake_case` so the TypeScript type uses `snake_case` too (mirror the existing `Position` / `ExecutionRow` types in peer features).

```ts
// src/features/trade-review/types.ts
export type Grade = "A" | "B" | "C" | "D" | "F";

export type BehavioralTag =
  | "chase_own_exit"
  | "late_otm_lottery"
  | "gamma_window_violation"
  | "single_name_concentration"
  | "position_sizing_ungraduated"
  | "post_loss_revenge"
  | "flat_close"
  | "discipline_on_loser"
  | "scaled_in_winner"
  | "scaled_in_loser"
  | "thesis_match_executed"
  | "off_thesis_trade";

export interface LegObservation {
  leg_id: string;
  observation_md: string;
  tag?: BehavioralTag;
}

export interface LegSummary {
  gross_pnl: number;
  net_pnl: number;
  commissions_total: number;
  n_round_trips: number;
  n_carryover: number;
  win_rate: number | null;
  by_symbol: Record<string, number>;
}

export interface TradeReviewDto {
  date: string;
  account: string;
  prompt_version: number;
  generated_at: string;
  grade: Grade;
  grade_score: number;
  summary: LegSummary;
  behavioral_tags: BehavioralTag[];
  leg_observations: LegObservation[];
  narrative_md: string;
  llm_call_id: string | null;
}
```

- [ ] **Step 2: Hook**

```ts
// src/features/trade-review/hooks/useTradeReview.ts
import { useQuery } from "@tanstack/react-query"; // if peer features use react-query; else useState/useEffect
import { getTradeReview } from "@/shared/api/assessments";

export function useTradeReview(date: string, account?: string) {
  return useQuery({
    queryKey: ["trade-review", date, account],
    queryFn: () => getTradeReview(date, { account }),
    staleTime: 60_000,
  });
}
```

(Substitute the data-fetching idiom in use elsewhere in the codebase; if peer features use `useState`/`useEffect`, mirror that.)

- [ ] **Step 3: Components**

```tsx
// src/features/trade-review/components/TradeReviewCard.tsx
import { useTradeReview } from "../hooks/useTradeReview";
import { GradeBadge } from "./GradeBadge";
import { BehavioralTagChip } from "./BehavioralTagChip";
import { LegObservationsList } from "./LegObservationsList";
import { EmptyTradeReview } from "./EmptyTradeReview";
import { Card, CardHeader, CardContent, CardTitle } from "@/shared/components/ui/card";

export function TradeReviewCard({ date, account }: { date: string; account?: string }) {
  const { data, isLoading, isError, error } = useTradeReview(date, account);
  if (isLoading) return <Card><CardContent>Loading review for {date}…</CardContent></Card>;
  if (isError) return <Card><CardContent>Failed to load: {String(error)}</CardContent></Card>;
  if (!data) return <EmptyTradeReview date={date} />;
  return (
    <Card>
      <CardHeader>
        <CardTitle>
          Trade review {data.date}
          <GradeBadge grade={data.grade} score={data.grade_score} />
        </CardTitle>
      </CardHeader>
      <CardContent>
        <SummaryRow summary={data.summary} />
        <TagsRow tags={data.behavioral_tags} />
        <Narrative md={data.narrative_md} />
        <LegObservationsList items={data.leg_observations} />
      </CardContent>
    </Card>
  );
}

function SummaryRow({ summary }: { summary: LegSummary }) { /* net P&L, commissions, win rate */ }
function TagsRow({ tags }: { tags: BehavioralTag[] }) { return tags.map(t => <BehavioralTagChip key={t} tag={t} />); }
function Narrative({ md }: { md: string }) { /* render markdown via existing renderer used in features/journal/ if it exists, else plain <pre> */ }
```

- [ ] **Step 4: Empty state**

```tsx
// src/features/trade-review/components/EmptyTradeReview.tsx
export function EmptyTradeReview({ date }: { date: string }) {
  return (
    <Card>
      <CardContent className="py-12 text-center text-muted-foreground">
        No trade review for {date} yet.
        <div className="text-xs mt-2">
          Reviews are written automatically at 17:00 ET. Check back after market close,
          or run <code>uv run qk-eod-review --date {date}</code> manually.
        </div>
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 5: Route**

Add `/review/:date` (and a default `/review` → today). Mirror how the existing trades route was registered.

- [ ] **Step 6: Vitest**

```tsx
// src/features/trade-review/components/TradeReviewCard.test.tsx
import { render, screen } from "@testing-library/react";
import { describe, it, expect, vi } from "vitest";
import { TradeReviewCard } from "./TradeReviewCard";

vi.mock("@/shared/api/assessments", () => ({
  getTradeReview: vi.fn(async (date) => null), // empty by default
}));

describe("TradeReviewCard", () => {
  it("renders empty state when no review exists", async () => {
    render(<TradeReviewCard date="2026-05-04" />);
    expect(await screen.findByText(/No trade review for 2026-05-04/i)).toBeInTheDocument();
  });

  it("renders grade and net P&L when review exists", async () => {
    const { getTradeReview } = await import("@/shared/api/assessments");
    (getTradeReview as any).mockResolvedValueOnce({
      date: "2026-05-04",
      account: "U1", prompt_version: 1, generated_at: "2026-05-04T21:00:00Z",
      grade: "B", grade_score: 12.4,
      summary: { gross_pnl: 401.10, net_pnl: 380.0, commissions_total: 21.10, n_round_trips: 3, n_carryover: 0, win_rate: 0.667, by_symbol: { TSLA: 380.0 } },
      behavioral_tags: ["flat_close", "discipline_on_loser", "chase_own_exit"],
      leg_observations: [],
      narrative_md: "Net positive day...",
      llm_call_id: null,
    });
    render(<TradeReviewCard date="2026-05-04" />);
    expect(await screen.findByText(/B/)).toBeInTheDocument();
    expect(await screen.findByText(/380/)).toBeInTheDocument();
  });
});
```

- [ ] **Step 7: Run vitest, commit.**

```bash
pnpm test:run -- src/features/trade-review
```

```bash
git add src/features/trade-review/ src/shared/api/assessments.ts
git commit -m "feat(ui): trade-review feature folder with empty + populated states"
```

### Task 4: Today's Playbook feature folder

**Files:** `src/features/playbook/...`

- [ ] **Step 1: Types** — mirror `Playbook`, `RankedSetup`, `SkipEntry`.

- [ ] **Step 2: Hook** — `usePlaybook(date)`.

- [ ] **Step 3: Components**

```tsx
// src/features/playbook/components/TodaysPlaybook.tsx
export function TodaysPlaybook({ date, account }: { date: string; account?: string }) {
  const { data, isLoading, isError, error } = usePlaybook(date, account);
  if (isLoading) return <Card><CardContent>Loading playbook…</CardContent></Card>;
  if (isError) return <Card><CardContent>Failed: {String(error)}</CardContent></Card>;
  if (!data) return <EmptyPlaybook date={date} />;
  return (
    <div className="space-y-4">
      <div className="flex justify-between items-center">
        <h2 className="text-xl font-semibold">Playbook {data.date}</h2>
        <span className="text-xs text-muted-foreground">
          generated {new Date(data.generated_at).toLocaleTimeString()} (gen #{data.generation_id})
        </span>
      </div>
      {data.ranked_setups.length === 0 ? (
        <Card><CardContent>No A/B-conviction setups today.</CardContent></Card>
      ) : (
        data.ranked_setups.map((s, i) => <RankedSetupCard key={i} setup={s} />)
      )}
      {data.skip_list.length > 0 && <SkipListSection items={data.skip_list} />}
    </div>
  );
}
```

```tsx
// src/features/playbook/components/RankedSetupCard.tsx
export function RankedSetupCard({ setup }: { setup: RankedSetup }) {
  return (
    <Card>
      <CardHeader>
        <CardTitle className="flex items-center gap-2">
          <ConvictionBadge conviction={setup.conviction} />
          {setup.symbol}
          <span className="text-xs text-muted-foreground">{setup.bias}</span>
        </CardTitle>
      </CardHeader>
      <CardContent className="space-y-2 text-sm">
        <Row label="Trigger" value={setup.trigger} />
        <Row label="Entry" value={setup.entry} />
        <Row label="Invalidation" value={setup.invalidation} className="text-red-600 dark:text-red-400" />
        <Row label="Target 1" value={setup.target_1} />
        {setup.target_2 && <Row label="Target 2" value={setup.target_2} />}
        <div className="prose prose-sm dark:prose-invert mt-2">
          {/* render setup.rationale_md */}
        </div>
      </CardContent>
    </Card>
  );
}
```

- [ ] **Step 4: Empty state, route, vitest, commit.**

### Task 5: Trader Profile feature folder

**Files:** `src/features/trader-profile/...`

- [ ] **Step 1: Types** — mirror `TraderProfile`, `TagFrequency`, `PnlByTag`, `Trendline`, `RecentIncident`.

- [ ] **Step 2: Hook** — `useTraderProfile(windowDays?)`.

- [ ] **Step 3: Components**

- `TraderProfilePage.tsx` — top-level layout with `TagFrequencyChart`, `PnlByTagHeatmap`, `TrendlineCard`, `RecentIncidentsList`.
- `TagFrequencyChart.tsx` — horizontal bar list, sorted by count, color-coded by tag weight (positive = green, negative = red — derive from a JS `TAG_WEIGHTS` constant mirrored from Rust).
- `PnlByTagHeatmap.tsx` — table: tag | n_days | total P&L | avg per day, with cell color tied to value sign.
- `TrendlineCard.tsx` — two-column comparison of last 7d vs prior 21d (n reviews, net P&L, avg grade score, top tag).
- `RecentIncidentsList.tsx` — date-grouped list with symbol, tag chip, leg observation excerpt.

- [ ] **Step 4: Mirror Rust tag weights in TS** (same drift risk as the Rust↔Python mirror). Add a small mirror-test in vitest if peer FE features have a precedent for that; otherwise document in the file header.

- [ ] **Step 5: Route, vitest, commit.**

### Task 6: Hook the Trades panel into the review

**Files:** `src/features/trades/components/TradesPage.tsx` (or wherever the summary banner lives)

- [ ] Add a "Review →" anchor in the summary banner that navigates to `/review/:date` for the currently selected date. One-line change, plus a router import.
- [ ] Commit.

### Task 7: Sidebar / nav

**Files:** wherever the main shell's nav lives (`src/App.tsx` or `src/shared/components/layout/Sidebar.tsx`)

- [ ] Add three nav entries: "Today's Playbook" → `/playbook/today`; "Trade Review" → `/review/today`; "Trader Profile" → `/profile`.
- [ ] Mirror the icon convention used by existing entries (lucide-react icons, presumably).
- [ ] Commit.

### Task 8: Browser-mocks fixtures

**Files:** `src/test/browser-mocks/`

- [ ] Add fixtures for the three new commands so `pnpm dev:browser` can render the panels without a live Tauri shell. Mirror the existing fixture patterns.
- [ ] Verify by running `pnpm dev:browser` and visiting `/review/today`, `/playbook/today`, `/profile`.
- [ ] Commit.

### Task 9: Manual smoke test

- [ ] Run `pnpm tauri dev` after Phases 4/5/6 have been live for at least one trading day.
- [ ] Open `/review/today` → see the day's review.
- [ ] Open `/playbook/today` → see the morning's setups.
- [ ] Open `/profile` → see the trailing 30-day dashboard.
- [ ] Cross-check: the LLM client (Claude Code) calling `get_trade_review(today)` returns the same row visible in the UI.
- [ ] Document tracer-bullet pass in master plan.

## Exit criteria

- [ ] Three Tauri commands registered, callable from the FE via `shared/api/assessments.ts`.
- [ ] Three feature folders shipped, each with empty / loading / error / populated states.
- [ ] Three new routes work; sidebar / nav surfaces them.
- [ ] Vitest covers the major view states for all three pages.
- [ ] Trades panel "Review →" link navigates correctly.
- [ ] Manual tracer-bullet: open the desktop app, see all three panels populated with live cron-driven data.
- [ ] `pnpm typecheck && pnpm lint && pnpm test:run` all clean.
- [ ] Pre-commit clean.
- [ ] Update master Phase 7 row + this Status header.

## Gotchas

- **`invoke` casing.** Rust `#[tauri::command] fn get_trade_review` is callable from JS as `invoke("get_trade_review", ...)`. Tauri auto-converts arg names from camelCase JS to snake_case Rust by default — but pin via the `args:` keys in the JS call to be explicit (some Tauri versions have different defaults).
- **DTO drift.** TS types are hand-mirrored from Rust DTOs. If a Rust field is renamed and the TS type isn't updated, serde silently drops the unknown field at deserialize. Mitigation: a smoke test that calls each command against a seeded DB and asserts non-empty key fields catches this. Add at minimum one such integration test per command.
- **`null` vs `undefined`.** Serde serializes `Option::None` as JSON `null`. JS has both `null` and `undefined`; the TS types use `T | null`. Don't use `T?:` (which is `T | undefined`) for nullable backend fields — use `T | null`.
- **Date arg format.** Always `YYYY-MM-DD` ET. The default routes use `today` as a sentinel; the page resolves "today" to the actual ET date via a `useTodayET()` hook (or the existing utility from peer features).
- **Markdown rendering.** `narrative_md` and `rationale_md` are markdown. If the codebase already has a markdown renderer (likely in `features/journal/` or `features/research-notes/` if those exist), use it. Otherwise add `react-markdown` to deps and use it minimally — don't roll your own.
- **Color coding.** Tag chips and P&L cells use red/green coding. Respect dark mode. Use the existing color tokens from `tailwind.config.ts`.
- **`recharts` (or absence).** If recharts is already a dep, use it for the bar chart. If not, a simple flex-row of CSS bars (`<div style={{ width: pct + "%" }}>`) is fine — don't add a chart library just for one bar chart.
- **Account picker.** v1 single-account; the page reads the sole managed account via the existing `useAccount` hook (or whatever peer features use). When a real multi-account setup ships, lift selection to the shell. Don't build a per-page picker now.
- **Empty-state copy.** Distinguish:
  - "No review yet for {date}" (cron hasn't run yet for today) — friendly, suggests waiting / manual run.
  - "No fills on {date}" (you didn't trade) — neutral, no action.
  - "Profile is empty" (first install, no reviews) — onboarding-style copy explaining the system needs time to learn.
- **Performance.** None of these views are large; React Query (or whatever data lib) caches them with a 60s stale time. Don't precompute / preload — the user opens the page on demand.
- **Forward-only history.** A date picker on the Review page that lets the user browse "yesterday" / "two days ago" works only as far back as Phase 4 has been writing. Cap the date picker's lower bound at the first non-null `day_reviews.date` for the account.
