"""Unit tests for `agent/playbook.py`."""

from __future__ import annotations

from playbook import (
    CONVICTION,
    PLAYBOOK_TOOL_NAME,
    RANKED_SETUPS_TOOL_SCHEMA,
    SETUP_BIAS,
    format_playbook_prompt,
    parse_tool_response,
)


# ---- Schema sanity ----------------------------------------------------------


def test_setup_bias_enum():
    assert SETUP_BIAS == ("long", "short")


def test_conviction_enum():
    assert CONVICTION == ("A", "B", "C")


def test_tool_name_pinned():
    assert PLAYBOOK_TOOL_NAME == "submit_playbook"
    assert RANKED_SETUPS_TOOL_SCHEMA["name"] == "submit_playbook"


def test_tool_schema_setup_required_fields():
    setup_schema = RANKED_SETUPS_TOOL_SCHEMA["input_schema"]["properties"][
        "ranked_setups"
    ]["items"]
    required = setup_schema["required"]
    for field in (
        "symbol",
        "bias",
        "trigger",
        "entry",
        "invalidation",
        "target_1",
        "conviction",
        "rationale_md",
    ):
        assert field in required, f"missing required field {field}"
    # target_2 is optional.
    assert "target_2" not in required


def test_tool_schema_skip_required_fields():
    skip_schema = RANKED_SETUPS_TOOL_SCHEMA["input_schema"]["properties"][
        "skip_list"
    ]["items"]
    assert set(skip_schema["required"]) == {"symbol", "reason"}


def test_tool_schema_enums_match_module_constants():
    setup_schema = RANKED_SETUPS_TOOL_SCHEMA["input_schema"]["properties"][
        "ranked_setups"
    ]["items"]["properties"]
    assert tuple(setup_schema["bias"]["enum"]) == SETUP_BIAS
    assert tuple(setup_schema["conviction"]["enum"]) == CONVICTION


# ---- parse_tool_response ----------------------------------------------------


def _ok_setup(symbol: str = "TSLA") -> dict:
    return {
        "symbol": symbol,
        "bias": "long",
        "trigger": "reclaim HOD",
        "entry": "100",
        "invalidation": "lose 95",
        "target_1": "110",
        "target_2": "120",
        "conviction": "A",
        "rationale_md": "good base",
        "evidence_refs": [{"source": "news", "note": "8-K"}],
    }


def test_parse_keeps_well_formed_setup_and_skip_entries():
    raw = {
        "ranked_setups": [_ok_setup("tsla")],
        "skip_list": [{"symbol": "aapl", "reason": "earnings AMC"}],
    }
    parsed = parse_tool_response(raw)
    assert len(parsed["ranked_setups"]) == 1
    s = parsed["ranked_setups"][0]
    assert s["symbol"] == "TSLA"  # upper-cased
    assert s["bias"] == "long"
    assert s["target_2"] == "120"
    assert s["evidence_refs"] == [{"source": "news", "note": "8-K"}]
    assert parsed["skip_list"] == [{"symbol": "AAPL", "reason": "earnings AMC"}]


def test_parse_drops_setups_with_unknown_bias_or_conviction():
    bad_bias = _ok_setup()
    bad_bias["bias"] = "neutral"  # not in SETUP_BIAS
    bad_conv = _ok_setup()
    bad_conv["conviction"] = "S"  # not in CONVICTION
    raw = {"ranked_setups": [bad_bias, bad_conv, _ok_setup()], "skip_list": []}
    parsed = parse_tool_response(raw)
    assert len(parsed["ranked_setups"]) == 1


def test_parse_drops_setups_missing_required_fields():
    incomplete = _ok_setup()
    del incomplete["target_1"]
    raw = {"ranked_setups": [incomplete], "skip_list": []}
    parsed = parse_tool_response(raw)
    assert parsed["ranked_setups"] == []


def test_parse_strips_target_2_when_blank():
    s = _ok_setup()
    s["target_2"] = "  "
    raw = {"ranked_setups": [s], "skip_list": []}
    parsed = parse_tool_response(raw)
    assert "target_2" not in parsed["ranked_setups"][0]


def test_parse_drops_malformed_skip_entries():
    raw = {
        "ranked_setups": [],
        "skip_list": [
            {"symbol": "AAPL", "reason": "ok"},
            {"symbol": 42, "reason": "x"},
            "not a mapping",
            {"reason": "missing symbol"},
        ],
    }
    parsed = parse_tool_response(raw)
    assert parsed["skip_list"] == [{"symbol": "AAPL", "reason": "ok"}]


def test_parse_handles_empty_input():
    parsed = parse_tool_response({})
    assert parsed["ranked_setups"] == []
    assert parsed["skip_list"] == []


# ---- format_playbook_prompt -------------------------------------------------


class _FakeBundle:
    def __init__(self, label: str) -> None:
        self.label = label

    def as_prompt_block(self) -> str:
        return f"## {self.label} block"


def test_format_prompt_renders_bundles_and_menus():
    bundles = [_FakeBundle("TSLA"), _FakeBundle("AAPL")]
    body = format_playbook_prompt(pack_date="2026-05-05", bundles=bundles)
    assert "PACK DATE: 2026-05-05" in body
    assert "## TSLA block" in body
    assert "## AAPL block" in body
    assert "BIAS MENU: long, short" in body
    assert "CONVICTION MENU: A, B, C" in body
    assert "submit_playbook" in body


def test_format_prompt_includes_trader_profile_placeholder_when_none():
    body = format_playbook_prompt(pack_date="2026-05-05", bundles=[])
    assert "TRADER PROFILE" in body
    assert "no profile available" in body


def test_format_prompt_renders_trader_profile_when_provided():
    profile = {
        "account": "U1",
        "window_days": 14,
        "since_date": "2026-04-21",
        "n_reviews": 8,
        "tag_frequencies": [
            {"tag": "flat_close", "count": 5, "pct_of_reviews": 5 / 8},
            {"tag": "chase_own_exit", "count": 3, "pct_of_reviews": 3 / 8},
        ],
        "pnl_by_tag": [],
        "trendline": {
            "last_7d": {
                "n_reviews": 5,
                "tag_counts": {"chase_own_exit": 3, "flat_close": 5},
                "net_pnl": -200.0,
                "avg_grade_score": -2.0,
            },
            "prior_21d": {
                "n_reviews": 3,
                "tag_counts": {},
                "net_pnl": 150.0,
                "avg_grade_score": 4.5,
            },
        },
        "recent_incidents": [
            {
                "date": "2026-05-01",
                "symbol": "TSLA",
                "tag": "chase_own_exit",
                "leg_observation": "Re-entered 395C at $2.50 within 2 min of selling at $2.45",
            }
        ],
    }
    body = format_playbook_prompt(
        pack_date="2026-05-05", bundles=[], trader_profile=profile
    )
    assert "reviews considered: 8 (last 14d, since 2026-04-21)" in body
    assert "flat_close: 5" in body
    assert "chase_own_exit: 3" in body
    assert "62% of reviews" in body  # 5/8 → 62.5% formatted to 62
    assert "trend: last 7d net P&L $-200" in body
    assert "prior 21d net P&L $150" in body
    assert "2026-05-01 TSLA chase_own_exit" in body
    assert "Re-entered 395C" in body


def test_format_prompt_is_deterministic():
    bundles = [_FakeBundle("TSLA")]
    a = format_playbook_prompt(pack_date="2026-05-05", bundles=bundles)
    b = format_playbook_prompt(pack_date="2026-05-05", bundles=bundles)
    assert a == b
