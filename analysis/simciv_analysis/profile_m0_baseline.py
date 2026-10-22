#!/usr/bin/env python3
"""M0 Baseline Profiling & Diagnostic Script.

Executes ``cargo bench --bench m0_baseline_bench`` and verifies its correctness
marker. Importing this module does not run a benchmark.
"""

from __future__ import annotations

import subprocess
import sys


def run_benchmark() -> None:
    print("[INFO] Running M0 Baseline Benchmark (release mode)...")
    result = subprocess.run(["cargo", "bench", "--bench", "m0_baseline_bench"], capture_output=True, text=True)
    if result.returncode != 0:
        print("[ERROR] Benchmark failed with returncode", result.returncode)
        print("STDOUT:\n", result.stdout)
        print("STDERR:\n", result.stderr)
        raise SystemExit(result.returncode)

    print(result.stdout)
    if "CORRECTNESS GATE: 100% BIT-EXACT MATCH PASSED." not in result.stdout:
        print("[ERROR] Correctness Gate verification failed!")
        raise SystemExit(1)
    print("[SUCCESS] Correctness Gate verified bitwise identical to M1 contracts.")


if __name__ == "__main__":
    run_benchmark()
