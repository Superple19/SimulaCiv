import json
from pathlib import Path

import pytest

from simciv_analysis.provenance import (
    digest_files,
    finalize_summary,
    source_digest,
    verify_summary,
)


def test_digest_files_is_sorted_and_content_sensitive(tmp_path):
    first = tmp_path / "spec.toml"
    second = tmp_path / "harness.rs"
    first.write_text("seed = 3\n", encoding="utf-8")
    second.write_text("fn main() {}\n", encoding="utf-8")

    expected = digest_files([first, second], tmp_path)
    assert expected == digest_files([second, first], tmp_path)
    second.write_text("fn main() { /* changed */ }\n", encoding="utf-8")
    assert expected != digest_files([first, second], tmp_path)


def test_verify_summary_checks_source_spec_and_harness_digests(tmp_path):
    source = tmp_path / "crates" / "sim-model" / "src" / "lib.rs"
    spec = tmp_path / "specs" / "case.toml"
    harness = tmp_path / "examples" / "measure.rs"
    root_inputs = [
        tmp_path / name
        for name in ("Cargo.toml", "Cargo.lock", "rust-toolchain.toml")
    ]
    crate_manifests = [
        tmp_path / "crates" / crate / "Cargo.toml"
        for crate in ("sim-model", "sim-core")
    ]
    for path, contents in [
        (source, "// source\n"),
        (spec, "seed = 3\n"),
        (harness, "fn main() {}\n"),
        *((path, "manifest = true\n") for path in root_inputs + crate_manifests),
    ]:
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
    summary = tmp_path / "summary.json"
    summary.write_text(
        json.dumps(
            {
                "source_digest": source_digest(tmp_path),
                "spec_digest": digest_files([spec], tmp_path),
                "harness_digest": digest_files([harness], tmp_path),
                "spec_files": ["specs/case.toml"],
                "harness_files": ["examples/measure.rs"],
            }
        ),
        encoding="utf-8",
    )
    verify_summary(summary, tmp_path)

    source.write_text("// newer source\n", encoding="utf-8")
    with pytest.raises(ValueError, match="source_digest mismatch"):
        verify_summary(summary, tmp_path)


def test_finalize_summary_records_dvc_and_content_hashes(tmp_path):
    source = tmp_path / "crates" / "sim-model" / "src" / "lib.rs"
    spec = tmp_path / "specs" / "case.toml"
    harness = tmp_path / "examples" / "measure.rs"
    raw = tmp_path / "docs" / "data" / "m5_05" / "case" / "raw"
    for path, contents in (
        (source, "// source\n"),
        (spec, "seed = 3\n"),
        (harness, "fn main() {}\n"),
        (tmp_path / "Cargo.toml", "workspace = true\n"),
        (tmp_path / "Cargo.lock", "version = 4\n"),
        (tmp_path / "rust-toolchain.toml", "[toolchain]\nchannel = 'stable'\n"),
        (tmp_path / "crates" / "sim-model" / "Cargo.toml", "name = 'sim-model'\n"),
        (tmp_path / "crates" / "sim-core" / "Cargo.toml", "name = 'sim-core'\n"),
        (raw / "samples.jsonl", '{"sample":1}\n'),
    ):
        path.parent.mkdir(parents=True, exist_ok=True)
        path.write_text(contents, encoding="utf-8")
    summary_path = tmp_path / "docs" / "data" / "m5_05" / "case" / "summary.json"
    summary_path.parent.mkdir(parents=True, exist_ok=True)
    summary_path.write_text(
        json.dumps(
            {
                "spec_files": ["specs/case.toml"],
                "harness_files": ["examples/measure.rs"],
                "raw_outputs": ["docs/data/m5_05/case/raw"],
            }
        ),
        encoding="utf-8",
    )
    (tmp_path / "dvc.lock").write_text(
        "stages:\n  case:\n    outs:\n    - md5: 0123456789abcdef0123456789abcdef.dir\n"
        "      path: docs/data/m5_05/case/raw\n",
        encoding="utf-8",
    )

    result = finalize_summary(summary_path, tmp_path)
    assert result["outputs"] == [
        {
            "path": "docs/data/m5_05/case/raw",
            "dvc_md5": "0123456789abcdef0123456789abcdef.dir",
            "sha256": digest_files([raw / "samples.jsonl"], tmp_path),
        }
    ]
    verify_summary(summary_path, tmp_path)
