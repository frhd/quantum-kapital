"""LLM synthesis step — turn the top-N scored candidates into ranked ideas
for the morning pack. The Anthropic call uses tool_choice ForceTool against
a tool whose schema mirrors the Rust `write_morning_pack` MCP tool's
`ranked_ideas` shape, so the model is forced to emit valid structured output.

We then pass the resulting ideas to the MCP server's `write_morning_pack` to
persist them. Two separate writes (LLM emit → MCP persist) so we can inspect
or dry-run between them.
"""

from __future__ import annotations

from typing import Any, Sequence

from budget_guard import BudgetGuard
from data_summary import CandidateBundle, to_prompt_text
from llm import LlmClient
from ranker import CandidateScore


WRITE_PACK_TOOL: dict[str, Any] = {
    "name": "write_morning_pack",
    "description": (
        "Emit 3-5 ranked ideas in priority order. Use exactly once. If you "
        "cannot find 3 candidates that meet at least the C bar, emit fewer "
        "(do not pad). Each thesis_md must cite specific evidence from the "
        "inputs (item ids, dates, levels)."
    ),
    "input_schema": {
        "type": "object",
        "properties": {
            "ranked_ideas": {
                "type": "array",
                "minItems": 1,
                "maxItems": 5,
                "items": {
                    "type": "object",
                    "properties": {
                        "symbol": {"type": "string"},
                        "thesis_md": {"type": "string"},
                        "conviction": {"type": "string", "enum": ["A", "B", "C"]},
                        "entry_zone": {"type": "string"},
                        "invalidation": {"type": "string"},
                        "evidence_refs": {
                            "type": "array",
                            "items": {"type": "object"},
                        },
                    },
                    "required": ["symbol", "thesis_md", "conviction", "invalidation"],
                    "additionalProperties": False,
                },
            }
        },
        "required": ["ranked_ideas"],
        "additionalProperties": False,
    },
}


def _bundle_lookup(bundles: Sequence[CandidateBundle]) -> dict[str, CandidateBundle]:
    return {b.symbol: b for b in bundles}


async def synthesize_pack(
    *,
    llm: LlmClient,
    model: str,
    system_prompt: str,
    scores: Sequence[CandidateScore],
    bundles: Sequence[CandidateBundle],
    guard: BudgetGuard,
    min_ideas: int,
    max_ideas: int,
    shadow: bool = False,
) -> list[dict[str, Any]]:
    if not scores:
        return []

    guard.ensure_can_spend()

    lookup = _bundle_lookup(bundles)
    top = list(scores)[: max(max_ideas, min_ideas) + 2]
    selected_bundles = [lookup[s.symbol] for s in top if s.symbol in lookup]

    score_block = "\n".join(
        f"- {s.symbol}: tech={s.technical:.2f} fund={s.fundamentals:.2f} "
        f"sent={s.sentiment:.2f} cat={s.catalyst:.2f} total={s.total:.2f} — {s.note}"
        for s in top
    )

    shadow_note = ""
    if shadow:
        shadow_note = (
            "\n\nIMPORTANT: shadow mode is ON. Prepend the literal markdown "
            "**[SHADOW PACK — researcher in evaluation. Do not act on these "
            "picks yet.]** to every thesis_md, on its own line."
        )

    user = (
        f"You scored these candidates:\n{score_block}\n\n"
        f"Now emit a morning pack with at least {min_ideas} and at most "
        f"{max_ideas} ideas via the `write_morning_pack` tool. Order them by "
        f"your conviction. Use exactly the structured fields. If fewer than "
        f"{min_ideas} candidates clear the C bar, emit fewer — do not pad."
        f"{shadow_note}\n\n"
        f"Full data for the top candidates:\n\n{to_prompt_text(selected_bundles)}"
    )

    resp = await llm.call(
        model=model,
        system=system_prompt,
        messages=[{"role": "user", "content": user}],
        tools=[WRITE_PACK_TOOL],
        tool_choice={"type": "tool", "name": "write_morning_pack"},
        max_tokens=4096,
    )
    guard.record(model, resp.input_tokens, resp.output_tokens)

    if not resp.tool_uses:
        return []
    raw = resp.tool_uses[0].input.get("ranked_ideas", [])
    ideas: list[dict[str, Any]] = []
    for idea in raw:
        if not isinstance(idea, dict):
            continue
        sym = (idea.get("symbol") or "").upper()
        thesis = (idea.get("thesis_md") or "").strip()
        conviction = idea.get("conviction") or "C"
        if not sym or not thesis:
            continue
        cleaned: dict[str, Any] = {
            "symbol": sym,
            "thesis_md": thesis,
            "conviction": conviction,
            "invalidation": idea.get("invalidation") or "",
        }
        if idea.get("entry_zone"):
            cleaned["entry_zone"] = idea["entry_zone"]
        if idea.get("evidence_refs"):
            cleaned["evidence_refs"] = idea["evidence_refs"]
        ideas.append(cleaned)
    return ideas
