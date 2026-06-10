export type TalonBuiltInCommandName = "clear";

export type TalonChatCommandContext<TTarget, TMessage> = {
  name: string;
  input: string;
  args: string;
  argv: string[];
  target: TTarget;
  messages: TMessage[];
  clear?: () => void | Promise<void>;
};

export type TalonChatCommand<TTarget = unknown, TMessage = unknown> = {
  name: string;
  aliases?: string[];
  description?: string;
  run: (context: TalonChatCommandContext<TTarget, TMessage>) => void | Promise<void>;
};

export type ParsedTalonChatCommand = {
  name: string;
  args: string;
  argv: string[];
};

export function normalizeCommandName(name: string) {
  return name.trim().replace(/^\/+/, "").toLowerCase();
}

export function parseTalonChatCommandInput(input: string): ParsedTalonChatCommand | null {
  const trimmed = input.trim();
  if (!trimmed.startsWith("/") || trimmed === "/") return null;
  const withoutPrefix = trimmed.slice(1);
  const firstWhitespaceIndex = withoutPrefix.search(/\s/);
  const rawName = firstWhitespaceIndex === -1 ? withoutPrefix : withoutPrefix.slice(0, firstWhitespaceIndex);
  const name = normalizeCommandName(rawName);
  if (!name) return null;
  const args = firstWhitespaceIndex === -1 ? "" : withoutPrefix.slice(firstWhitespaceIndex).trim();
  return {
    name,
    args,
    argv: args ? args.split(/\s+/) : [],
  };
}

export function findTalonChatCommand<TTarget, TMessage>(
  commands: Array<TalonChatCommand<TTarget, TMessage>> | undefined,
  parsed: ParsedTalonChatCommand | null,
) {
  if (!parsed || !commands?.length) return null;
  return commands.find((command) => {
    if (normalizeCommandName(command.name) === parsed.name) return true;
    return command.aliases?.some((alias) => normalizeCommandName(alias) === parsed.name) ?? false;
  }) ?? null;
}
