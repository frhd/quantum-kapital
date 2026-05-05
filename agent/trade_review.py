"""Trade review module — mirrors the Rust `BehavioralTag` enum and
provides helpers for the EOD review's trade-review extension.

The closed `BEHAVIORAL_TAGS` tuple here is pinned against the Rust enum
in `services/trade_reviews/tags.rs` by `tests/test_tag_mirror.py`. Don't
add a value here without also adding it to Rust (and vice versa).
"""

from __future__ import annotations

from typing import Any, Mapping, Sequence


# Mirror of services/trade_reviews/tags.rs::BehavioralTag.
BEHAVIORAL_TAGS: tuple[str, ...] = (
    "chase_own_exit",
    "late_otm_lottery",
    "gamma_window_violation",
    "single_name_concentration",
    "position_sizing_ungraduated",
    "post_loss_revenge",
    "flat_close",
    "discipline_on_loser",
    "scaled_in_winner",
    "scaled_in_loser",
    "thesis_match_executed",
    "off_thesis_trade",
)


# Prompt-version sentinel. Bump when (a) the rubric weights change in
# Rust, (b) the tag enum gains/loses a value, OR (c) the system prompt
# in `prompts/trade_review.md` changes materially. Old reviews stay
# queryable; new versions UPSERT as separate rows.
PROMPT_VERSION: int = 1


# ---- LegSummary helpers -----------------------------------------------------


def leg_summary_from_legs(legs: Sequence[Mapping[str, Any]]) -> dict[str, Any]:
    """Compute the LegSummary the LLM consumes.

    The server-side `write_trade_review` re-computes the grade from this
    summary + the chosen tags, so we don't need to send a grade. The
    summary is also forwarded verbatim into the persisted row.

    Each `leg` is one `TradeLeg` row from `get_trade_legs(date)`. The
    keys we read mirror the Rust DTO: `symbol`, `gross_pnl`,
    `commission_total`, `net_pnl`, `tags` (`["round_trip"|"carryover"|...]`).
    """
    gross = 0.0
    net = 0.0
    commissions = 0.0
    n_round = 0
    n_carry = 0
    closed_legs: list[Mapping[str, Any]] = []
    by_symbol: dict[str, float] = {}

    for leg in legs:
        if not isinstance(leg, Mapping):
            continue
        gross += _f(leg.get("gross_pnl"))
        commissions += _f(leg.get("commission_total"))
        net += _f(leg.get("net_pnl"))
        leg_tags = leg.get("tags") or []
        if "round_trip" in leg_tags:
            n_round += 1
            closed_legs.append(leg)
        if "carryover" in leg_tags:
            n_carry += 1
        sym = str(leg.get("symbol", "")).upper()
        by_symbol[sym] = by_symbol.get(sym, 0.0) + _f(leg.get("net_pnl"))

    win_rate: float | None
    if closed_legs:
        winners = sum(1 for l in closed_legs if _f(l.get("net_pnl")) > 0)
        win_rate = winners / len(closed_legs)
    else:
        win_rate = None

    return {
        "gross_pnl": gross,
        "net_pnl": net,
        "commissions_total": commissions,
        "n_round_trips": n_round,
        "n_carryover": n_carry,
        "win_rate": win_rate,
        "by_symbol": by_symbol,
    }


def _f(v: Any) -> float:
    try:
        return float(v) if v is not None else 0.0
    except (TypeError, ValueError):
        return 0.0


# ---- LLM tool schema --------------------------------------------------------


TRADE_REVIEW_TOOL_NAME = "submit_trade_review"


TRADE_REVIEW_TOOL_SCHEMA: dict[str, Any] = {
    "name": TRADE_REVIEW_TOOL_NAME,
    "description": (
        "Pick behavioral tags from the closed enum and write a narrative "
        "scoring today's fills. Do not pass a grade — the server computes "
        "it deterministically from the summary + your tags."
    ),
    "input_schema": {
        "type": "object",
        "properties": {
            "behavioral_tags": {
                "type": "array",
                "items": {"type": "string", "enum": list(BEHAVIORAL_TAGS)},
                "description": (
                    "Closed enum — pick only from the listed values. "
                    "Empty list is allowed for an unremarkable day."
                ),
            },
            "leg_observations": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "leg_id": {"type": "string"},
                        "observation_md": {"type": "string"},
                        "tag": {
                            "type": "string",
                            "enum": list(BEHAVIORAL_TAGS),
                        },
                    },
                    "required": ["leg_id", "observation_md"],
                    "additionalProperties": False,
                },
                "description": (
                    "1–3 most consequential legs of the day. Include the "
                    "biggest winner, biggest loser, and any leg that fired "
                    "a behavioral tag. Each observation is 1–2 sentences."
                ),
            },
            "narrative_md": {
                "type": "string",
                "description": (
                    "200–400 words of markdown commentary. No front-matter, "
                    "no fenced wrappers, no headers above ###."
                ),
            },
        },
        "required": ["behavioral_tags", "narrative_md"],
        "additionalProperties": False,
    },
}


