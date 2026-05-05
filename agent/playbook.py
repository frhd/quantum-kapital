"""Playbook module — schemas + helpers for the morning sweep's
playbook extension.

The closed `SETUP_BIAS` / `CONVICTION` enums here mirror the Rust
`SetupBias` / `Conviction` types in `services/playbooks/types.rs`. The
Rust `write_playbook` rail validates payloads at the MCP boundary, so
any drift fails loudly rather than silently storing junk.

Schema-drift checklist when changing `RankedSetup` / `SkipEntry`:
  1. Rust types: `services/playbooks/types.rs`.
  2. Python schema: `RANKED_SETUPS_TOOL_SCHEMA` below.
  3. Round-trip test: `services/playbooks/tests.rs::playbook_serde_round_trip_*`.
"""

from __future__ import annotations

from typing import Any, Mapping, Sequence


# Mirror of services/playbooks/types.rs::SetupBias.
SETUP_BIAS: tuple[str, ...] = ("long", "short")

# Mirror of services/playbooks/types.rs::Conviction.
CONVICTION: tuple[str, ...] = ("A", "B", "C")


# ---- LLM tool schema --------------------------------------------------------


PLAYBOOK_TOOL_NAME = "submit_playbook"


RANKED_SETUPS_TOOL_SCHEMA: dict[str, Any] = {
    "name": PLAYBOOK_TOOL_NAME,
    "description": (
        "Emit ranked, actionable setups for today plus an explicit skip "
        "list. Each setup must carry a precise trigger, entry, "
        "invalidation, and at least one target. Skip-list entries name "
        "symbols to AVOID with a one-line reason."
    ),
    "input_schema": {
        "type": "object",
        "properties": {
            "ranked_setups": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "bias": {"type": "string", "enum": list(SETUP_BIAS)},
                        "trigger": {"type": "string"},
                        "entry": {"type": "string"},
                        "invalidation": {"type": "string"},
                        "target_1": {"type": "string"},
                        "target_2": {"type": "string"},
                        "conviction": {
                            "type": "string",
                            "enum": list(CONVICTION),
                        },
                        "rationale_md": {"type": "string"},
                        "evidence_refs": {
                            "type": "array",
                            "items": {
                                "type": "object",
                                "properties": {
                                    "source": {"type": "string"},
                                    "note": {"type": "string"},
                                },
                                "required": ["source", "note"],
                                "additionalProperties": False,
                            },
                        },
                    },
                    "required": [
                        "symbol",
                        "bias",
                        "trigger",
                        "entry",
                        "invalidation",
                        "target_1",
                        "conviction",
                        "rationale_md",
                    ],
                    "additionalProperties": False,
                },
            },
            "skip_list": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "reason": {"type": "string"},
                    },
                    "required": ["symbol", "reason"],
                    "additionalProperties": False,
                },
            },
        },
        "required": ["ranked_setups", "skip_list"],
        "additionalProperties": False,
    },
}


def parse_tool_response(tool_input: Mapping[str, Any]) -> dict[str, Any]:
    """Validate + normalise the forced-tool response.

    Drops malformed entries defensively (the schema enum should already
    prevent these; this is belt-and-braces against an LLM that ignored
    the schema).
    """
    raw_setups = tool_input.get("ranked_setups") or []
    setups: list[dict[str, Any]] = []
    for s in raw_setups:
        if not isinstance(s, Mapping):
            continue
        symbol = s.get("symbol")
        bias = s.get("bias")
        trigger = s.get("trigger")
        entry = s.get("entry")
        invalidation = s.get("invalidation")
        target_1 = s.get("target_1")
        conviction = s.get("conviction")
        rationale = s.get("rationale_md")
        if not (
            isinstance(symbol, str)
            and isinstance(bias, str)
            and bias in SETUP_BIAS
            and isinstance(trigger, str)
            and isinstance(entry, str)
            and isinstance(invalidation, str)
            and isinstance(target_1, str)
            and isinstance(conviction, str)
            and conviction in CONVICTION
            and isinstance(rationale, str)
        ):
            continue
        clean: dict[str, Any] = {
            "symbol": symbol.upper(),
            "bias": bias,
            "trigger": trigger,
            "entry": entry,
            "invalidation": invalidation,
            "target_1": target_1,
            "conviction": conviction,
            "rationale_md": rationale,
        }
        target_2 = s.get("target_2")
        if isinstance(target_2, str) and target_2.strip():
            clean["target_2"] = target_2
        evidence = s.get("evidence_refs") or []
        ev_clean: list[dict[str, str]] = []
        for ref in evidence:
            if not isinstance(ref, Mapping):
                continue
            src = ref.get("source")
            note = ref.get("note")
            if isinstance(src, str) and isinstance(note, str):
                ev_clean.append({"source": src, "note": note})
        if ev_clean:
            clean["evidence_refs"] = ev_clean
        setups.append(clean)

    raw_skip = tool_input.get("skip_list") or []
    skip: list[dict[str, str]] = []
    for entry in raw_skip:
        if not isinstance(entry, Mapping):
            continue
        symbol = entry.get("symbol")
        reason = entry.get("reason")
        if isinstance(symbol, str) and isinstance(reason, str):
            skip.append({"symbol": symbol.upper(), "reason": reason})

    return {"ranked_setups": setups, "skip_list": skip}


