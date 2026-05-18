# `@talonai/copilot`

`@talonai/copilot` provides a React chat panel for Talon agent sessions.

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