def parse_tool_response(tool_input: Mapping[str, Any]) -> dict[str, Any]:
    """Validate + normalise the forced-tool response. Drops unknown tag
    values defensively (the schema enum should already prevent this; the
    extra check is belt-and-braces against an LLM that ignored the enum)."""
    raw_tags = tool_input.get("behavioral_tags") or []
    tags = [t for t in raw_tags if isinstance(t, str) and t in BEHAVIORAL_TAGS]
    raw_observations = tool_input.get("leg_observations") or []
    observations: list[dict[str, Any]] = []
    for obs in raw_observations:
        if not isinstance(obs, Mapping):
            continue
        leg_id = obs.get("leg_id")
        observation_md = obs.get("observation_md")
        if not isinstance(leg_id, str) or not isinstance(observation_md, str):
            continue
        clean: dict[str, Any] = {
            "leg_id": leg_id,
            "observation_md": observation_md,
        }
        tag = obs.get("tag")
        if isinstance(tag, str) and tag in BEHAVIORAL_TAGS:
            clean["tag"] = tag
        observations.append(clean)
    narrative = tool_input.get("narrative_md") or ""
    if not isinstance(narrative, str):
        narrative = str(narrative)
    return {
        "behavioral_tags": tags,
        "leg_observations": observations,
        "narrative_md": narrative.strip(),
    }


# ---- Prompt formatting -----------------------------------------------------


def format_trade_review_prompt(
    *,
    pack_date: str,
    legs: Sequence[Mapping[str, Any]],
    summary: Mapping[str, Any],
    pack_ideas: Sequence[Mapping[str, Any]] = (),
) -> str:
    """Render the user message body the LLM sees for the trade-review call.

    The output is plain text — markdown that the LLM will mirror back in
    `narrative_md`. Keep it terse: total prompt budget ~1500 input tokens.
    """
    lines: list[str] = []
    lines.append(f"PACK DATE: {pack_date}")
    lines.append("")
    lines.append("DAY SUMMARY")
    lines.append("-----------")
    lines.append(f"  net_pnl:       ${_f(summary.get('net_pnl')):.2f}")
    lines.append(f"  gross_pnl:     ${_f(summary.get('gross_pnl')):.2f}")
    lines.append(
        f"  commissions:   ${_f(summary.get('commissions_total')):.2f}"
    )
    n_round = int(summary.get("n_round_trips") or 0)
    n_carry = int(summary.get("n_carryover") or 0)
    lines.append(f"  round_trips:   {n_round}")
    lines.append(f"  carryover:     {n_carry}")
    win_rate = summary.get("win_rate")
    if win_rate is not None:
        lines.append(f"  win_rate:      {float(win_rate) * 100:.1f}%")
    by_symbol = summary.get("by_symbol") or {}
    if isinstance(by_symbol, Mapping) and by_symbol:
        lines.append("  by_symbol:")
        for sym, pnl in sorted(by_symbol.items()):
            lines.append(f"    {sym}: ${_f(pnl):+.2f}")
    lines.append("")

    lines.append("LEGS")
    lines.append("----")
    for i, leg in enumerate(legs, start=1):
        if not isinstance(leg, Mapping):
            continue
        leg_id = leg.get("leg_id", f"leg_{i}")
        sym = str(leg.get("symbol", "?")).upper()
        opened_at = leg.get("opened_at")
        closed_at = leg.get("closed_at")
        net = _f(leg.get("net_pnl"))
        tags = leg.get("tags") or []
        hold_min = leg.get("hold_minutes")
        kind = "round-trip" if "round_trip" in tags else (
            "carryover" if "carryover" in tags else "open"
        )
        lines.append(
            f"  {i}. {leg_id} {sym} ({kind}) net=${net:+.2f}"
        )
        if opened_at:
            lines.append(f"     opened: {opened_at}")
        if closed_at:
            lines.append(f"     closed: {closed_at}")
        if hold_min is not None:
            lines.append(f"     held:   {hold_min}m")
        if tags:
            lines.append(f"     tags:   {','.join(tags)}")
    lines.append("")

    if pack_ideas:
        lines.append("MORNING PLAYBOOK (today's pack)")
        lines.append("-------------------------------")
        for idea in pack_ideas:
            if not isinstance(idea, Mapping):
                continue
            sym = str(idea.get("symbol", "?")).upper()
            conviction = idea.get("conviction") or "-"
            lines.append(f"  - {sym} ({conviction})")
        lines.append("")

    lines.append("BEHAVIORAL TAG MENU (pick zero or more from this closed enum):")
    for tag in BEHAVIORAL_TAGS:
        lines.append(f"  - {tag}")
    lines.append("")
    lines.append(
        "Call `submit_trade_review` with `behavioral_tags`, "
        "`leg_observations` (1–3 most consequential legs), and "
        "`narrative_md` (200–400 words). DO NOT pick a grade — the "
        "server computes it from the summary + your tags."
    )

    return "\n".join(lines)
