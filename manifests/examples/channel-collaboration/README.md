# Channel Collaboration Tutorial Assets

This example creates a `channel-collaboration` namespace with:

- `incident-room` channel
- `triage-agent` routed by `@triage-agent` or `@triage`
- `scribe-agent` routed manually by posting with `subscription_names: ["scribe"]`

The default compose stack can apply these resources with:

```bash
docker compose --profile tutorial-channels up --build -d
```

Post a channel message through the gateway edge:

```bash
curl -sS http://localhost:18789/v1/ns/channel-collaboration/channels/incident-room/messages \
  -X POST \
  -H 'content-type: application/json' \
  -d '{"authorKind":"user","author":"operator","content":"@triage-agent production checkout latency is elevated. What should we do first?"}'
```
