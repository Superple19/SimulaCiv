"""Read the canonical tool pins and check duplicated installation references."""

from __future__ import annotations

import sys
import tomllib
from pathlib import Path


ROOT = Path(__file__).resolve().parents[2]
TOOLS_FILE = ROOT / "tools.toml"
REQUIRED_TOOLS = {"uv", "python", "quarto", "just", "vale", "prek", "dvc"}


def load_tool_versions(root: Path = ROOT) -> dict[str, dict[str, str]]:
    path = root / "tools.toml"
    document = tomllib.loads(path.read_text(encoding="utf-8"))
    tools = document.get("tools")
    if not isinstance(tools, dict) or tools.keys() != REQUIRED_TOOLS:
        raise ValueError(f"{path.relative_to(root)}: expected tool entries {sorted(REQUIRED_TOOLS)}")
    for name, values in tools.items():
        if not isinstance(values, dict) or not isinstance(values.get("version"), str):
            raise ValueError(f"{path.relative_to(root)}: {name} requires a version string")
    return tools


def tool_version(name: str, root: Path = ROOT) -> str:
    return load_tool_versions(root)[name]["version"]


def check_repository_pins(root: Path = ROOT) -> list[str]:
    tools = load_tool_versions(root)
    versions = {name: entry["version"] for name, entry in tools.items()}
    errors: list[str] = []

    def read(relative: str) -> str:
        path = root / relative
        try:
            return path.read_text(encoding="utf-8")
        except OSError as error:
            errors.append(f"{relative}: cannot read tool pin source: {error}")
            return ""

    uv_toml = read("uv.toml")
    try:
        declared_uv = tomllib.loads(uv_toml).get("required-version")
    except tomllib.TOMLDecodeError as error:
        errors.append(f"uv.toml: invalid TOML while checking uv {versions['uv']}: {error}")
        declared_uv = None
    if declared_uv != f"=={versions['uv']}":
        errors.append(f"uv.toml: expected uv {versions['uv']}, found {declared_uv!r}")

    declared_python = read(".python-version").strip()
    if declared_python != versions["python"]:
        errors.append(f".python-version: expected Python {versions['python']}, found {declared_python!r}")

    workflow = read(".github/workflows/ci.yml")
    workflow_markers = {
        f'version: "{versions["uv"]}"': 1,
        f'version: "{versions["quarto"]}"': 1,
        f'just-version: "{versions["just"]}"': 2,
        f"vale_{versions['vale']}_Linux_64-bit.tar.gz": 1,
        f"{tools['vale']['linux_64_sha256']}  /tmp/vale.tar.gz": 1,
        f"releases/download/v{versions['prek']}/prek-x86_64-unknown-linux-gnu.tar.gz": 1,
        f"{tools['prek']['x86_64_unknown_linux_gnu_sha256']}  /tmp/prek.tar.gz": 1,
    }
    for marker, expected_count in workflow_markers.items():
        found = workflow.count(marker)
        if found != expected_count:
            errors.append(
                f".github/workflows/ci.yml: expected {expected_count} occurrence(s) of {marker!r} "
                f"from tools.toml, found {found}"
            )

    analysis_readme = read("analysis/README.md")
    analysis_markers = (
        "../tools.toml",
        f"Python {versions['python']}, uv {versions['uv']}",
        "Install the tools at the versions listed in [tools.toml](../tools.toml)",
        f"https://astral.sh/uv/{versions['uv']}/install.sh",
        f"quarto-cli/releases/download/v{versions['quarto']}/quarto-{versions['quarto']}-linux-amd64.tar.gz",
        f"{tools['quarto']['linux_amd64_sha256']}  /tmp/quarto-{versions['quarto']}-linux-amd64.tar.gz",
        f"--version {versions['just']} --locked",
        f"vale-cli/vale/releases/download/v{versions['vale']}/vale_{versions['vale']}_Linux_64-bit.tar.gz",
        f"{tools['vale']['linux_64_sha256']}  /tmp/vale.tar.gz",
        f"j178/prek/releases/download/v{versions['prek']}/prek-x86_64-unknown-linux-gnu.tar.gz",
        f"{tools['prek']['x86_64_unknown_linux_gnu_sha256']}  /tmp/prek.tar.gz",
        f"dvc=={versions['dvc']}",
    )
    for marker in analysis_markers:
        if marker not in analysis_readme:
            errors.append(f"analysis/README.md: missing expected install pin {marker!r} from tools.toml")

    data_readme = read("docs/data/README.md")
    data_markers = ("../../tools.toml",)
    for marker in data_markers:
        if marker not in data_readme:
            errors.append(f"docs/data/README.md: missing expected install pin {marker!r} from tools.toml")

    return errors


def main() -> int:
    try:
        errors = check_repository_pins()
    except (OSError, ValueError) as error:
        print(error, file=sys.stderr)
        return 1
    if errors:
        print("\n".join(errors), file=sys.stderr)
        return 1
    print("Tool pins in CI and install documentation match tools.toml")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
