"""Find static reads of DVC outputs in tests and Markdown report generators."""

from __future__ import annotations

import ast
from pathlib import Path, PurePosixPath
from typing import Iterable

from .git_hooks import dvc_output_paths
from .provenance import ROOT


READ_METHODS = {"open", "read_bytes", "read_text"}


def _names(target: ast.expr) -> Iterable[str]:
    if isinstance(target, ast.Name):
        yield target.id
    elif isinstance(target, (ast.Tuple, ast.List)):
        for item in target.elts:
            yield from _names(item)


def _path_value(node: ast.AST, symbols: dict[str, str]) -> str | None:
    if isinstance(node, ast.Constant) and isinstance(node.value, str):
        return node.value.replace("\\", "/").strip("/")
    if isinstance(node, ast.Name):
        return "" if node.id == "ROOT" else symbols.get(node.id)
    if isinstance(node, ast.BinOp) and isinstance(node.op, ast.Div):
        left, right = _path_value(node.left, symbols), _path_value(node.right, symbols)
        if left is None or right is None:
            return None
        return "/".join(part for part in (left, right) if part)
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Name) and node.func.id == "Path" and node.args:
        return _path_value(node.args[0], symbols)
    if isinstance(node, ast.Call) and isinstance(node.func, ast.Attribute):
        if node.func.attr in {"resolve", "absolute", "relative_to"}:
            return _path_value(node.func.value, symbols)
    return None


def _path_symbols(tree: ast.Module) -> dict[str, str]:
    symbols = {"ROOT": ""}
    assignments = sorted(
        (node for node in ast.walk(tree) if isinstance(node, (ast.Assign, ast.AnnAssign))),
        key=lambda node: node.lineno,
    )
    for node in assignments:
        value = node.value
        targets = node.targets if isinstance(node, ast.Assign) else [node.target]
        for target in targets:
            for name in _names(target):
                if name == "ROOT":
                    symbols[name] = ""
                    continue
                path = _path_value(value, symbols)
                if path is not None:
                    symbols[name] = path
    return symbols


def _read_path(call: ast.Call) -> ast.AST | None:
    function = call.func
    if (
        isinstance(function, ast.Attribute)
        and function.attr == "open"
        and isinstance(function.value, ast.Name)
        and function.value.id == "gzip"
        and call.args
    ):
        return call.args[0]
    if isinstance(function, ast.Attribute) and function.attr in READ_METHODS:
        return function.value
    if isinstance(function, ast.Name) and function.id in {"open", "load_data"} and call.args:
        return call.args[0]
    return None


def find_dvc_opened_paths(source: str, outputs: set[str]) -> list[str]:
    tree = ast.parse(source)
    symbols = _path_symbols(tree)
    found = []
    for node in ast.walk(tree):
        if not isinstance(node, ast.Call):
            continue
        expression = _read_path(node)
        candidate = _path_value(expression, symbols) if expression is not None else None
        if candidate is None:
            continue
        candidate = PurePosixPath(candidate).as_posix().lstrip("./")
        if any(candidate == output or candidate.startswith(output.rstrip("/") + "/") for output in outputs):
            found.append(candidate)
    return found


def analysis_dvc_read_violations(root: Path = ROOT) -> list[str]:
    outputs = dvc_output_paths(root)
    sources = sorted((root / "analysis" / "tests").rglob("*.py"))
    sources.extend(sorted((root / "analysis" / "simciv_analysis").glob("report_*.py")))
    violations = []
    for path in sources:
        try:
            reads = find_dvc_opened_paths(path.read_text(encoding="utf-8"), outputs)
        except SyntaxError as error:
            violations.append(f"{path.relative_to(root)}:{error.lineno}: invalid Python syntax")
            continue
        violations.extend(f"{path.relative_to(root)} opens DVC output {read}" for read in reads)
    return violations
