import { useMemo } from "react";
import type { TalonBuiltInCommandName } from "../lib/commands";
import type { TalonSessionCommand } from "./TalonSessionTypes";

type UseSessionCommandsOptions = {
  clearSession: () => Promise<void>;
  compactSession: () => Promise<void>;
  doctorSession: () => Promise<void>;
  commands?: TalonSessionCommand[];
  enabledBuiltInCommands?: TalonBuiltInCommandName[];
};

/** Combines host commands with the session-local clear and goal commands. */
export function useSessionCommands({ clearSession, compactSession, doctorSession, commands, enabledBuiltInCommands }: UseSessionCommandsOptions) {
  const resolvedCommands = useMemo<TalonSessionCommand[]>(() => {
    const builtIns: TalonSessionCommand[] = [];
    if (enabledBuiltInCommands?.includes("clear")) {
      builtIns.push({ name: "clear", description: "Clear the current session history.", run: ({ clear }) => clear?.() });
    }
    if (enabledBuiltInCommands?.includes("goal")) {
      builtIns.push({ name: "goal", description: "Create or update a session Goal.", run: () => undefined });
    }
    if (enabledBuiltInCommands?.includes("compact")) {
      builtIns.push({ name: "compact", description: "Compact session history and reset provider continuation state.", run: () => compactSession() });
    }
    if (enabledBuiltInCommands?.includes("doctor")) {
      builtIns.push({ name: "doctor", description: "Diagnose and repair stale session continuation state.", run: () => doctorSession() });
    }
    return [...(commands ?? []), ...builtIns];
  }, [clearSession, compactSession, doctorSession, commands, enabledBuiltInCommands]);
  const commandMenuItems = useMemo(
    () => resolvedCommands.map(({ name, aliases, description }) => ({ name, aliases, description })),
    [resolvedCommands],
  );
  return { commandMenuItems, resolvedCommands };
}
