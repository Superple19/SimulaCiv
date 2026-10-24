"""Read mise pins and reject drift in duplicated installation instructions."""

from __future__ import annotations

import re
import subprocess
import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TOOL_BACKENDS = {
    "uv": "aqua:astral-sh/uv",
    "just": "aqua:casey/just",
    "vale": "aqua:vale-cli/vale",
    "prek": "aqua:j178/prek",
    "quarto": "aqua:quarto-dev/quarto-cli",
    "dvc": "pipx:dvc",
}
LOCK_PLATFORMS = {"windows-x64", "linux-x64", "macos-arm64"}
HISTORICAL_CONTRACT = "docs/contracts/M5_REPRODUCIBILITY_CONTRACT.md"
HISTORICAL_MARKER = "Current tool pins are managed by mise.toml and mise.lock; versions in this register are historical records."
BOOTSTRAP_VERSION = re.compile(r'^# mise-bootstrap-version = "([^"]+)"$', re.MULTILINE)
TOOL_VERSION_PATTERN = re.compile(
    r"(?i)\b(?P<tool>uv|python|quarto|just|vale|prek|dvc)\b"
    r"(?:\s+cli)?\s*(?:==|@|(?<![<>])=|\s+(?:--version\s+|version\s+)?)"
    r"\s*v?(?P<version>\d+(?:\.\d+){1,2})\b"
)
URL_TOOL_VERSION_PATTERN = re.compile(
    r"(?i)\b(?:uv|python|quarto|just|vale|prek|dvc)[^\s]{0,100}/v?"
    r"(?P<version>\d+\.\d+(?:\.\d+)?)"
)
SCANNED_SUFFIXES = {".md", ".qmd", ".toml", ".yaml", ".yml", ".py", ".ps1", ".sh", ".txt"}
PIN_FILES = {"mise.toml", "mise.lock", "analysis/pyproject.toml", "analysis/.python-version", "analysis/uv.lock", "rust-toolchain.toml"}


def load_mise_config(root: Path = ROOT) -> tuple[dict[str, dict[str, str]], str]:
    path = root / "mise.toml"
    text = path.read_text(encoding="utf-8")
    config = tomllib.loads(text)
    tools = config.get("tools")
    if not isinstance(tools, dict) or set(tools) != set(TOOL_BACKENDS.values()):
        raise ValueError(f"mise.toml: expected exactly these tool backends: {sorted(TOOL_BACKENDS.values())}")
    match = BOOTSTRAP_VERSION.search(text)
    if match is None:
        raise ValueError("mise.toml: missing # mise-bootstrap-version pin")

    versions: dict[str, dict[str, str]] = {}
    for name, backend in TOOL_BACKENDS.items():
        entry = tools[backend]
        if isinstance(entry, str):
            values = {"version": entry}
        elif isinstance(entry, dict) and isinstance(entry.get("version"), str):
            values = dict(entry)
        else:
            raise ValueError(f"mise.toml: {backend} requires a pinned version")
        if name == "dvc" and not re.fullmatch(r"\d+\.\d+", values.get("python", "")):
            raise ValueError("mise.toml: pipx:dvc requires a private Python major.minor runtime")
        versions[name] = values

    # The analysis interpreter is intentionally managed by uv, not mise.
    python_version = (root / "analysis" / ".python-version").read_text(encoding="utf-8").strip()
    if not python_version:
        raise ValueError("analysis/.python-version: expected a pinned analysis Python version")
    versions["python"] = {"version": python_version}
    return versions, match.group(1)


def load_tool_versions(root: Path = ROOT) -> dict[str, dict[str, str]]:
    """Keep the lookup API while sourcing standalone tools from mise.toml."""
    return load_mise_config(root)[0]


def tool_version(name: str, root: Path = ROOT) -> str:
    return load_tool_versions(root)[name]["version"]


def mise_version(root: Path = ROOT) -> str:
    return load_mise_config(root)[1]


