from pathlib import Path
from types import SimpleNamespace
from typing import Any

import conftest
from e2e import stack


def test_conftest_binary_helpers_cover_candidate_and_path_resolution(monkeypatch: Any) -> None:
    # Verify the binary lookup helpers try expected filename variants and fall
    # back to the system PATH when local artifacts are absent.
    assert list(conftest.binary_candidates("talon_server")) == [
        "talon_server",
        "talon-server",
    ]
    assert list(conftest.binary_candidates("talon-worker")) == [
        "talon-worker",
        "talon_worker",
    ]

    monkeypatch.delenv("BUILD_WORKSPACE_DIRECTORY", raising=False)
    monkeypatch.setattr(conftest, "get_runfile_binary_path", lambda name: None)
    monkeypatch.setattr(conftest.shutil, "which", lambda name: f"/usr/bin/{name}")
    assert conftest.get_binary_path("talon_server") == "/usr/bin/talon_server"


def test_standalone_bazel_binary_lookup(monkeypatch: Any, tmp_path: Path) -> None:
    binary = tmp_path / "bazel-bin" / "talon-server"
    binary.parent.mkdir()
    binary.touch()
    monkeypatch.setattr(stack, "REPO_ROOT", tmp_path)
    monkeypatch.setattr(stack, "get_runfile_binary_path", lambda name: None)
    monkeypatch.delenv("BUILD_WORKSPACE_DIRECTORY", raising=False)
    assert stack.get_binary_path("talon_server") == str(binary)

    # bazel run can start outside the source tree and supplies the workspace.
    monkeypatch.setattr(stack, "REPO_ROOT", tmp_path / "other")
    monkeypatch.setenv("BUILD_WORKSPACE_DIRECTORY", str(tmp_path))
    assert stack.get_binary_path("talon_server") == str(binary)


def test_standalone_bazel_runfiles_lookup(monkeypatch: Any, tmp_path: Path) -> None:
    binary = tmp_path / "talon-server"
    binary.touch()
    manifest = SimpleNamespace(
        CurrentRepository=lambda: "",
        Rlocation=lambda name: str(binary) if name == "_main/talon-server" else None,
    )
    monkeypatch.setattr(stack, "runfiles", SimpleNamespace(Create=lambda: manifest))
    assert stack.get_runfile_binary_path("talon_server") == str(binary)
