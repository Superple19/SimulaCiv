# SimulaCiv analysis

The analysis environment is pinned to Python 3.15.1, uv 0.13.5, pandas 3.0.6, matplotlib 3.11.2, pytest 9.1.1, and Quarto CLI 1.10.19. The Python package requires Python >=3.11; the pinned version is for development and CI.

## Setup

Install uv 0.13.5, Quarto CLI 1.10.19, and just 1.58.0 using their official installation instructions. From the repository root, run:

```sh
uv python install
just analysis-sync
```

`just check` runs the analysis tests and verifies links, claims, and committed reports without rewriting them. To reproduce the M5-01 report from the committed ledger, run the single command:

```sh
just report-m5-01
```

This regenerates the `.qmd` and renders its GitHub-flavored Markdown output with the pinned Quarto version. Raw claims are regenerated separately with `just claims`.
