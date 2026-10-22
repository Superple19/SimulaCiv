"""Stable Markdown table formatting without optional rendering dependencies."""

from __future__ import annotations

import math
from numbers import Integral, Real

import pandas as pd


def format_value(value: object) -> str:
    if value is None or (not isinstance(value, (str, bytes)) and pd.isna(value)):
        return ""
    if isinstance(value, bool):
        return "true" if value else "false"
    if isinstance(value, Integral):
        return f"{value:,}"
    if isinstance(value, Real):
        number = float(value)
        if not math.isfinite(number):
            return ""
        rendered = f"{number:,.6f}".rstrip("0").rstrip(".")
        return "0" if rendered == "-0" else rendered
    return str(value).replace("|", r"\|").replace("\r\n", "<br>").replace("\n", "<br>")


def dataframe_to_markdown(frame: pd.DataFrame) -> str:
    columns = [format_value(column) for column in frame.columns]
    rows = [[format_value(value) for value in row] for row in frame.itertuples(index=False, name=None)]
    widths = [max(len(columns[index]), *(len(row[index]) for row in rows)) for index in range(len(columns))]

    def render(row: list[str]) -> str:
        return "| " + " | ".join(value.ljust(widths[index]) for index, value in enumerate(row)) + " |"

    return "\n".join([render(columns), render(["-" * width for width in widths]), *(render(row) for row in rows)])
