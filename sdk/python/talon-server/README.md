# talon-server

Starts `talon-node` as a local subprocess with SQLite and a `local_socket`
broker for tests and development. By default the helper writes a temporary
`talon.yaml` and stores SQLite data under that temporary directory.

Set `TALON_NODE_PATH` to a local `talon-node` binary, or let the helper download
one from the Talon GitHub releases.

JWTs should be minted by trusted Talon operator tooling, such as the Talon CLI,
rather than by SDK callers.

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
