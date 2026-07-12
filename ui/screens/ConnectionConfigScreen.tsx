import { useRef, useState } from 'react';
import { ArrowUpRight, ChevronDown } from 'lucide-react';
import { motion } from 'framer-motion';
import { cn } from '../utils/cn';

export type ConnectionConfigValues = {
  gatewayUrl: string;
  apiKey: string;
  jwtToken: string;
  namespace: string;
};

export type ConnectionConfigScreenProps = {
  gatewayUrl: string;
  jwtToken: string;
  apiKey: string;
  namespace: string;
  isConnecting: boolean;
  googleSsoEnabled: boolean;
  googleSsoError: string | null;
  connectionError: string | null;
  onGatewayUrlChange: (value: string) => void;
  onJwtTokenChange: (value: string) => void;
  onApiKeyChange: (value: string) => void;
  onNamespaceChange: (value: string) => void;
  onGoogleSignIn: () => void;
  onConnect: (values: ConnectionConfigValues) => void;
};

function GoogleIcon() {
  return (
    <svg aria-hidden="true" viewBox="0 0 24 24" className="h-4 w-4">
      <path
        fill="#4285F4"
        d="M23.49 12.27c0-.82-.07-1.43-.22-2.06H12v3.75h6.62c-.13.93-.85 2.33-2.45 3.27l-.02.13 3.56 2.61.25.02c2.3-2 3.53-4.95 3.53-7.72Z"
      />
      <path
        fill="#34A853"
        d="M12 24c3.28 0 6.04-1.02 8.06-2.78l-3.84-2.9c-1.03.68-2.41 1.15-4.22 1.15-3.22 0-5.95-2-6.92-4.77l-.14.01-3.7 2.71-.05.13C3.2 20.84 7.24 24 12 24Z"
      />
      <path
        fill="#FBBC05"
        d="M5.08 14.7A6.98 6.98 0 0 1 4.7 12c0-.94.14-1.85.37-2.7l-.01-.18-3.74-2.75-.12.05A11.37 11.37 0 0 0 0 12c0 2 .5 3.88 1.38 5.53l3.7-2.83Z"
      />
      <path
        fill="#EA4335"
        d="M12 4.53c2.28 0 3.82.93 4.7 1.7l3.44-3.17C18.03 1.2 15.28 0 12 0 7.24 0 3.2 3.16 1.38 6.47l3.69 2.83C6.05 6.53 8.78 4.53 12 4.53Z"
      />
    </svg>
  );
}

