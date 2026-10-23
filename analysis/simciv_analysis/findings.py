"""Render report findings together with their executable data checks."""

from __future__ import annotations

from dataclasses import dataclass
import re


@dataclass(frozen=True)
class Finding:
    tag: str
    text: str
    predicate: str
    value: bool


def render_findings(findings: list[Finding]) -> tuple[str, str]:
    if not findings:
        raise ValueError("a generated report must include at least one checked finding")
    predicate_names: set[str] = set()
    for finding in findings:
        if finding.tag != "[exploratory]" and not re.fullmatch(r"\[pre-registered: [A-Za-z0-9_-]+\]", finding.tag):
            raise ValueError(f"invalid finding tag: {finding.tag!r}")
        if not re.fullmatch(r"finding_[a-z0-9_]+", finding.predicate):
            raise ValueError(f"invalid finding predicate name: {finding.predicate!r}")
        if finding.predicate in predicate_names:
            raise ValueError(f"duplicate finding predicate: {finding.predicate}")
        predicate_names.add(finding.predicate)
    failed = [finding.predicate for finding in findings if not finding.value]
    if failed:
        raise ValueError(f"paired finding checks failed: {', '.join(failed)}")

    bullets = "\n".join(f"- {finding.tag} {finding.text}" for finding in findings)
    rows = [
        "| Finding tag | Paired predicate | Value |",
        "|---|---|---|",
        *[
            f"| {finding.tag} | `{finding.predicate}` | `{str(finding.value).lower()}` |"
            for finding in findings
        ],
    ]
    return bullets, "\n".join(rows)
