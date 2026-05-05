"""Unit tests for `agent/trade_review.py`."""

from __future__ import annotations

from trade_review import (
    BEHAVIORAL_TAGS,
    PROMPT_VERSION,
    TRADE_REVIEW_TOOL_SCHEMA,
    format_trade_review_prompt,
    leg_summary_from_legs,
    parse_tool_response,
)


# ---- BEHAVIORAL_TAGS / PROMPT_VERSION sanity checks --------------------------


def test_behavioral_tags_count_matches_rust_enum():
    # The mirror test asserts name-for-name correspondence; this catches
    # accidental list-length drift independently.
    assert len(BEHAVIORAL_TAGS) == 12


def test_behavioral_tags_are_unique_snake_case():
    assert len(set(BEHAVIORAL_TAGS)) == len(BEHAVIORAL_TAGS)
    for t in BEHAVIORAL_TAGS:
        assert t == t.lower()
        assert " " not in t
        assert "-" not in t


def test_prompt_version_is_positive_int():
    assert isinstance(PROMPT_VERSION, int)
    assert PROMPT_VERSION >= 1


def test_tool_schema_enum_matches_behavioral_tags():
    enum = TRADE_REVIEW_TOOL_SCHEMA["input_schema"]["properties"][
        "behavioral_tags"
    ]["items"]["enum"]
    assert tuple(enum) == BEHAVIORAL_TAGS


# ---- leg_summary_from_legs --------------------------------------------------


def _round_trip_leg(symbol: str, net: float, gross: float, commission: float) -> dict:
    return {
        "leg_id": f"L-{symbol}",
        "symbol": symbol,
        "tags": ["round_trip"],
        "net_pnl": net,
        "gross_pnl": gross,
        "commission_total": commission,
    }


def _carryover_leg(symbol: str) -> dict:
    return {
        "leg_id": f"L-{symbol}-carry",
        "symbol": symbol,
        "tags": ["carryover"],
        "net_pnl": 0.0,
        "gross_pnl": 0.0,
        "commission_total": 0.0,
    }


def test_leg_summary_aggregates_round_trips_and_carryover():
    legs = [
        _round_trip_leg("TSLA", 100.0, 110.0, 10.0),
        _round_trip_leg("AAPL", -50.0, -40.0, 10.0),
        _round_trip_leg("TSLA", 200.0, 210.0, 10.0),
        _carryover_leg("MSFT"),
    ]
    s = leg_summary_from_legs(legs)
    assert s["gross_pnl"] == 280.0
    assert s["net_pnl"] == 250.0
    assert s["commissions_total"] == 30.0
    assert s["n_round_trips"] == 3
    assert s["n_carryover"] == 1
    # 2 of 3 round-trips are winners.
    assert abs(s["win_rate"] - (2.0 / 3.0)) < 1e-9
    assert s["by_symbol"] == {"TSLA": 300.0, "AAPL": -50.0, "MSFT": 0.0}


def test_leg_summary_empty_input():
    s = leg_summary_from_legs([])
    assert s["gross_pnl"] == 0.0
    assert s["net_pnl"] == 0.0
    assert s["commissions_total"] == 0.0
    assert s["n_round_trips"] == 0
    assert s["n_carryover"] == 0
    assert s["win_rate"] is None
    assert s["by_symbol"] == {}


def test_leg_summary_no_closed_legs_yields_null_win_rate():
    legs = [_carryover_leg("TSLA"), _carryover_leg("AAPL")]
    s = leg_summary_from_legs(legs)
    assert s["win_rate"] is None
    assert s["n_round_trips"] == 0
    assert s["n_carryover"] == 2


def test_leg_summary_normalises_symbol_case():
    legs = [
        _round_trip_leg("tsla", 100.0, 110.0, 10.0),
        _round_trip_leg("Tsla", 50.0, 60.0, 10.0),
    ]
    s = leg_summary_from_legs(legs)
    # All variants accumulate under the upper-cased ticker.
    assert s["by_symbol"] == {"TSLA": 150.0}


