import threading
import time
from collections.abc import Sequence
from typing import Optional

import grpc

from talon_client.proto.talon.v1 import auth_pb2, auth_pb2_grpc


class ApiKeyTokenSource:
    def __init__(self, channel: grpc.Channel, api_key: str, refresh_skew_seconds: int = 60):
        api_key = api_key.strip()
        if not api_key:
            raise ValueError("api_key is required")
        self._stub = auth_pb2_grpc.AuthServiceStub(channel)
        self._api_key = api_key
        self._refresh_skew_seconds = refresh_skew_seconds
        self._lock = threading.Lock()
        self._token: Optional[str] = None
        self._expires_at = 0

    def token(self) -> str:
        now = int(time.time())
        if self._token and self._expires_at > now + self._refresh_skew_seconds:
            return self._token
        with self._lock:
            now = int(time.time())
            if self._token and self._expires_at > now + self._refresh_skew_seconds:
                return self._token
            response = self._stub.ExchangeApiKey(
                auth_pb2.ExchangeApiKeyRequest(api_key=self._api_key)
            )
            self._token = response.access_token
            self._expires_at = response.expires_at
            return self._token


class ApiKeyAuthMetadataPlugin(grpc.AuthMetadataPlugin):
    def __init__(self, token_source: ApiKeyTokenSource):
        self._token_source = token_source

    def __call__(self, context, callback):
        try:
            callback((("authorization", f"Bearer {self._token_source.token()}"),), None)
        except Exception as exc:
            callback((), exc)


def api_key_call_credentials(
    channel: grpc.Channel,
    api_key: str,
    refresh_skew_seconds: int = 60,
) -> grpc.CallCredentials:
    token_source = ApiKeyTokenSource(channel, api_key, refresh_skew_seconds)
    return grpc.metadata_call_credentials(ApiKeyAuthMetadataPlugin(token_source))


def secure_channel_with_api_key(
    target: str,
    api_key: str,
    *,
    channel_credentials: Optional[grpc.ChannelCredentials] = None,
    options: Optional[Sequence[tuple[str, object]]] = None,
) -> grpc.Channel:
    channel_credentials = channel_credentials or grpc.ssl_channel_credentials()
    bootstrap_channel = grpc.secure_channel(target, channel_credentials, options=options)
    call_credentials = api_key_call_credentials(bootstrap_channel, api_key)
    credentials = grpc.composite_channel_credentials(channel_credentials, call_credentials)
    return grpc.secure_channel(target, credentials, options=options)
