#!/usr/bin/env python3
"""
M0 Baseline Profiling & Diagnostic Script.

Executes `cargo bench --bench m0_baseline_bench` and parses benchmark metrics
for automated performance tracking and baseline verification.
"""

import subprocess
import sys
import re

def run_benchmark():
    print("[INFO] Running M0 Baseline Benchmark (release mode)...")
    cmd = ["cargo", "bench", "--bench", "m0_baseline_bench"]
    res = subprocess.run(cmd, capture_output=True, text=True)

    if res.returncode != 0:
        print("[ERROR] Benchmark failed with returncode", res.returncode)
        print("STDOUT:\n", res.stdout)
        print("STDERR:\n", res.stderr)
        sys.exit(res.returncode)

    output = res.stdout
    print(output)

    # Validate Correctness Gate
    if "CORRECTNESS GATE: 100% BIT-EXACT MATCH PASSED." not in output:
        print("[ERROR] Correctness Gate verification failed!")
        sys.exit(1)

    print("[SUCCESS] Correctness Gate verified bitwise identical to M1 contracts.")

if __name__ == "__main__":
    run_benchmark()
