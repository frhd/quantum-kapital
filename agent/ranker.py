"""LLM ranking step — score each candidate on the 0-1 rubric.

Forced-tool call: the LLM must emit exactly one `score_candidates` tool_use,
which we parse and return as a sorted list. No prose path.
"""

from __future__ import annotations

from dataclasses import dataclass
from typing import Any, Sequence

from budget_guard import BudgetGuard
from data_summary import CandidateBundle, to_prompt_text
from llm import LlmClient


SCORE_TOOL: dict[str, Any] = {
    "name": "score_candidates",
    "description": (
        "Score each candidate on four 0-1 sub-scores (technical, fundamentals, "
        "sentiment, catalyst) plus a single-line note. Order does not matter."
    ),
    "input_schema": {
        "type": "object",
        "properties": {
            "scores": {
                "type": "array",
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "technical": {"type": "number", "minimum": 0, "maximum": 1},
                        "fundamentals": {"type": "number", "minimum": 0, "maximum": 1},
                        "sentiment": {"type": "number", "minimum": 0, "maximum": 1},
                        "catalyst": {"type": "number", "minimum": 0, "maximum": 1},
                        "note": {"type": "string"},
                    },
                    "required": [
                        "symbol",
                        "technical",
                        "fundamentals",
                        "sentiment",
                        "catalyst",
                    ],
                    "additionalProperties": False,
                },
            }
        },
        "required": ["scores"],
        "additionalProperties": False,
    },
}


@dataclass
class CandidateScore:
    symbol: str
    technical: float
    fundamentals: float
    sentiment: float
    catalyst: float
    note: str

    @property
    def total(self) -> float:
        return (self.technical + self.fundamentals + self.sentiment + self.catalyst) / 4.0


def _coerce_scores(payload: Any) -> list[CandidateScore]:
    rows = (payload or {}).get("scores", []) if isinstance(payload, dict) else []
    out: list[CandidateScore] = []
    for r in rows:
        if not isinstance(r, dict):
            continue
        sym = (r.get("symbol") or "").upper()
        if not sym:
            continue
        out.append(
            CandidateScore(
                symbol=sym,
                technical=float(r.get("technical") or 0),
                fundamentals=float(r.get("fundamentals") or 0),
                sentiment=float(r.get("sentiment") or 0),
                catalyst=float(r.get("catalyst") or 0),
                note=str(r.get("note") or ""),
            )
        )
    return out


async def rank_candidates(
    *,
    llm: LlmClient,
    model: str,
    system_prompt: str,
    bundles: Sequence[CandidateBundle],
    guard: BudgetGuard,
) -> list[CandidateScore]:
    if not bundles:
        return []

    guard.ensure_can_spend()

    user = (
        "Score the following candidates on the 0-1 rubric per `score_candidates`. "
        "Be calibrated: most candidates should score in the 0.3-0.7 range. Only "
        "give 0.8+ when evidence is unambiguous. Note field: one short clause.\n\n"
        + to_prompt_text(bundles)
    )

    resp = await llm.call(
        model=model,
        system=system_prompt,
        messages=[{"role": "user", "content": user}],
        tools=[SCORE_TOOL],
        tool_choice={"type": "tool", "name": "score_candidates"},
        max_tokens=2048,
    )
    guard.record(model, resp.input_tokens, resp.output_tokens, envelope_cost_usd=resp.cost_usd)

    if not resp.tool_uses:
        return []
    payload = resp.tool_uses[0].input
    scores = _coerce_scores(payload)
    scores.sort(key=lambda s: s.total, reverse=True)
    return scores
