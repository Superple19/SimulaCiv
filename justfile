set windows-shell := ["powershell.exe", "-NoLogo", "-Command"]

hooks-install:
    prek install --hook-type pre-commit --hook-type commit-msg --hook-type pre-push

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

data-integrity-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.data_integrity --check

check-autocrlf: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.data_integrity --check-autocrlf

data-integrity-update: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.data_integrity --write

action-refs-check:
    uv run --directory analysis --project . python -m simciv_analysis.action_refs

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

lint-prose: analysis-sync
    vale test --config analysis/vale/.vale.ini
    vale --config analysis/vale/.vale.ini docs/data/m5_05/STATUS.md docs/reproducibility/M5_01_INVENTORY.md docs/reproducibility/M5_04_M4_REGENERATION.md docs/reproducibility/M5_05_M0_M3_REGENERATION.md

check: analysis-test action-refs-check check-autocrlf data-integrity-check links claims-check report-m5-01-check report-m5-04-check report-m5-05-check lint-prose

provenance SUMMARY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.provenance {{SUMMARY}}

provenance-finalize SUMMARY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.provenance --finalize {{SUMMARY}}

m5-05-plan: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m5_05 validate

m5-05-deterministic: m5-05-plan
    dvc repro docs/data/m5_05/dvc.yaml:m5_05_deterministic
    just provenance-finalize docs/data/m5_05/deterministic/summary.json
    just provenance docs/data/m5_05/deterministic/summary.json

m5-05-timing ENTRY: m5-05-plan
    dvc repro docs/data/m5_05/dvc.yaml:m5_05_{{ENTRY}}
    dvc freeze docs/data/m5_05/dvc.yaml:m5_05_{{ENTRY}}
    just provenance-finalize docs/data/m5_05/{{ENTRY}}/summary.json
    just provenance docs/data/m5_05/{{ENTRY}}/summary.json

ci:
    cargo fmt --check
    cargo clippy --workspace --all-targets -- -D warnings
    cargo test --workspace
    just check
