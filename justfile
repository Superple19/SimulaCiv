set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

analysis-sync:
    uv sync --project analysis --frozen

analysis-test: analysis-sync
    uv run --directory analysis --project . pytest

links: analysis-sync
    uv run --directory analysis --project . python check_links.py

claims: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.claims

claims-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.claims --check

report-m5-01: claims
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01

report-m5-01-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01 --check

report-m5-04: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_04
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01

report-m5-04-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_04 --check

check: analysis-test links claims-check report-m5-01-check report-m5-04-check

provenance SUMMARY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.provenance {{SUMMARY}}

ci:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    just check
