"""Check preserved milestone reports through one gate entry point."""

from __future__ import annotations

import argparse
import sys

from . import report_m5_01, report_m5_04, report_m5_05


REPORT_CHECKS = (
    ("M5-01", report_m5_01.check_outputs),
    ("M5-04", report_m5_04.check_outputs),
    ("M5-05", report_m5_05.check_outputs),
)


def check_reports() -> bool:
    failed = [name for name, check in REPORT_CHECKS if not check()]
    if failed:
        print(f"generated report checks failed: {', '.join(failed)}", file=sys.stderr)
        return False
    return True


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--check", action="store_true")
    args = parser.parse_args(argv)
    if not args.check:
        parser.error("only --check is supported")
    return 0 if check_reports() else 1


if __name__ == "__main__":
    raise SystemExit(main())
