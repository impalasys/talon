from typing import Any

import conftest


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
