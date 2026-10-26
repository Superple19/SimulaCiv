"""Reject unreviewed reductions in the claim ledger's evidence strength."""

from __future__ import annotations

import argparse
import csv
import gzip
import io
import subprocess
import sys
import tomllib
from pathlib import Path

from .claims import OUTPUT, ROOT, extract_claims, ledger_bytes


ALLOWLIST = ROOT / "docs" / "reproducibility" / "ledger_transition_allowlist.toml"
LEDGER_PATH = OUTPUT.relative_to(ROOT).as_posix()

# Ordered from least to most evidence. Primary recorded verdicts share a protected top tier.
EVIDENCE_STRENGTH = {
    "unrecoverable": 15,
    "unmapped": 0,
    "unverifiable": 10,
    "missing-data": 20,
    "missing-source": 20,
    "recovered-weak (composite field)": 30,
    "recovered-derived": 40,
    "recovered-other-run": 40,
    "recovered-output-mismatch": 45,
    "recovered-original-output (formatted)": 45,
    "recovered-original-output (unit-converted)": 45,
    "recovered-original-output": 50,
    "analysis-only": 100,
    "bit-exact": 100,
    "bit-exact after documented normalization": 100,
    "conclusion holds": 100,
    "conclusion changed": 100,
    "historical-log": 100,
    "historical-protocol": 100,
    "inconclusive": 100,
    "inconclusive (validity)": 100,
    "not reproduced": 100,
    "reproduced": 100,
    "reproduced (loose)": 100,
    "reproducible-now": 100,
}
PRIMARY_STATUSES = {status for status, strength in EVIDENCE_STRENGTH.items() if strength == 100}


def _csv_rows(data: bytes) -> list[dict[str, str]]:
    return list(csv.DictReader(io.StringIO(gzip.decompress(data).decode("utf-8"), newline="")))


def load_allowlist(path: Path = ALLOWLIST) -> list[dict[str, str]]:
    return list(tomllib.loads(path.read_text(encoding="utf-8")).get("transition", []))


def validate_transitions(
    previous: list[dict[str, str]],
    current: list[dict[str, str]],
    allowlist: list[dict[str, str]],
) -> list[tuple[str, str, str]]:
    old_by_key = {row["claim_key"]: row for row in previous}
    current_by_key = {row["claim_key"]: row for row in current}
    transitions: list[tuple[str, str, str]] = []
    violations: list[str] = []
    for key in sorted(old_by_key.keys() & current_by_key.keys()):
        old = old_by_key[key]
        new = current_by_key[key]
        before, after = old.get("status", ""), new.get("status", "")
        if before == after:
            continue
        if before not in EVIDENCE_STRENGTH or after not in EVIDENCE_STRENGTH:
            violations.append(f"{key}: unclassified status transition {before!r} -> {after!r}")
            continue
        if EVIDENCE_STRENGTH[after] >= EVIDENCE_STRENGTH[before]:
            continue
        evidence = new.get("recovered_evidence", "none")
        allowed = before not in PRIMARY_STATUSES and any(
            item.get("from_status") == before
            and item.get("to_status") == after
            and item.get("recovered_evidence") == evidence
            and bool(item.get("reason", "").strip())
            for item in allowlist
        )
        if allowed:
            transitions.append((before, after, evidence))
        else:
            violations.append(
                f"{key}: evidence weakened {before!r} -> {after!r} (recovered_evidence={evidence!r}); "
                "add a reviewed, reasoned allowlist entry if intentional"
            )
    if violations:
        raise ValueError("claim ledger evidence regression:\n" + "\n".join(violations[:30]))
    return transitions


def _base_ref(root: Path, requested: str | None) -> str:
    candidates = [requested] if requested else ["origin/main", "HEAD~1"]
    head = subprocess.run(["git", "rev-parse", "HEAD"], cwd=root, check=True, capture_output=True, text=True).stdout.strip()
    for candidate in candidates:
        if not candidate:
            continue
        result = subprocess.run(["git", "rev-parse", "--verify", candidate], cwd=root, capture_output=True, text=True)
        if result.returncode == 0 and result.stdout.strip() != head:
            return candidate
    if requested:
        raise ValueError(f"base revision does not resolve to a commit different from HEAD: {requested}")
    raise ValueError("could not resolve origin/main or HEAD~1 as a ledger comparison base")


def _base_ledger(root: Path, ref: str) -> bytes:
    result = subprocess.run(
        ["git", "show", f"{ref}:{LEDGER_PATH}"], cwd=root, capture_output=True
    )
    if result.returncode:
        raise ValueError(f"could not read {LEDGER_PATH} from {ref}: {result.stderr.decode(errors='replace').strip()}")
    return result.stdout


def check(root: Path = ROOT, base: str | None = None) -> list[tuple[str, str, str]]:
    ref = _base_ref(root, base)
    previous = _csv_rows(_base_ledger(root, ref))
    current = _csv_rows(ledger_bytes(extract_claims(root)))
    allowed = load_allowlist(root / ALLOWLIST.relative_to(ROOT))
    return validate_transitions(previous, current, allowed)


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--base", help="comparison base; defaults to origin/main, then HEAD~1")
    args = parser.parse_args(argv)
    try:
        transitions = check(base=args.base)
    except (OSError, ValueError, subprocess.CalledProcessError, gzip.BadGzipFile) as error:
        print(f"ledger transition check failed: {error}", file=sys.stderr)
        return 1
    print(f"claim evidence transitions checked against base; {len(transitions)} reviewed downgrade(s) allowlisted")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
