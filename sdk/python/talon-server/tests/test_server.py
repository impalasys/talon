import base64
import json
from types import MappingProxyType

from talon_server import JwtOptions, Options, Server, authorization_header, mint_jwt
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


def test_mint_jwt_creates_scoped_talon_token() -> None:
    token = mint_jwt(
        "secret",
        JwtOptions(subject="browser-demo", ttl_seconds=60, namespace="demo", agent="copilot", channel="chat"),
    )
    header_segment, payload_segment, signature = token.split(".")
    assert signature
    header = _decode(header_segment)
    payload = _decode(payload_segment)
    assert header == {"alg": "HS256", "typ": "JWT"}
    assert payload["sub"] == "browser-demo"
    assert payload["aud"] == "talon"
    assert payload["talon:ns"] == "demo"
    assert payload["talon:agent"] == "copilot"
    assert payload["talon:channel"] == "chat"
    assert authorization_header(token) == f"Bearer {token}"


def test_mint_jwt_requires_namespace_for_channel_scope() -> None:
    try:
        mint_jwt("secret", JwtOptions(channel="chat"))
    except ValueError as error:
        assert "namespace" in str(error)
    else:
        raise AssertionError("expected ValueError")


def _decode(segment: str) -> dict[str, object]:
    padded = segment + "=" * (-len(segment) % 4)
    return json.loads(base64.urlsafe_b64decode(padded).decode("utf-8"))
