import type { Source } from "@post-tester/bindings";

export type SourceType = Source["type"];

export type SourceInfo = { type: SourceType; icon: string; label: string; detail: string | null };

const ICONS: Record<SourceType, string> = {
  user: "👤",
  agent: "🤖",
  cli: "⌨️",
  plugin: "🔌",
  schedule: "⏰",
  system: "⚙️",
};

export const SOURCE_TYPES: readonly { type: SourceType; icon: string; label: string }[] = [
  { type: "user", icon: ICONS.user, label: "You" },
  { type: "agent", icon: ICONS.agent, label: "AI agent" },
  { type: "cli", icon: ICONS.cli, label: "CLI" },
  { type: "plugin", icon: ICONS.plugin, label: "Plugin" },
  { type: "schedule", icon: ICONS.schedule, label: "Schedule" },
  { type: "system", icon: ICONS.system, label: "System" },
];

/** Who started an operation, as shown on every activity row. */
export function sourceInfo(source: Source): SourceInfo {
  switch (source.type) {
    case "user":
      return { type: "user", icon: ICONS.user, label: "You", detail: null };
    case "agent":
      return { type: "agent", icon: ICONS.agent, label: "AI agent", detail: source.client };
    case "cli":
      return { type: "cli", icon: ICONS.cli, label: "CLI", detail: null };
    case "plugin":
      return { type: "plugin", icon: ICONS.plugin, label: "Plugin", detail: source.name };
    case "schedule":
      return { type: "schedule", icon: ICONS.schedule, label: "Schedule", detail: null };
    case "system":
      return { type: "system", icon: ICONS.system, label: "System", detail: null };
  }
}

/** `AI agent (claude-code)`, `Plugin my-plugin`, `You`. */
export function sourceLabel(source: Source): string {
  const info = sourceInfo(source);
  if (!info.detail) return info.label;
  return info.type === "agent" ? `${info.label} (${info.detail})` : `${info.label} ${info.detail}`;
}
