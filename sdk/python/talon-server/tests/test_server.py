from types import MappingProxyType

from talon_server import Options, Server, authorization_header
from talon_server.server import _config_with_data_dir, _default_config


def test_config_uses_sqlite_and_local_socket() -> None:
    config = _default_config(None, "/tmp/talon-data")
    assert config["control_plane"]["database"]["driver"] == "sqlite"
    assert config["control_plane"]["database"]["data_dir"] == "/tmp/talon-data"
    assert config["control_plane"]["message_broker"]["driver"] == "local_socket"


def test_config_can_specify_general_talon_settings() -> None:
    config = _config_with_data_dir(
        {
            "workspace_dir": "/tmp/workspace",
            "default_provider": "openai",
            "control_plane": {
                "database": {"driver": "sqlite"},
                "message_broker": {"driver": "local_socket"},
            },
        },
        None,
    )
    assert config["workspace_dir"] == "/tmp/workspace"
    assert config["default_provider"] == "openai"


def test_config_with_data_dir_preserves_custom_mapping_values() -> None:
    config = _config_with_data_dir(
        MappingProxyType(
            {
                "control_plane": MappingProxyType(
                    {
                        "database": MappingProxyType({"driver": "sqlite"}),
                        "message_broker": MappingProxyType({"driver": "local_socket"}),
                    }
                )
            }
        ),
        None,
    )
    assert config["control_plane"]["database"]["driver"] == "sqlite"
    assert isinstance(config["control_plane"], dict)
    assert isinstance(config["control_plane"]["database"], dict)


def test_config_path_rejects_generated_config_options() -> None:
    try:
        Server.start(Options(config_path="talon.yaml", config={"workspace_dir": "."}))
    except ValueError as error:
        assert "config_path cannot be combined" in str(error)
    else:
        raise AssertionError("expected ValueError")


def test_authorization_header_formats_bearer_token() -> None:
    token = "test-token"
    assert authorization_header(token) == f"Bearer {token}"


def test_authorization_header_requires_token() -> None:
    try:
        authorization_header(" ")
    except ValueError as error:
        assert "token is required" in str(error)
    else:
        raise AssertionError("expected ValueError")
