import base64
import json

from talon_server import JwtOptions, authorization_header, mint_jwt
from talon_server.server import _config_yaml


def test_config_uses_sqlite_and_local_socket() -> None:
    config = _config_yaml(None)
    assert "driver: sqlite" in config
    assert "driver: local_socket" in config


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
