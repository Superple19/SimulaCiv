"""Deterministic headless Matplotlib configuration and SVG serialization."""

from __future__ import annotations

from io import BytesIO

import matplotlib


FIXED_STYLE = {
    "figure.dpi": 100,
    "savefig.dpi": 100,
    "font.family": "DejaVu Sans",
    "font.size": 10,
    "axes.linewidth": 0.8,
    "lines.linewidth": 1.5,
    "svg.hashsalt": "simciv-analysis-v1",
    "svg.fonttype": "none",
    "path.simplify": False,
}


def configure() -> None:
    matplotlib.use("Agg", force=True)
    matplotlib.rcParams.update(FIXED_STYLE)


def svg_bytes(figure) -> bytes:
    configure()
    output = BytesIO()
    figure.savefig(output, format="svg", metadata={"Date": None})
    return output.getvalue()
