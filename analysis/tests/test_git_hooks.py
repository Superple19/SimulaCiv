from pathlib import Path

from simciv_analysis.git_hooks import dvc_output_paths, large_file_violations


def test_large_file_check_allows_dvc_output_and_ignores_unstaged(tmp_path):
    output = tmp_path / "docs/data/sample/raw"
    output.mkdir(parents=True)
    (output / "large.bin").write_bytes(b"x" * (1024 * 1024 + 1))
    outside = tmp_path / "large.bin"
    outside.write_bytes(b"x" * (1024 * 1024 + 1))

    violations = large_file_violations(
        tmp_path,
        ["docs/data/sample/raw/large.bin", "large.bin"],
        {"docs/data/sample/raw/large.bin", "large.bin"},
        {"docs/data/sample/raw"},
    )

    assert violations == [("large.bin", 1024 * 1024 + 1)]
    assert large_file_violations(tmp_path, ["large.bin"], set(), set()) == []


def test_dvc_output_paths_read_pipeline_wdir(tmp_path):
    pipeline = tmp_path / "docs/data/stage/dvc.yaml"
    pipeline.parent.mkdir(parents=True)
    pipeline.write_text(
        "stages:\n  sample:\n    wdir: ../../../\n    outs:\n    - docs/data/stage/raw\n",
        encoding="utf-8",
    )

    assert "docs/data/stage/raw" in dvc_output_paths(tmp_path)