def _tracked_paths(root: Path) -> list[Path]:
    result = subprocess.run(
        ["git", "ls-files", "--cached", "--others", "--exclude-standard", "-z"],
        cwd=root,
        capture_output=True,
        check=False,
    )
    if result.returncode:
        raise RuntimeError(f"git ls-files failed: {result.stderr.decode(errors='replace')}")
    return sorted({Path(raw.decode("utf-8")) for raw in result.stdout.split(b"\0") if raw})


def hardcoded_tool_versions(text: str, relative_path: str) -> list[str]:
    if relative_path == ".github/workflows/ci.yml":
        return []  # The mise bootstrap is checked against its workflow environment separately.
    findings = []
    for number, line in enumerate(text.splitlines(), 1):
        if "uses:" in line:
            continue  # Action refs have their own exact-tag check.
        if re.search(r"\bpython\s*(?:>=|<=|>|<)\s*\d", line, re.IGNORECASE):
            continue  # Compatibility ranges are not pinned interpreter installs.
        if "smoke matrix" in line.lower():
            continue  # Historical CI matrix decisions are not installed tool versions.
        if re.search(r"\bpython\s+\d+(?:\.\d+){1,2}\s+or newer\b", line, re.IGNORECASE):
            continue  # Minimum-version compatibility requirements are not interpreter pins.
        matches = [*TOOL_VERSION_PATTERN.finditer(line), *URL_TOOL_VERSION_PATTERN.finditer(line)]
        for match in matches:
            findings.append(
                f"{relative_path}:{number}: hard-coded {match.groupdict().get('tool', 'tool')} version "
                f"{match.group('version')}; use mise.toml"
            )
    return findings


