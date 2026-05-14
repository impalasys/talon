#!/bin/bash
set -e

# Securely fetch secrets from macOS Keychain if available, otherwise rely on .env or existing env
KEYCHAIN_CEREBRAS=$(security find-generic-password -a talon-engine -s CEREBRAS_API_KEY -w 2>/dev/null || true)
if [ -n "$KEYCHAIN_CEREBRAS" ]; then
  export CEREBRAS_API_KEY="$KEYCHAIN_CEREBRAS"
fi

KEYCHAIN_NOVITA=$(security find-generic-password -a talon-engine -s NOVITA_API_KEY -w 2>/dev/null || true)
if [ -n "$KEYCHAIN_NOVITA" ]; then
  export NOVITA_API_KEY="$KEYCHAIN_NOVITA"
fi

KEYCHAIN_ANTHROPIC=$(security find-generic-password -a talon-engine -s ANTHROPIC_API_KEY -w 2>/dev/null || true)
if [ -n "$KEYCHAIN_ANTHROPIC" ]; then
  export ANTHROPIC_API_KEY="$KEYCHAIN_ANTHROPIC"
fi

if [ -z "$CEREBRAS_API_KEY" ] && ! grep -q "CEREBRAS_API_KEY=" .env 2>/dev/null; then
  echo "Warning: CEREBRAS_API_KEY not found in keychain, environment, or .env file."
fi

if [ -z "$NOVITA_API_KEY" ] && ! grep -q "NOVITA_API_KEY=" .env 2>/dev/null; then
  echo "Warning: NOVITA_API_KEY not found in keychain, environment, or .env file."
fi

if [ -z "$ANTHROPIC_API_KEY" ] && ! grep -q "ANTHROPIC_API_KEY=" .env 2>/dev/null; then
  echo "Warning: ANTHROPIC_API_KEY not found in keychain, environment, or .env file."
fi

echo "Starting Talon Autonomous Infrastructure (Engine + UI)..."
# Now running from within the talon folder
docker compose down --remove-orphans >/dev/null 2>&1 || true
docker-compose up --build -d

echo "---------------------------------------------------"
echo "Talon is spinning up!"
echo "Engine Gateway: http://localhost:18789"
echo "Debugger Web UI: http://localhost:3000"
echo "---------------------------------------------------"
echo "To view logs: docker-compose logs -f"
