"""Loads agent/config.toml into a typed structure."""

from __future__ import annotations

import tomllib
from dataclasses import dataclass
from pathlib import Path


@dataclass(frozen=True)
class BudgetConfig:
    per_loop_usd: float
    abort_if_global_spend_above: float


@dataclass(frozen=True)
class UniverseConfig:
    top_k: int
    candidate_min_score: float
    setups_lookback_days: int


@dataclass(frozen=True)
class OutputConfig:
    min_ideas: int
    max_ideas: int


@dataclass(frozen=True)
class ModelsConfig:
    fast: str
    smart: str


@dataclass(frozen=True)
class McpConfig:
    server_bin: str
    socket_path: str | None


@dataclass(frozen=True)
class AgentConfig:
    budget: BudgetConfig
    universe: UniverseConfig
    output: OutputConfig
    models: ModelsConfig
    mcp: McpConfig


def load(path: str | Path | None = None) -> AgentConfig:
    cfg_path = Path(path) if path else Path(__file__).resolve().parent / "config.toml"
    with cfg_path.open("rb") as fh:
        raw = tomllib.load(fh)

    return AgentConfig(
        budget=BudgetConfig(
            per_loop_usd=float(raw["budget"]["per_loop_usd"]),
            abort_if_global_spend_above=float(
                raw["budget"]["abort_if_global_spend_above"]
            ),
        ),
        universe=UniverseConfig(
            top_k=int(raw["universe"]["top_k"]),
            candidate_min_score=float(raw["universe"]["candidate_min_score"]),
            setups_lookback_days=int(raw["universe"]["setups_lookback_days"]),
        ),
        output=OutputConfig(
            min_ideas=int(raw["output"]["min_ideas"]),
            max_ideas=int(raw["output"]["max_ideas"]),
        ),
        models=ModelsConfig(
            fast=str(raw["models"]["fast"]),
            smart=str(raw["models"]["smart"]),
        ),
        mcp=McpConfig(
            server_bin=str(raw["mcp"]["server_bin"]),
            socket_path=raw["mcp"].get("socket_path"),
        ),
    )
