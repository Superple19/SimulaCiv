import json

from simciv_analysis.provenance import finalize_summary


def test_m5_05_summary_uses_its_stage_data_lock(tmp_path):
    for relative in (
        "Cargo.toml",
        "Cargo.lock",
        "rust-toolchain.toml",
        "crates/sim-model/Cargo.toml",
        "crates/sim-core/Cargo.toml",
    ):
        path = tmp_path / relative
        path.parent.mkdir(parents=True, exist_ok=True)
        contents = '[toolchain]\nchannel = "1.98.1"\n' if relative == "rust-toolchain.toml" else "fixture = true\n"
        path.write_text(contents, encoding="utf-8")
    raw = tmp_path / "docs" / "data" / "m5_05" / "case" / "raw"
    raw.mkdir(parents=True)
    (raw / "samples.jsonl").write_text('{"sample":1}\n', encoding="utf-8")
    summary = raw.parent / "summary.json"
    summary.write_text(
        json.dumps(
            {
                "spec_files": [],
                "harness_files": [],
                "raw_outputs": ["docs/data/m5_05/case/raw"],
            }
        ),
        encoding="utf-8",
    )
    (raw.parent.parent / "dvc.lock").write_text(
        "stages:\n  case:\n    outs:\n    - path: docs/data/m5_05/case/raw\n"
        "      md5: 0123456789abcdef0123456789abcdef.dir\n",
        encoding="utf-8",
    )

    result = finalize_summary(summary, tmp_path)

    assert result["outputs"][0]["dvc_md5"] == "0123456789abcdef0123456789abcdef.dir"
