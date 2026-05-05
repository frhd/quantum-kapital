"""Mirror test — Rust `BehavioralTag` enum ↔ Python `BEHAVIORAL_TAGS` list.

If you add or remove a tag, BOTH sides must change, or this test fails.
The Rust source is parsed textually rather than via FFI so the test
runs against any check-out of the repo without a Cargo build.
"""

from __future__ import annotations

import re
from pathlib import Path

from trade_review import BEHAVIORAL_TAGS


RUST_FILE = (
    Path(__file__).resolve().parents[2]
    / "src-tauri"
    / "src"
    / "services"
    / "trade_reviews"
    / "tags.rs"
)


def _rust_variants() -> list[str]:
    """Extract the variant identifiers declared inside `pub enum BehavioralTag { ... }`."""
    text = RUST_FILE.read_text(encoding="utf-8")
    enum_block = re.search(
        r"pub enum BehavioralTag\s*\{([^}]+)\}", text, flags=re.DOTALL
    )
    assert enum_block, f"BehavioralTag enum not found in {RUST_FILE}"
    raw_lines = enum_block.group(1).splitlines()
    variants: list[str] = []
    for line in raw_lines:
        line = line.strip().rstrip(",")
        if not line or line.startswith("//"):
            continue
        # Variants are bare identifiers (no payload) — strip any trailing
        # parens just in case a future variant grows fields.
        name = line.split("(", 1)[0].strip()
        if name:
            variants.append(name)
    return variants


def _to_snake(camel: str) -> str:
    s1 = re.sub("(.)([A-Z][a-z]+)", r"\1_\2", camel)
    return re.sub("([a-z0-9])([A-Z])", r"\1_\2", s1).lower()


def test_rust_and_python_tag_lists_match():
    rust = [_to_snake(v) for v in _rust_variants()]
    assert sorted(rust) == sorted(BEHAVIORAL_TAGS), (
        f"\nRust   = {sorted(rust)}\nPython = {sorted(BEHAVIORAL_TAGS)}"
    )


def test_rust_variant_count_matches_python():
    # Independent count check so a subtle name-typo on both sides still trips.
    assert len(_rust_variants()) == len(BEHAVIORAL_TAGS) == 12


def test_rust_file_exists():
    """Defensive — the parents[] index above breaks if the agent dir moves."""
    assert RUST_FILE.exists(), f"missing Rust source: {RUST_FILE}"
