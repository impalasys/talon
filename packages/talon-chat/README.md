# `@impalasys/talon-chat`

`@impalasys/talon-chat` provides React panels for Talon agent sessions and channels.

## Install

```bash
pnpm add @impalasys/talon-chat @impalasys/talon-client
```

`react` and `react-dom` are required peer dependencies.

## Usage

```tsx
import { createTalonClient } from "@impalasys/talon-client";
import { TalonSession } from "@impalasys/talon-chat";

const gatewayClient = createTalonClient({
  baseUrl: "http://localhost:50051",
  authToken: "secret-token",
});

export function App() {
  return (
    <TalonSession
      namespace="support"
      agent="docs"
      gatewayClient={gatewayClient}
    />
  );
}
```

Keep session state in your app when you want to control which transcript is shown:

```tsx
<TalonSession
  namespace="support"
  agent="docs"
  gatewayClient={gatewayClient}
  sessionId={sessionId}
  onSessionChange={(nextSessionId) => setSessionId(nextSessionId)}
/>
```

`TalonCopilot` is still exported as an alias for existing integrations.

## Image uploads

`TalonSession` can accept image attachments when you provide an `onImageUpload`
callback. The callback is responsible for uploading bytes to your object store
or backend upload route and returning the Talon `ObjectRef`. The chat request
then sends only text plus object references.

```tsx
<TalonSession
  namespace="support"
  agent="docs"
  gatewayClient={gatewayClient}
  onImageUpload={async ({ file, namespace, agent, sessionId, signal }) => {
    const form = new FormData();
    form.set("file", file);
    form.set("namespace", namespace);
    form.set("agent", agent);
    form.set("sessionId", sessionId);

    const response = await fetch("/api/talon/objects", {
      method: "POST",
      body: form,
      signal,
    });
    if (!response.ok) {
      throw new Error(`Upload failed: ${response.status}`);
    }
    return response.json();
  }}
/>
```

The uploader may return either an `ObjectRef` directly or `{ object:
ObjectRef }`. Supported image types default to PNG, JPEG, GIF, and WebP. Use
`acceptedImageTypes`, `maxImageAttachments`, and `maxImageBytes` to tune the
composer validation.

## Commands

Both `TalonSession` and `TalonChannel` can intercept slash commands before they are sent as chat messages. Enable the built-in session `/clear` command with `enabledBuiltInCommands`:

```tsx
<TalonSession
  namespace="support"
  agent="docs"
  gatewayClient={gatewayClient}
  enabledBuiltInCommands={["clear"]}
/>
```

For sessions, `/clear` calls the gateway session clear API when a session is active and then clears the visible transcript. Channels do not include a built-in clear command because channel messages are shared history, not per-session transcript state.

You can also provide custom commands:

```tsx
<TalonChannel
  namespace="support"
  channel="incident-room"
  gatewayClient={gatewayClient}
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
import { createTalonClient } from "@impalasys/talon-client";
import { TalonChannel } from "@impalasys/talon-chat";

const gatewayClient = createTalonClient({
  baseUrl: "http://localhost:50051",
  authToken: `Bearer ${channelJwt}`,
});

<TalonChannel
  namespace="support"
  channel="incident-room"
  gatewayClient={gatewayClient}
  disableUserInput
  renderMessageActions={(message) => {
    const agent = message.sourceAgent || message.source_agent;
    const sessionId = message.sourceSessionId || message.source_session_id;
    return agent && sessionId ? <button>Open session</button> : null;
  }}
/>
```

For local development with untrusted frontends, mint a short-lived channel token from the platform private PEM and pass it as a Bearer token:

```bash
TALON_PLATFORM_JWT_ISSUER=https://talon.localhost \
talon-cli auth local-token \
  --private-key-pem-file ./talon-jwt-private-key.pem \
  --namespace support \
  --channel incident-room \
  --ttl-seconds 900
```

In production, mint frontend tokens through your trusted backend after OIDC-authenticated authorization.

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
