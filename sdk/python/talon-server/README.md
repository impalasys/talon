# talon-server

Starts `talon-node` as a local subprocess with a temporary SQLite database and
`local_socket` broker for tests and development.

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
