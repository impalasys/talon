# `@impalasys/talon-chat`

`@impalasys/talon-chat` provides React panels for Talon agent sessions and channels.

## Install

```bash
pnpm add @impalasys/talon-chat
```

`react` and `react-dom` are required peer dependencies.

## Usage

```tsx
import { TalonSession } from "@impalasys/talon-chat";

export function App() {
  return (
    <TalonSession
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
<TalonSession
  namespace="support"
  agent="docs"
  gatewayUrl="http://localhost:18789"
  gatewayClient={client}
  sessionId={sessionId}
  onSessionChange={(nextSessionId) => setSessionId(nextSessionId)}
/>
```

`TalonCopilot` is still exported as an alias for existing integrations.

## Commands

Both `TalonSession` and `TalonChannel` can intercept slash commands before they are sent as chat messages. Enable the built-in session `/clear` command with `enabledBuiltInCommands`:

```tsx
<TalonSession
  namespace="support"
  agent="docs"
  gatewayUrl="http://localhost:18789"
  enabledBuiltInCommands={["clear"]}
/>
```

For sessions, `/clear` calls the gateway session clear API when a session is active and then clears the visible transcript. Channels do not include a built-in clear command because channel messages are shared history, not per-session transcript state.

You can also provide custom commands:

```tsx
<TalonChannel
  namespace="support"
  channel="incident-room"
  gatewayUrl="http://localhost:18789"
  commands={[
    {
      name: "ack",
      description: "Acknowledge the active incident room.",
      run: ({ target }) => console.log(`Acknowledged ${target.channel}`),
    },
  ]}
/>
```

Channels can be rendered with the same package:

```tsx
import { TalonChannel } from "@impalasys/talon-chat";

<TalonChannel
  namespace="support"
  channel="incident-room"
  gatewayUrl="http://localhost:18789"
  authToken={`Bearer ${channelJwt}`}
  disableUserInput
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

## Storybook and Chromatic

Run the component preview locally:

```bash
pnpm --filter @impalasys/talon-chat storybook
```

Build the static Storybook:

```bash
pnpm --filter @impalasys/talon-chat build-storybook
```

Publish visual snapshots to Chromatic:

```bash
CHROMATIC_PROJECT_TOKEN=chpt_... pnpm --filter @impalasys/talon-chat chromatic
```

GitHub Actions also publishes Chromatic builds for `packages/talon-chat` changes. Configure the repository secret `CHROMATIC_PROJECT_TOKEN` before enabling that check.
