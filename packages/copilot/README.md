# `@talonai/copilot`

`@talonai/copilot` provides React panels for Talon agent sessions and channels.

## Install

```bash
pnpm add @talonai/copilot
```

`react` and `react-dom` are required peer dependencies.

## Usage

```tsx
import { TalonCopilot } from "@talonai/copilot";

export function App() {
  return (
    <TalonCopilot
      namespace="support"
      agent="docs"
      gatewayUrl="http://localhost:18789"
      authToken="secret-token"
    />
  );
}
```

You can also inject a gateway client for session CRUD:

```tsx
<TalonCopilot
  namespace="support"
  agent="docs"
  gatewayUrl="http://localhost:18789"
  gatewayClient={client}
  sessionId={sessionId}
  onSessionChange={(nextSessionId) => setSessionId(nextSessionId)}
/>
```

Channels can be rendered with the same package:

```tsx
import { TalonChannel } from "@talonai/copilot";

<TalonChannel
  namespace="support"
  channel="incident-room"
  gatewayUrl="http://localhost:18789"
  authToken={`Bearer ${channelJwt}`}
  renderMessageActions={(message) => {
    const agent = message.sourceAgent || message.source_agent;
    const sessionId = message.sourceSessionId || message.source_session_id;
    return agent && sessionId ? <button>Open session</button> : null;
  }}
/>
```

For untrusted frontends, mint a short-lived channel token on your backend and pass it as a Bearer token:

```bash
talon-cli --jwt-secret "$GATEWAY_JWT_SECRET" auth channel-token \
  --namespace support \
  --channel incident-room \
  --ttl-seconds 900
```

The token is scoped to channel message APIs for that namespace/channel only.