# ---- Prompt formatting -----------------------------------------------------


def format_playbook_prompt(
    *,
    pack_date: str,
    bundles: Sequence[Any],
    trader_profile: Mapping[str, Any] | None = None,
) -> str:
    """Render the user message body the playbook LLM call sees.

    Deterministic given the same `bundles` + `trader_profile` so an
    LLM-call replay against the same inputs produces the same prompt.
    The `{trader_profile_section}` slot is rendered as a placeholder
    when no profile is provided so wiring Phase 6's real profile in is
    a one-line change here, not a prompt edit.
    """
    lines: list[str] = []
    lines.append(f"PACK DATE: {pack_date}")
    lines.append("")
    lines.append(_render_trader_profile_section(trader_profile))
    lines.append("")
    lines.append("WATCHLIST BRIEFING")
    lines.append("------------------")
    if not bundles:
        lines.append("(empty)")
    else:
        for b in bundles:
            block = getattr(b, "as_prompt_block", None)
            if callable(block):
                lines.append(block())
            else:
                lines.append(str(b))
    lines.append("")
    lines.append("BIAS MENU: " + ", ".join(SETUP_BIAS))
    lines.append("CONVICTION MENU: " + ", ".join(CONVICTION))
    lines.append("")
    lines.append(
        "TASK: Call `submit_playbook` with `ranked_setups` (each with "
        "symbol, bias, trigger, entry, invalidation, target_1, "
        "conviction, rationale_md; target_2 + evidence_refs optional) "
        "and `skip_list` (each {symbol, reason}). Empty ranked_setups "
        "is acceptable on a no-trade day — explain in skip_list."
    )
    return "\n".join(lines)


def _render_trader_profile_section(profile: Mapping[str, Any] | None) -> str:
    """Render the TRADER PROFILE block the playbook LLM sees.

    The profile envelope produced by `get_trader_profile` carries
    `tag_frequencies: list[{tag, count, pct_of_reviews}]`,
    `recent_incidents: list[{date, symbol, tag, leg_observation}]`,
    and a `trendline` with last_7d / prior_21d windows. Defensive
    against missing keys so a partial profile (or a future schema
    extension) doesn't crash the prompt."""
    if profile is None:
        return (
            "TRADER PROFILE\n"
            "--------------\n"
            "(no profile available — first reviews not yet collected)"
        )
    lines: list[str] = ["TRADER PROFILE", "--------------"]
    window = profile.get("window_days")
    n_reviews = profile.get("n_reviews")
    since = profile.get("since_date")
    if n_reviews is not None and window:
        lines.append(f"reviews considered: {n_reviews} (last {window}d, since {since})")

    tags = profile.get("tag_frequencies")
    if isinstance(tags, Sequence) and tags:
        lines.append("most frequent tags:")
        for tf in list(tags)[:6]:
            if not isinstance(tf, Mapping):
                continue
            name = tf.get("tag", "?")
            count = tf.get("count", 0)
            pct = tf.get("pct_of_reviews")
            pct_str = f" ({float(pct) * 100:.0f}% of reviews)" if isinstance(pct, (int, float)) else ""
            lines.append(f"  - {name}: {count}{pct_str}")

    trend = profile.get("trendline")
    if isinstance(trend, Mapping):
        last_7 = trend.get("last_7d") if isinstance(trend.get("last_7d"), Mapping) else None
        prior_21 = trend.get("prior_21d") if isinstance(trend.get("prior_21d"), Mapping) else None
        if last_7 and prior_21:
            lines.append(
                "trend: last 7d net P&L ${:.0f} (avg score {:.1f}); "
                "prior 21d net P&L ${:.0f} (avg score {:.1f})".format(
                    float(last_7.get("net_pnl", 0.0) or 0.0),
                    float(last_7.get("avg_grade_score", 0.0) or 0.0),
                    float(prior_21.get("net_pnl", 0.0) or 0.0),
                    float(prior_21.get("avg_grade_score", 0.0) or 0.0),
                )
            )

    incidents = profile.get("recent_incidents")
    if isinstance(incidents, Sequence) and incidents:
        lines.append("recent behavioral incidents (last 7d):")
        for inc in list(incidents)[:5]:
            if not isinstance(inc, Mapping):
                continue
            sym = inc.get("symbol", "?")
            tag = inc.get("tag", "?")
            date_ = inc.get("date", "?")
            obs = inc.get("leg_observation") or ""
            obs_short = obs if len(obs) <= 120 else obs[:117] + "..."
            line = f"  - {date_} {sym} {tag}"
            if obs_short:
                line += f" — {obs_short}"
            lines.append(line)
    return "\n".join(lines)