# ---- parse_tool_response ----------------------------------------------------


def test_parse_tool_response_keeps_valid_tags_and_observations():
    raw = {
        "behavioral_tags": ["flat_close", "discipline_on_loser"],
        "leg_observations": [
            {
                "leg_id": "L-1",
                "observation_md": "tightest exit of the week.",
                "tag": "discipline_on_loser",
            }
        ],
        "narrative_md": "  Solid day.  ",
    }
    parsed = parse_tool_response(raw)
    assert parsed["behavioral_tags"] == ["flat_close", "discipline_on_loser"]
    assert parsed["leg_observations"][0]["tag"] == "discipline_on_loser"
    assert parsed["narrative_md"] == "Solid day."


def test_parse_tool_response_drops_unknown_tag_values():
    raw = {
        "behavioral_tags": ["flat_close", "made_up_tag"],
        "leg_observations": [
            {
                "leg_id": "L-1",
                "observation_md": "ok",
                "tag": "made_up_tag",
            }
        ],
        "narrative_md": "x",
    }
    parsed = parse_tool_response(raw)
    assert parsed["behavioral_tags"] == ["flat_close"]
    # Observation kept, but bogus tag dropped.
    assert parsed["leg_observations"][0]["leg_id"] == "L-1"
    assert "tag" not in parsed["leg_observations"][0]


def test_parse_tool_response_drops_malformed_observations():
    raw = {
        "behavioral_tags": [],
        "leg_observations": [
            {"leg_id": "L-1", "observation_md": "ok"},  # valid
            {"leg_id": 42, "observation_md": "bad leg_id"},  # invalid
            "not a mapping",  # invalid
            {"leg_id": "L-2"},  # missing observation_md
        ],
        "narrative_md": "y",
    }
    parsed = parse_tool_response(raw)
    assert len(parsed["leg_observations"]) == 1
    assert parsed["leg_observations"][0]["leg_id"] == "L-1"


def test_parse_tool_response_handles_empty_input():
    parsed = parse_tool_response({"narrative_md": "n"})
    assert parsed["behavioral_tags"] == []
    assert parsed["leg_observations"] == []
    assert parsed["narrative_md"] == "n"


# ---- format_trade_review_prompt --------------------------------------------


def test_format_prompt_renders_summary_and_legs():
    legs = [
        _round_trip_leg("TSLA", 100.0, 110.0, 10.0),
        _carryover_leg("MSFT"),
    ]
    summary = leg_summary_from_legs(legs)
    body = format_trade_review_prompt(
        pack_date="2026-05-04",
        legs=legs,
        summary=summary,
    )
    assert "PACK DATE: 2026-05-04" in body
    assert "DAY SUMMARY" in body
    assert "$100.00" in body  # net_pnl formatted
    assert "round_trips:   1" in body
    assert "carryover:     1" in body
    assert "win_rate:      100.0%" in body
    assert "TSLA" in body
    assert "MSFT" in body
    assert "BEHAVIORAL TAG MENU" in body
    # Every enum value lands in the menu.
    for tag in BEHAVIORAL_TAGS:
        assert tag in body


def test_format_prompt_renders_pack_ideas_when_present():
    body = format_trade_review_prompt(
        pack_date="2026-05-04",
        legs=[],
        summary=leg_summary_from_legs([]),
        pack_ideas=[{"symbol": "TSLA", "conviction": "A"}],
    )
    assert "MORNING PLAYBOOK" in body
    assert "TSLA (A)" in body


def test_format_prompt_omits_playbook_when_no_pack_ideas():
    body = format_trade_review_prompt(
        pack_date="2026-05-04",
        legs=[],
        summary=leg_summary_from_legs([]),
    )
    assert "MORNING PLAYBOOK" not in body
