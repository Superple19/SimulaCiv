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

ledger-transition-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.ledger_transition_guard

claims-locate KEY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.claims --locate {{KEY}}

data-integrity-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.data_integrity --check

check-autocrlf: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.data_integrity --check-autocrlf

data-integrity-update: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.data_integrity --write

dvc-push REMOTE: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.dvc_push_guard {{REMOTE}}
    dvc push -r {{REMOTE}}

action-refs-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.action_refs

check-quarto:
    uv run --directory analysis --project . python -m simciv_analysis.quarto_check

tool-versions-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.tool_versions

check-tools: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.check_tools

commit-messages-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.git_hooks --check-commit-range

large-file-range-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.git_hooks --check-pushed-range

data-integrity-range-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.data_integrity --check-base-range origin/main HEAD

gitleaks-range-check:
    prek run --stage pre-push gitleaks-pushed-range

decisions: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.decisions --write

decisions-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.decisions --check

dvc-freeze-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.dvc_freeze_guard --check

report-m5-01: check-quarto claims
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01

report-m5-01-check: check-quarto analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01 --check

report-m5-04: check-quarto analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_04
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01

report-m5-04-check: check-quarto analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_04 --check

report-m5-05: check-quarto analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_05
    uv run --directory analysis --project . python -m simciv_analysis.claims
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01

report-m5-05-check: check-quarto analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_05 --check

reconstruct-all *ARGS: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.reconstruct_all {{ARGS}}

lint-prose: analysis-sync
    vale test --config analysis/vale/.vale.ini
    vale --config analysis/vale/.vale.ini docs/data/m5_05/STATUS.md docs/reproducibility/M5_01_INVENTORY.md docs/reproducibility/M5_04_M4_REGENERATION.md docs/reproducibility/M5_05_M0_M3_REGENERATION.md

check: check-quarto tool-versions-check check-tools analysis-test action-refs-check commit-messages-check large-file-range-check data-integrity-range-check gitleaks-range-check decisions-check dvc-freeze-check check-autocrlf data-integrity-check links claims-check ledger-transition-check reconstruction-inventory-check generated-artifacts-check m3-05-pilot-report-check report-m5-01-check report-m5-04-check report-m5-05-check lint-prose

ci-analysis: check
    uv run --directory analysis --project . python -m simciv_analysis.ci_analysis

ci-rust:
    cargo fmt --check
    cargo clippy --locked --workspace --all-targets -- -D warnings
    cargo test --locked --workspace

clippy-cross:
    cargo clippy --locked --workspace --all-targets --target x86_64-unknown-linux-gnu -- -D warnings
    cargo clippy --locked --workspace --all-targets --target aarch64-apple-darwin -- -D warnings

ci-clean: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.ci_clean

ci-linux:
    mise exec -- uv run --directory analysis --project . python -m simciv_analysis.ci_linux

provenance SUMMARY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.provenance {{SUMMARY}}

provenance-finalize SUMMARY: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.provenance --finalize {{SUMMARY}}

m5-05-plan: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m5_05 validate

reconstruction-inventory: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m2_m3_reconstruction

reconstruction-inventory-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m2_m3_reconstruction --check

recovered-claims: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.recovered_claims

recovered-claims-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.recovered_claims --check

recovered-claim-audit: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.recovered_claim_audit

recovered-claim-audit-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.recovered_claim_audit --check

generated-artifacts-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.generated_artifacts

m5-05-dvc-status: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m5_05_dvc_status

m5-05-dvc-status-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m5_05_dvc_status --check

m3-05-pilot-report: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m3_05_pilot_runtime --summary-only

m3-05-pilot-report-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.m3_05_pilot_runtime --summary-check

check-local-data: analysis-sync
    just recovered-claims-check
    just recovered-claim-audit-check
    just m3-05-pilot-classify-check
    just m3-05-pilot-runtime-check
    just m5-05-dvc-status-check


m5-05-deterministic: m5-05-plan
    dvc repro docs/data/m5_05/dvc.yaml:m5_05_deterministic
    just provenance-finalize docs/data/m5_05/deterministic/summary.json
    just provenance docs/data/m5_05/deterministic/summary.json

m5-05-timing ENTRY: m5-05-plan
    dvc repro docs/data/m5_05/dvc.yaml:m5_05_{{ENTRY}}
    dvc freeze docs/data/m5_05/dvc.yaml:m5_05_{{ENTRY}}
    just provenance-finalize docs/data/m5_05/{{ENTRY}}/summary.json
    just provenance docs/data/m5_05/{{ENTRY}}/summary.json

ci: check-quarto
    just ci-rust
    just clippy-cross
    just ci-analysis

ship: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.ship

m3-05-pilot-classify:
    uv run --directory analysis --project . python -m simciv_analysis.m3_05_pilot_results

m3-05-pilot-classify-check:
    uv run --directory analysis --project . python -m simciv_analysis.m3_05_pilot_results --check

m3-05-pilot-runtime:
    uv run --directory analysis --project . python -m simciv_analysis.m3_05_pilot_runtime

m3-05-pilot-runtime-check:
    uv run --directory analysis --project . python -m simciv_analysis.m3_05_pilot_runtime --check
