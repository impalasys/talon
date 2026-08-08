import { useMemo } from "react";
import type { TalonBuiltInCommandName } from "../lib/commands";
import type { TalonSessionCommand } from "./TalonSessionTypes";

type UseSessionCommandsOptions = {
  clearSession: () => Promise<void>;
  commands?: TalonSessionCommand[];
  enabledBuiltInCommands?: TalonBuiltInCommandName[];
};

/** Combines host commands with the session-local clear and goal commands. */
export function useSessionCommands({ clearSession, commands, enabledBuiltInCommands }: UseSessionCommandsOptions) {
  const resolvedCommands = useMemo<TalonSessionCommand[]>(() => {
    const builtIns: TalonSessionCommand[] = [];
    if (enabledBuiltInCommands?.includes("clear")) {
      builtIns.push({ name: "clear", description: "Clear the current session history.", run: ({ clear }) => clear?.() });
    }
    if (enabledBuiltInCommands?.includes("goal")) {
      builtIns.push({ name: "goal", description: "Create or update a session Goal.", run: () => undefined });
    }
    return [...(commands ?? []), ...builtIns];
  }, [clearSession, commands, enabledBuiltInCommands]);
  const commandMenuItems = useMemo(
    () => resolvedCommands.map(({ name, aliases, description }) => ({ name, aliases, description })),
    [resolvedCommands],
  );
  return { commandMenuItems, resolvedCommands };
}
