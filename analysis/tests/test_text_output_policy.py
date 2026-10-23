import ast
from pathlib import Path


PACKAGE = Path(__file__).resolve().parents[1] / "simciv_analysis"
GENERATED_REPORTS = [
    Path("docs/reproducibility/M5_01_INVENTORY.qmd"),
    Path("docs/reproducibility/M5_01_INVENTORY.md"),
    Path("docs/reproducibility/M5_04_M4_REGENERATION.qmd"),
    Path("docs/reproducibility/M5_04_M4_REGENERATION.md"),
    Path("docs/reproducibility/M5_05_M0_M3_REGENERATION.qmd"),
    Path("docs/reproducibility/M5_05_M0_M3_REGENERATION.md"),
    Path("docs/reproducibility/figures/m5_04_old_new_timing.svg"),
    Path("docs/reproducibility/figures/m5_04_old_new_divergence.svg"),
    Path("docs/reproducibility/figures/m5_05_old_new_timing.svg"),
]


def _write_mode(call: ast.Call, bound_method: bool) -> str | None:
    mode_positions = (0, 1) if bound_method else (1,)
    for position in mode_positions:
        if len(call.args) > position and isinstance(call.args[position], ast.Constant):
            value = call.args[position].value
            if isinstance(value, str) and value[:1] in {"w", "a", "x"}:
                return value
    for keyword in call.keywords:
        if keyword.arg == "mode" and isinstance(keyword.value, ast.Constant):
            return keyword.value.value if isinstance(keyword.value.value, str) else None
    return None


def test_package_text_writes_are_centralized_in_io():
    violations = []
    for source in PACKAGE.rglob("*.py"):
        if source.name == "io.py":
            continue
        tree = ast.parse(source.read_text(encoding="utf-8"), filename=str(source))
        for node in ast.walk(tree):
            if not isinstance(node, ast.Call):
                continue
            function = node.func
            bound_method = isinstance(function, ast.Attribute)
            name = function.id if isinstance(function, ast.Name) else function.attr if bound_method else ""
            mode = _write_mode(node, bound_method) if name == "open" else None
            if name == "write_text" or (mode and (mode[:1] in {"w", "a", "x"} or "+" in mode)):
                violations.append(f"{source.name}:{node.lineno}:{name}")
    assert violations == []


def test_generated_reports_contain_no_carriage_returns():
    root = PACKAGE.parents[1]
    for relative in GENERATED_REPORTS:
        path = root / relative
        assert path.is_file(), f"generated report is missing: {relative}"
        assert b"\r" not in path.read_bytes(), f"generated report contains CR: {relative}"