export function ConnectionConfigScreen({
  gatewayUrl,
  jwtToken,
  apiKey,
  namespace,
  isConnecting,
  googleSsoEnabled,
  googleSsoError,
  connectionError,
  onGatewayUrlChange,
  onJwtTokenChange,
  onApiKeyChange,
  onNamespaceChange,
  onGoogleSignIn,
  onConnect,
}: ConnectionConfigScreenProps) {
  const formRef = useRef<HTMLFormElement>(null);
  const [advancedOpen, setAdvancedOpen] = useState(false);

  const submitConnection = () => {
    const form = formRef.current;
    if (!form) return;
    const formData = new FormData(form);
    onConnect({
      gatewayUrl: String(formData.get('gatewayUrl') || ''),
      apiKey: String(formData.get('apiKey') || ''),
      jwtToken: String(formData.get('jwtToken') || ''),
      namespace: String(formData.get('namespace') || ''),
    });
  };

  return (
    <main className="grid min-h-screen min-w-0 grid-cols-1 overflow-hidden bg-background text-foreground lg:grid-cols-2">
      <section className="flex min-h-screen items-center px-6 py-10 sm:px-10 lg:px-16">
        <motion.form
          ref={formRef}
          initial={{ opacity: 0, x: 10 }}
          animate={{ opacity: 1, x: 0 }}
          noValidate
          onSubmit={(event) => {
            event.preventDefault();
            submitConnection();
          }}
          className="mx-auto w-full max-w-[440px] space-y-5"
        >
          <div className="pb-5 text-center">
            <h1 className="text-[34px] font-semibold leading-tight tracking-tight text-foreground sm:text-[40px]">
              Connect to Talon
            </h1>
          </div>

          <div className="space-y-2.5">
            <label htmlFor="gateway-url-input" className="text-sm font-medium text-foreground">Gateway URL</label>
            <input
              id="gateway-url-input"
              name="gatewayUrl"
              type="text"
              inputMode="url"
              required
              defaultValue={gatewayUrl}
              onChange={(event) => onGatewayUrlChange(event.target.value)}
              className="h-12 w-full rounded-lg border border-border/80 bg-background px-4 font-mono text-[15px] text-foreground shadow-sm transition-shadow focus:border-ring focus:outline-none focus:ring-2 focus:ring-ring/20"
              placeholder="https://talon.impala.systems"
              disabled={isConnecting}
              autoFocus
            />
          </div>
          <div className="space-y-2.5">
            <label htmlFor="api-key-input" className="text-sm font-medium text-foreground">API Key (Optional)</label>
            <input
              id="api-key-input"
              name="apiKey"
              type="password"
              value={apiKey}
              onChange={(event) => onApiKeyChange(event.target.value)}
              className="h-12 w-full rounded-lg border border-border/80 bg-background px-4 font-mono text-[15px] text-foreground shadow-sm transition-shadow focus:border-ring focus:outline-none focus:ring-2 focus:ring-ring/20"
              placeholder="talon_sk_..."
              disabled={isConnecting}
            />
          </div>
          <div className="overflow-hidden rounded-lg border border-border/80 bg-muted/10 shadow-sm">
            <button
              type="button"
              onClick={() => setAdvancedOpen((open) => !open)}
              className="flex h-12 w-full items-center justify-between px-4 text-sm font-medium text-foreground"
              aria-expanded={advancedOpen}
              aria-controls="advanced-auth-options"
            >
              Advanced options
              <ChevronDown className={cn('h-4 w-4 text-muted-foreground transition-transform', advancedOpen && 'rotate-180')} />
            </button>
            {advancedOpen ? (
              <div id="advanced-auth-options" className="space-y-4 border-t border-border/70 px-4 py-4">
                <div className="space-y-2.5">
                  <label htmlFor="namespace-input" className="text-sm font-medium text-foreground">Namespace (Optional)</label>
                  <input
                    id="namespace-input"
                    name="namespace"
                    type="text"
                    value={namespace}
                    onChange={(event) => onNamespaceChange(event.target.value)}
                    className="h-12 w-full rounded-lg border border-border/80 bg-background px-4 font-mono text-[15px] text-foreground transition-shadow focus:border-ring focus:outline-none focus:ring-2 focus:ring-ring/20"
                    placeholder="Tenant:conic"
                    disabled={isConnecting}
                  />
                </div>
                <div className="space-y-2.5">
                  <label htmlFor="jwt-token-input" className="text-sm font-medium text-foreground">JWT Token (Optional)</label>
                  <input
                    id="jwt-token-input"
                    name="jwtToken"
                    type="password"
                    value={jwtToken}
                    onChange={(event) => onJwtTokenChange(event.target.value)}
                    className="h-12 w-full rounded-lg border border-border/80 bg-background px-4 font-mono text-[15px] text-foreground transition-shadow focus:border-ring focus:outline-none focus:ring-2 focus:ring-ring/20"
                    placeholder="Enter bearer token"
                    disabled={isConnecting}
                  />
                </div>
              </div>
            ) : null}
          </div>
          {connectionError ? <p className="text-[12px] leading-5 text-red-400">{connectionError}</p> : null}
          <button
            type="button"
            onClick={submitConnection}
            disabled={isConnecting}
            className="flex h-12 w-full items-center justify-center gap-2 rounded-lg bg-[#030716] text-sm font-semibold text-white shadow-sm transition-all hover:bg-[#111827] disabled:opacity-50"
          >
            {isConnecting ? 'Connecting...' : 'Connect'}
            {!isConnecting ? <ArrowUpRight className="h-4 w-4 stroke-[2]" /> : null}
          </button>
          {googleSsoEnabled ? (
            <div className="space-y-4 pt-3">
              <div className="flex items-center gap-3">
                <div className="h-px flex-1 bg-border" />
                <span className="text-xs font-medium text-muted-foreground">or</span>
                <div className="h-px flex-1 bg-border" />
              </div>
              <div className="space-y-2">
                <button
                  type="button"
                  onClick={onGoogleSignIn}
                  disabled={isConnecting}
                  className="flex h-12 w-full items-center justify-center gap-2 rounded-lg border border-border/80 bg-background text-sm font-medium text-foreground shadow-sm transition-all hover:bg-muted/45"
                >
                    <GoogleIcon />
                    Connect with Google
                </button>
                {googleSsoError ? <p className="text-[12px] text-red-400">{googleSsoError}</p> : null}
              </div>
            </div>
          ) : null}
        </motion.form>
      </section>
      <section className="relative hidden min-h-screen overflow-hidden bg-[#263241] text-white lg:block">
        <div className="absolute inset-0 bg-[radial-gradient(circle_at_20%_15%,rgba(203,213,225,0.16),transparent_34%),linear-gradient(135deg,#374151_0%,#263241_48%,#1f2937_100%)]" />
        <div className="absolute inset-0 opacity-[0.18] [background-image:linear-gradient(rgba(255,255,255,0.14)_1px,transparent_1px),linear-gradient(90deg,rgba(255,255,255,0.14)_1px,transparent_1px)] [background-size:42px_42px]" />
        <div className="relative flex h-full flex-col px-16 py-16">
          <div className="max-w-xl">
            <h2 className="text-[42px] font-semibold leading-tight tracking-tight text-white">
              Monitor your entire agent fleet with Sightline
            </h2>
            <p className="mt-6 max-w-md text-base leading-7 text-slate-300">
              Inspect sessions, namespaces, schedules, and runtime activity from one connected workspace.
            </p>
          </div>
        </div>
      </section>
    </main>
  );
}
