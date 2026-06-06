# talon-server

Starts `talon-node` as a local subprocess with SQLite and a `local_socket`
broker for tests and development. By default the helper writes a temporary
`talon.yaml` and stores SQLite data under that temporary directory.

Set `TALON_NODE_PATH` to a local `talon-node` binary, or let the helper download
one from the Talon GitHub releases.

Pass `jwt_secret` to start the gateway in JWT-auth mode, then mint scoped
browser or test tokens with `mint_jwt`:

```python
from talon_server import JwtOptions, Options, mint_jwt, start

secret = "dev-secret"
server = start(Options(jwt_secret=secret))
token = mint_jwt(secret, JwtOptions(subject="browser-demo", namespace="demo", agent="copilot"))
```

To provide the Talon runtime config directly, pass `config`:

```python
server = start(Options(config={
    "workspace_dir": ".",
    "control_plane": {
        "database": {"driver": "sqlite", "data_dir": ".talon-data"},
        "message_broker": {"driver": "local_socket"},
    },
}))
```

For the common local SQLite case, `data_dir` is a convenience overlay:

```python
server = start(Options(data_dir=".talon-data"))
```

To own the full runtime configuration file, pass `config_path` pointing at a
`talon.yaml` or `talon.json`. When `config_path` is set, all config settings
should live in that file.
