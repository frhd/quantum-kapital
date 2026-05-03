"""Pricing-table sync test.

`agent/budget_guard.py::_PRICES_USD_PER_MTOK` mirrors
`src-tauri/src/services/llm_service/prices.rs::price_for`. Drift is
silent — it just under- or over-counts. This test parses the Rust
`match` arms and asserts every model + (input, output) rate matches
the Python table.

If the Rust side gains a third dimension (e.g. cache_read), this test
only checks the (input, output) pair the Python guard uses; extend it
when the Python side learns about cache_read pricing.
"""

from __future__ import annotations

import re
from pathlib import Path

import budget_guard as bg


_RUST_PRICES = (
    Path(__file__).resolve().parent.parent.parent
    / "src-tauri"
    / "src"
    / "services"
    / "llm_service"
    / "prices.rs"
)


_ARM_RE = re.compile(
    r'"(?P<model>claude-[a-z0-9-]+)"\s*=>\s*Some\(\s*'
    r'\(\s*(?P<inp>[0-9.]+)\s*,\s*(?P<out>[0-9.]+)\s*,\s*[0-9.]+\s*\)\s*\)',
)


def _rust_table() -> dict[str, tuple[float, float]]:
    text = _RUST_PRICES.read_text()
    out: dict[str, tuple[float, float]] = {}
    for m in _ARM_RE.finditer(text):
        out[m.group("model")] = (float(m.group("inp")), float(m.group("out")))
    return out


def test_python_table_includes_every_rust_model():
    rust = _rust_table()
    assert rust, f"failed to parse any models from {_RUST_PRICES}"
    for model, (inp, out) in rust.items():
        assert model in bg._PRICES_USD_PER_MTOK, (
            f"Rust prices.rs lists {model!r} but Python table is missing it; "
            "keep agent/budget_guard.py in lockstep with prices.rs."
        )
        py_inp, py_out = bg._PRICES_USD_PER_MTOK[model]
        assert (py_inp, py_out) == (inp, out), (
            f"price drift for {model}: rust={(inp, out)} python={(py_inp, py_out)}"
        )


def test_rust_table_includes_every_python_model_we_might_call():
    """The Python side has an extra `claude-opus-4-7` entry that the
    Rust pricing table does not list — opus is used by neither agent
    loop nor the Rust call sites today, so the asymmetry is intentional.
    Document it here so a maintainer who adds opus to the agents
    remembers to extend prices.rs at the same time."""
    rust = _rust_table()
    asymmetric = set(bg._PRICES_USD_PER_MTOK) - set(rust)
    assert asymmetric == {"claude-opus-4-7"}, (
        f"unexpected asymmetric models {asymmetric}; either add the model to "
        "src-tauri/src/services/llm_service/prices.rs or remove it from "
        "agent/budget_guard.py."
    )
