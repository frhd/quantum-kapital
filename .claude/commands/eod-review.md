---
description: Run the EOD trade review for a given date via the quantum-kapital MCP server (default — yesterday ET). Replaces `uv run qk-eod-review` when running interactively.
argument-hint: "[YYYY-MM-DD]"
---

# /eod-review

You are running the post-close trade review interactively, in place of `agent/eod_review.py`. The Tauri app must be running so the `quantum-kapital` MCP server is reachable.

**Surveillance-only.** No order placement, sizing, or exit recommendations.

## Step 0 — Resolve the date

Argument: `$ARGUMENTS`

- If non-empty and matches `YYYY-MM-DD`, use it.
- Else, use yesterday's ET trading day (skip weekends — if today ET is Monday, use last Friday).

## Step 1 — Read the canonical prompt and tag rules

Read these two files; they are the source of truth and override anything below if they conflict:

1. `agent/prompts/trade_review.md` — the system prompt the Python agent uses. Follow it.
2. `agent/trade_review.py` — defines `BEHAVIORAL_TAGS` (the closed enum), `PROMPT_VERSION`, and `leg_summary_from_legs` (the deterministic summary computation). Mirror these rules exactly.

## Step 2 — Resolve the account

Call `mcp__quantum-kapital__get_account_summary` with no args, or `list_accounts` if available, to discover the active account id. If multiple, ask the user which one.

## Step 3 — Fetch inputs via MCP (read-only)

Call concurrently where possible:

- `mcp__quantum-kapital__get_trade_legs({ date, account })` — already FIFO-matched legs.
- `mcp__quantum-kapital__get_today_playbook({ date })` then fall back to `get_morning_pack({ date })` — for thesis-match / off-thesis context.
- `mcp__quantum-kapital__get_trader_profile({ window_days: 30 })` — for behavioral context (recent incidents).

If `get_trade_legs` returns zero legs, stop and report "No fills for {date} — nothing to review." Do **not** write a row.

## Step 4 — Build `summary` deterministically

Mirror `leg_summary_from_legs` in `agent/trade_review.py`:

- `gross_pnl`, `net_pnl`, `commissions_total`: sum across legs.
- `n_round_trips`: count legs whose `tags` contain `round_trip`.
- `n_carryover`: count legs whose `tags` contain `carryover`.
- `win_rate`: round-trip winners / round-trip count, or `null` if none closed.
- `by_symbol`: per-symbol `net_pnl` sum.

**Trust these numbers** — do not recompute differently from this rule.

## Step 5 — Pick `behavioral_tags`

From the closed enum only (`agent/trade_review.py::BEHAVIORAL_TAGS`):

```
chase_own_exit, late_otm_lottery, gamma_window_violation,
single_name_concentration, position_sizing_ungraduated,
post_loss_revenge, flat_close, discipline_on_loser,
scaled_in_winner, scaled_in_loser, thesis_match_executed,
off_thesis_trade
```

Apply each tag literally per the rules in `agent/prompts/trade_review.md`. Empty list is fine for an unremarkable day. The MCP boundary will reject unknown values.

## Step 6 — Write `leg_observations`

1–3 of the most consequential legs (biggest winner, biggest loser, any leg that fired a tag). Each: 1–2 sentences citing `leg_id`. Tie back to a tag where applicable.

## Step 7 — Write `narrative_md`

200–400 words, markdown only (no front-matter, no fenced wrappers, no headers above `###`). Cover: (a) the day's net P&L and shape; (b) what worked; (c) what didn't; (d) one or two notes on what to watch tomorrow. Do **not** issue a letter grade — the server computes it from `(summary, behavioral_tags)`.

## Step 8 — Persist via MCP

Call `mcp__quantum-kapital__write_trade_review` with:

```json
{
  "date": "<YYYY-MM-DD>",
  "account": "<account>",
  "prompt_version": 1,
  "summary": { ...from Step 4 },
  "behavioral_tags": [ ...from Step 5 ],
  "leg_observations": [ ...from Step 6 ],
  "narrative_md": "<from Step 7>"
}
```

Keep `prompt_version: 1` to match `agent/trade_review.py::PROMPT_VERSION` so this row UPSERTs the same slot as the Python cron's row. Bump only if you change the rubric or system prompt materially.

The server response includes the deterministic `grade` and `score` — surface those to the user.

## Step 9 — Mirror to journal (optional but standard)

Call `mcp__quantum-kapital__append_journal_entry` with:

- `journal_date`: same date
- `section`: `"EOD Review (Agent)"`
- `body_md`: the same `narrative_md` from Step 7 (so `/journal` renders it verbatim under the EOD Review section).

## Step 10 — Report back

One short summary: date, account, grade, net P&L, tag count, narrative excerpt (≤2 lines). Note that the Trade Review card in the UI should now populate for that date.

## What this does NOT do

- It does **not** consume the `LlmService`/`BudgetGuard` ledgers — your Claude Code subscription pays for the reasoning, not the daily USD budget. The kill-switch protections of the Python path are bypassed; that's the deliberate trade-off for running this manually.
- It does **not** place orders. Read-only + audit-tracked write rails only.
