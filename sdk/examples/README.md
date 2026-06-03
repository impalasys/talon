# Talon Server SDK Examples

These examples show the intended SDK application loop:

1. Start a local Talon node with the language-specific `talon-server` package.
2. Connect to its gRPC gateway with the generated `talon-client` package.
3. Create and list a namespace.
4. Stop the local server and clean up temporary state.

Until release packaging fills the bundled SDK binaries, point examples at a locally built `talon-node`:

```bash
cargo build --bin talon-node
export TALON_NODE_PATH="$(pwd)/target/debug/talon-node"
```

Then run the language example you want:

```bash
cd sdk/examples/go && go run .
cd sdk/examples/rust && cargo run
cd sdk/examples/java && gradle run
cd sdk/examples/js && pnpm install && pnpm start
cd sdk/examples/python && python3 -m venv .venv && . .venv/bin/activate && pip install -e ../../python/talon-client -e ../../python/talon-server . && python main.py
```

Each example uses SQLite and the `local_socket` broker through the `talon-server` defaults, so it is suitable for local development and tests rather than production hosting.
