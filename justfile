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

report-m5-01: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01

report-m5-01-check: analysis-sync
    uv run --directory analysis --project . python -m simciv_analysis.report_m5_01 --check

check: analysis-test links claims-check report-m5-01-check
