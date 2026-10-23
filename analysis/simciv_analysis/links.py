"""Check local Markdown links without following external URLs."""

from __future__ import annotations

import re
import sys
from pathlib import Path
from urllib.parse import unquote, urlsplit

from simciv_analysis.git_hooks import dvc_output_paths


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


def _is_dvc_output(candidate: Path, root: Path, outputs: set[str]) -> bool:
    try:
        relative = candidate.relative_to(root.resolve()).as_posix()
    except ValueError:
        return False
    return any(
        relative == output or relative.startswith(output.rstrip("/") + "/")
        for output in outputs
    )


def main(paths: list[str] | None = None) -> int:
    assert targets("[local](a.md) [web](https://example.com)\n")[0][1] == "a.md"
    assert relative_target(ROOT / "README.md", "https://example.com") is None

    broken: list[tuple[Path, int, str]] = []
    dvc_links: list[tuple[Path, int, str]] = []
    checked = 0
    outputs = dvc_output_paths(ROOT)
    if paths:
        sources = sorted((ROOT / path).resolve() for path in paths)
    else:
        sources = sorted((*ROOT.rglob("*.md"), *ROOT.rglob("*.qmd")))
    for source in sources:
        if source.suffix.lower() not in {".md", ".qmd"}:
            continue
        if ".git" in source.parts or "target" in source.parts or ".venv" in source.parts:
            continue
        for line, target in targets(source.read_text(encoding="utf-8")):
            candidate = relative_target(source, target)
            if candidate is None:
                continue
            checked += 1
            if _is_dvc_output(candidate, ROOT, outputs):
                dvc_links.append((source.relative_to(ROOT), line, target))
            elif not candidate.exists():
                broken.append((source.relative_to(ROOT), line, target))

    if dvc_links:
        for source, line, target in dvc_links:
            print(
                f"{source}:{line}: DVC-tracked output is unavailable in a clean checkout: "
                f"{target}; link to the stage dvc.yaml/README entry or render the path as code"
            )
    if broken:
        for source, line, target in broken:
            print(f"{source}:{line}: missing relative target: {target}")
        print(f"{len(broken)} broken of {checked} relative Markdown links")
    if broken or dvc_links:
        if dvc_links:
            print(f"{len(dvc_links)} DVC-tracked output links must be replaced")
        return 1
    print(f"Checked {checked} relative Markdown links across the documentation; 0 broken.")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv[1:] or None))
