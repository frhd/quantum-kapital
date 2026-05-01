"""Compress raw MCP tool outputs into compact strings the LLM can fit in
context without burning tokens. Heuristic-only — no charting, no TA library."""

from __future__ import annotations

import json
import statistics
from dataclasses import dataclass
from typing import Any, Mapping, Sequence


@dataclass
class CandidateBundle:
    symbol: str
    daily_summary: str
    intraday_summary: str
    fundamentals_summary: str
    news_summary: str
    sentiment_summary: str
    setups_summary: str

    def as_prompt_block(self) -> str:
        return (
            f"## {self.symbol}\n"
            f"- daily (1y): {self.daily_summary}\n"
            f"- intraday (last RTH 5m): {self.intraday_summary}\n"
            f"- fundamentals: {self.fundamentals_summary}\n"
            f"- news 24h: {self.news_summary}\n"
            f"- sentiment 24h: {self.sentiment_summary}\n"
            f"- recent setups (30d): {self.setups_summary}\n"
        )


def _close(bar: Mapping[str, Any]) -> float | None:
    for key in ("close", "c"):
        if key in bar:
            try:
                return float(bar[key])
            except (TypeError, ValueError):
                return None
    return None


def _high(bar: Mapping[str, Any]) -> float | None:
    for key in ("high", "h"):
        if key in bar:
            try:
                return float(bar[key])
            except (TypeError, ValueError):
                return None
    return None


def _low(bar: Mapping[str, Any]) -> float | None:
    for key in ("low", "l"):
        if key in bar:
            try:
                return float(bar[key])
            except (TypeError, ValueError):
                return None
    return None


def summarize_daily_bars(bars: Sequence[Mapping[str, Any]]) -> str:
    closes = [c for c in (_close(b) for b in bars) if c is not None and c > 0]
    if not closes:
        return "no data"

    last = closes[-1]
    high_52w = max(_high(b) or _close(b) or 0.0 for b in bars[-min(252, len(bars)) :])
    low_52w = min(c for c in (_low(b) or _close(b) for b in bars[-min(252, len(bars)) :]) if c)
    pct_from_high = (last - high_52w) / high_52w * 100.0 if high_52w else 0.0
    ma50 = statistics.fmean(closes[-50:]) if len(closes) >= 50 else None
    ma200 = statistics.fmean(closes[-200:]) if len(closes) >= 200 else None

    def fmt(v: float | None) -> str:
        return f"{v:.2f}" if v is not None else "n/a"

    return (
        f"last={last:.2f} 52w_hi={high_52w:.2f} 52w_lo={low_52w:.2f} "
        f"pct_from_hi={pct_from_high:+.1f}% MA50={fmt(ma50)} MA200={fmt(ma200)} "
        f"n_bars={len(closes)}"
    )


def summarize_intraday_bars(bars: Sequence[Mapping[str, Any]]) -> str:
    closes = [c for c in (_close(b) for b in bars) if c is not None and c > 0]
    if not closes:
        return "no data"
    first, last = closes[0], closes[-1]
    high = max(_high(b) or _close(b) or 0.0 for b in bars)
    low = min(c for c in (_low(b) or _close(b) for b in bars) if c)
    pct = (last - first) / first * 100.0 if first else 0.0
    return f"open={first:.2f} close={last:.2f} hi={high:.2f} lo={low:.2f} sess={pct:+.2f}%"


def summarize_fundamentals(payload: Mapping[str, Any] | None) -> str:
    if not payload:
        return "n/a"
    keys = (
        "Sector",
        "Industry",
        "MarketCapitalization",
        "PERatio",
        "ProfitMargin",
        "RevenueTTM",
        "QuarterlyEarningsGrowthYOY",
        "AnalystTargetPrice",
    )
    bits = []
    for k in keys:
        if k in payload and payload[k] not in (None, "", "None"):
            bits.append(f"{k}={payload[k]}")
    return ", ".join(bits) if bits else "n/a"


def summarize_news(items: Sequence[Mapping[str, Any]] | None) -> str:
    if not items:
        return "none"
    out = []
    for n in items[:5]:
        verdict = n.get("verdict") or n.get("sentiment") or "?"
        title = (n.get("title") or n.get("headline") or "").strip()
        nid = n.get("id")
        out.append(f"[#{nid} {verdict}] {title[:120]}")
    return " | ".join(out)


def summarize_sentiment(rows: Sequence[Mapping[str, Any]] | None) -> str:
    if not rows:
        return "none"
    by_source: dict[str, dict[str, float]] = {}
    for r in rows:
        src = r.get("source", "?")
        agg = by_source.setdefault(src, {"mentions": 0.0, "bull": 0.0, "bear": 0.0})
        agg["mentions"] += float(r.get("mentions") or r.get("count") or 0)
        agg["bull"] += float(r.get("bullish") or r.get("bull_count") or 0)
        agg["bear"] += float(r.get("bearish") or r.get("bear_count") or 0)
    bits = []
    for src, agg in by_source.items():
        m = int(agg["mentions"])
        b = int(agg["bull"])
        s = int(agg["bear"])
        bits.append(f"{src}: {m}m b{b}/s{s}")
    return ", ".join(bits)


def summarize_setups(setups: Sequence[Mapping[str, Any]] | None) -> str:
    if not setups:
        return "none"
    out = []
    for s in setups[:5]:
        sid = s.get("id")
        kind = s.get("kind") or s.get("setup_kind") or "?"
        status = s.get("status") or "?"
        when = s.get("created_at") or s.get("first_detected_at") or ""
        out.append(f"[#{sid} {kind}/{status} {str(when)[:10]}]")
    return " ".join(out)


def candidate_set(
    candidates: Sequence[Mapping[str, Any]],
    watchlist: Sequence[Mapping[str, Any]],
    *,
    min_score: float,
    top_k: int,
) -> list[str]:
    """Merge candidate-inbox entries with the active watchlist into a single
    de-duplicated, score-sorted symbol list, capped at top_k."""
    scored: dict[str, float] = {}

    for w in watchlist:
        sym = (w.get("symbol") or "").upper()
        if not sym:
            continue
        # Watchlist always passes — give it a high pseudo-score so it ranks first.
        scored[sym] = max(scored.get(sym, 0.0), 1.0)

    for c in candidates:
        sym = (c.get("symbol") or "").upper()
        if not sym:
            continue
        score = float(c.get("score") or 0.0)
        if score < min_score:
            continue
        scored[sym] = max(scored.get(sym, 0.0), score)

    ordered = sorted(scored.items(), key=lambda kv: kv[1], reverse=True)
    return [sym for sym, _ in ordered[:top_k]]


def to_prompt_text(bundles: Sequence[CandidateBundle]) -> str:
    return "\n".join(b.as_prompt_block() for b in bundles)


def safe_json(value: Any, max_len: int = 4000) -> str:
    """Pretty JSON dump truncated to max_len, used for evidence_refs round-trip."""
    s = json.dumps(value, default=str, ensure_ascii=False)
    return s if len(s) <= max_len else s[: max_len - 3] + "..."
