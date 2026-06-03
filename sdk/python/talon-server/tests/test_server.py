from talon_server.server import _config_yaml


def test_config_uses_sqlite_and_local_socket() -> None:
    config = _config_yaml(None)
    assert "driver: sqlite" in config
    assert "driver: local_socket" in config

