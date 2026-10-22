"""Check local Markdown links without following external URLs."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit


ROOT = Path(__file__).resolve().parents[2]
INLINE_LINK = re.compile(r"!?\[[^\]]*\]\(([^)]*)\)")
REFERENCE_LINK = re.compile(r"^\s{0,3}\[[^\]]+\]:\s*(.+?)\s*$", re.MULTILINE)
FENCE = re.compile(r"^\s*(`{3,}|~{3,})")


def targets(markdown: str) -> list[tuple[int, str]]:
    lines = markdown.splitlines()
    visible: list[str] = []
    fence_char: str | None = None
    fence_size = 0
    for line in lines:
        match = FENCE.match(line)
        if match:
            marker = match.group(1)
            if fence_char is None:
                fence_char, fence_size = marker[0], len(marker)
            elif marker[0] == fence_char and len(marker) >= fence_size:
                fence_char = None
            visible.append("")
        elif fence_char is None:
            visible.append(line)
        else:
            visible.append("")

    text = "\n".join(visible)
    result: list[tuple[int, str]] = []
    for pattern in (INLINE_LINK, REFERENCE_LINK):
        for match in pattern.finditer(text):
            raw = match.group(1).strip()
            if raw.startswith("<") and ">" in raw:
                raw = raw[1 : raw.index(">")]
            else:
                raw = raw.split(maxsplit=1)[0] if raw else ""
            if raw:
                result.append((text.count("\n", 0, match.start()) + 1, raw))
    return result


def relative_target(source: Path, target: str) -> Path | None:
    parsed = urlsplit(target)
    if parsed.scheme or parsed.netloc:
        return None
    path = unquote(parsed.path)
    candidate = (source.parent / path) if path else source
    return candidate.resolve()


def main() -> int:
    assert targets("[local](a.md) [web](https://example.com)\n")[0][1] == "a.md"
    assert relative_target(ROOT / "README.md", "https://example.com") is None

    broken: list[tuple[Path, int, str]] = []
    checked = 0
    for source in sorted(ROOT.rglob("*.md")):
        if ".git" in source.parts or "target" in source.parts:
            continue
        for line, target in targets(source.read_text(encoding="utf-8")):
            candidate = relative_target(source, target)
            if candidate is None:
                continue
            checked += 1
            if not candidate.exists():
                broken.append((source.relative_to(ROOT), line, target))

    if broken:
        for source, line, target in broken:
            print(f"{source}:{line}: missing relative target: {target}")
        print(f"{len(broken)} broken of {checked} relative Markdown links")
        return 1
    print(f"Checked {checked} relative Markdown links across the documentation; 0 broken.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
