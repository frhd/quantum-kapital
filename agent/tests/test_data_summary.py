"""Data-summary unit tests."""

from __future__ import annotations

import data_summary as ds


def test_summarize_daily_bars_empty():
    assert ds.summarize_daily_bars([]) == "no data"


def test_summarize_daily_bars_extracts_levels():
    bars = [{"close": 100, "high": 102, "low": 99} for _ in range(60)]
    bars[-1] = {"close": 105, "high": 106, "low": 104}
    s = ds.summarize_daily_bars(bars)
    assert "last=105.00" in s
    assert "MA50=" in s
    assert "n_bars=60" in s


def test_summarize_intraday_bars_session_pct():
    bars = [{"close": 100} for _ in range(10)]
    bars[-1] = {"close": 102}
    s = ds.summarize_intraday_bars(bars)
    assert "open=100.00" in s
    assert "close=102.00" in s
    assert "+2.00%" in s


def test_summarize_news_truncates_and_keeps_id():
    items = [
        {"id": 1, "verdict": "bullish", "title": "Big news " * 50},
        {"id": 2, "sentiment": "neutral", "headline": "Quiet day"},
    ]
    s = ds.summarize_news(items)
    assert "[#1 bullish]" in s
    assert "[#2 neutral]" in s
    # Truncated to 120 chars per item plus surrounds.
    assert len(s) < 600


def test_summarize_news_empty():
    assert ds.summarize_news([]) == "none"
    assert ds.summarize_news(None) == "none"


def test_summarize_sentiment_groups_by_source():
    rows = [
        {"source": "stocktwits", "mentions": 50, "bullish": 30, "bearish": 5},
        {"source": "stocktwits", "mentions": 10, "bullish": 8, "bearish": 1},
        {"source": "reddit_wsb", "mentions": 5, "bullish": 2, "bearish": 1},
    ]
    s = ds.summarize_sentiment(rows)
    assert "stocktwits: 60m b38/s6" in s
    assert "reddit_wsb: 5m b2/s1" in s


def test_summarize_setups_keeps_top_5():
    setups = [{"id": i, "kind": "vbo", "status": "armed", "created_at": f"2026-04-{i:02d}"} for i in range(1, 9)]
    s = ds.summarize_setups(setups)
    # Only top 5
    assert "#5" in s
    assert "#6" not in s


def test_summarize_setups_empty():
    assert ds.summarize_setups([]) == "none"
    assert ds.summarize_setups(None) == "none"


def test_summarize_fundamentals_filters_empty_keys():
    payload = {
        "Sector": "Technology",
        "Industry": "",
        "PERatio": "25.5",
        "ProfitMargin": None,
        "MarketCapitalization": "1000000000",
    }
    s = ds.summarize_fundamentals(payload)
    assert "Sector=Technology" in s
    assert "PERatio=25.5" in s
    assert "MarketCapitalization=1000000000" in s
    assert "Industry" not in s
    assert "ProfitMargin" not in s


def test_summarize_fundamentals_none_returns_na():
    assert ds.summarize_fundamentals(None) == "n/a"
    assert ds.summarize_fundamentals({}) == "n/a"


def test_candidate_set_merges_watchlist_and_candidates():
    candidates = [
        {"symbol": "AAPL", "score": 0.9},
        {"symbol": "TSLA", "score": 0.4},
        {"symbol": "weak", "score": 0.05},  # below min_score
    ]
    watchlist = [
        {"symbol": "MSFT"},
        {"symbol": "AAPL"},  # dedupe
    ]
    syms = ds.candidate_set(candidates, watchlist, min_score=0.1, top_k=10)
    assert "AAPL" in syms
    assert "MSFT" in syms
    assert "TSLA" in syms
    assert "WEAK" not in syms  # filtered by min_score


def test_candidate_set_caps_at_top_k():
    candidates = [{"symbol": f"T{i}", "score": 1.0 - i * 0.01} for i in range(20)]
    syms = ds.candidate_set(candidates, [], min_score=0.0, top_k=5)
    assert len(syms) == 5


def test_candidate_set_uppercases():
    syms = ds.candidate_set([{"symbol": "aapl", "score": 0.5}], [], min_score=0.1, top_k=10)
    assert syms == ["AAPL"]
