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

report-m5-05: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_05
    uv run --directory analysis --project . python -m simciv_analysis.claims
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01

report-m5-05-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_05 --check

check: analysis-test links claims-check report-m5-01-check report-m5-04-check report-m5-05-check

provenance SUMMARY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.provenance {{SUMMARY}}

provenance-finalize SUMMARY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.provenance --finalize {{SUMMARY}}

m5-05-plan: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m5_05 validate

m5-05-deterministic: m5-05-plan
    dvc repro m5_05_deterministic
    just provenance-finalize docs/data/m5_05/deterministic/summary.json
    just provenance docs/data/m5_05/deterministic/summary.json

m5-05-timing ENTRY: m5-05-plan
    dvc repro m5_05_{{ENTRY}}
    dvc freeze m5_05_{{ENTRY}}
    just provenance-finalize docs/data/m5_05/{{ENTRY}}/summary.json
    just provenance docs/data/m5_05/{{ENTRY}}/summary.json

ci:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    just check