def check_tool_versions(root: Path = ROOT) -> list[str]:
    pins, manager_version = load_mise_config(root)
    errors: list[str] = []

    def read(relative: str) -> str:
        try:
            return (root / relative).read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"{relative}: cannot read: {error}")
            return ""

    try:
        analysis_project = tomllib.loads(read("analysis/pyproject.toml"))
    except tomllib.TOMLDecodeError as error:
        errors.append(f"analysis/pyproject.toml: invalid TOML: {error}")
        analysis_project = {}
    expected_uv = f"=={pins['uv']['version']}"
    uv_settings = analysis_project.get("tool", {}).get("uv", {})
    if uv_settings.get("required-version") != expected_uv:
        errors.append(f"analysis/pyproject.toml: expected tool.uv.required-version {expected_uv!r}, found {uv_settings.get('required-version')!r}")

    python_pin = read("analysis/.python-version").strip()
    if python_pin != pins["python"]["version"]:
        errors.append(f"analysis/.python-version: expected Python {pins['python']['version']}, found {python_pin!r}")
    if (root / ".python-version").exists():
        errors.append(".python-version: move the analysis interpreter pin to analysis/.python-version")
    if (root / "uv.toml").exists():
        errors.append("uv.toml: move required-version to analysis/pyproject.toml")

    mise = tomllib.loads(read("mise.toml"))
    settings = mise.get("settings", {})
    platforms = sorted(settings.get("lockfile_platforms", []))
    if settings.get("lockfile") is not True:
        errors.append("mise.toml: settings.lockfile must be true")
    if platforms != sorted(LOCK_PLATFORMS):
        errors.append(f"mise.toml: expected lockfile_platforms {sorted(LOCK_PLATFORMS)}, found {platforms}")

    try:
        lock = tomllib.loads(read("mise.lock"))
    except tomllib.TOMLDecodeError as error:
        errors.append(f"mise.lock: invalid TOML: {error}")
        lock = {}
    lock_tools = lock.get("tools", {})
    for name, backend in TOOL_BACKENDS.items():
        entries = lock_tools.get(backend, [])
        version = pins[name]["version"]
        if not isinstance(entries, list) or not any(entry.get("version") == version for entry in entries):
            errors.append(f"mise.lock: missing {backend}@{version}")
        if backend.startswith("aqua:"):
            locked_platforms = {
                key.removeprefix("platforms.")
                for entry in entries
                for key in entry
                if key.startswith("platforms.")
            }
            if locked_platforms != LOCK_PLATFORMS:
                errors.append(f"mise.lock: {backend} platform entries are {sorted(locked_platforms)}")

    dvc_entry = lock_tools.get("pipx:dvc", [{}])[0]
    uv_lock_path = root / dvc_entry.get("uv", {}).get("path", "missing")
    if not (uv_lock_path / "pyproject.toml").is_file() or not (uv_lock_path / "uv.lock").is_file():
        errors.append("mise.lock: private pipx:dvc lock sidecar is missing")
    private_python = lock_tools.get("python", [])
    private_python_minor = pins["dvc"]["python"]
    if not any(entry.get("version", "").startswith(f"{private_python_minor}.") for entry in private_python):
        errors.append("mise.lock: private Python runtime for pipx:dvc is missing")
    else:
        locked_platforms = {
            key.removeprefix("platforms.")
            for entry in private_python
            for key in entry
            if key.startswith("platforms.")
        }
        if locked_platforms != LOCK_PLATFORMS:
            errors.append(f"mise.lock: private Python platform entries are {sorted(locked_platforms)}")

    workflow = read(".github/workflows/ci.yml")
    if workflow.count(f'MISE_VERSION: "{manager_version}"') != 1:
        errors.append(f".github/workflows/ci.yml: expected MISE_VERSION {manager_version!r} from mise.toml")
    if workflow.count("uses: jdx/mise-action@") != 2:
        errors.append(".github/workflows/ci.yml: expected one mise-action step in the checks and analysis jobs")

    for relative in ("analysis/README.md", "docs/data/README.md"):
        text = read(relative)
        if "mise.toml" not in text or "mise install --locked" not in text:
            errors.append(f"{relative}: setup instructions must use mise.toml and mise install --locked")
        if "tools.toml" in text:
            errors.append(f"{relative}: stale tools.toml reference")
    analysis_readme = read("analysis/README.md")
    for marker in ("analysis/pyproject.toml", "analysis/.python-version"):
        if marker not in analysis_readme:
            errors.append(f"analysis/README.md: missing analysis pin reference {marker!r}")
    if "uv.toml" in analysis_readme:
        errors.append("analysis/README.md: stale uv.toml reference")
    for marker in (
        "winget install --id jdx.mise",
        "mise self-update $miseVersion",
        "mise trust --yes",
        "mise install --locked",
        "uv python install",
    ):
        if marker not in analysis_readme:
            errors.append(f"analysis/README.md: missing setup instruction {marker!r}")
    wsl_readme = analysis_readme.split("## WSL (Linux) checks", 1)[-1]
    for marker in ("MISE_VERSION=\"v${mise_version}\"", "mise install --locked", "mise exec -- just ci-rust"):
        if marker not in wsl_readme:
            errors.append(f"analysis/README.md: WSL setup missing {marker!r}")

    contract = read(HISTORICAL_CONTRACT)
    if HISTORICAL_MARKER not in contract:
        errors.append(f"{HISTORICAL_CONTRACT}: append a Later update marking the old tool pins historical")

    for relative in _tracked_paths(root):
        path = relative.as_posix()
        if path in PIN_FILES or path == HISTORICAL_CONTRACT or path.startswith(("docs/data/", ".mise/", "target/")):
            continue
        if relative.suffix.lower() not in SCANNED_SUFFIXES:
            continue
        try:
            text = (root / relative).read_text(encoding="utf-8")
        except (OSError, UnicodeError):
            continue
        errors.extend(hardcoded_tool_versions(text, path))
    return errors


def main() -> int:
    try:
        errors = check_tool_versions()
    except (OSError, RuntimeError, ValueError, tomllib.TOMLDecodeError) as error:
        print(error, file=sys.stderr)
        return 1
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("mise.toml/mise.lock match uv, Python, CI, and setup documentation")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
