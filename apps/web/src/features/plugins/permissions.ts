/** Plain-language explanations of plugin permissions (see crates/plugin-host manifest.rs). */
export function describePermission(permission: string): { text: string; risk: "low" | "medium" | "high" } {
  if (permission === "network:*") return { text: "Send HTTP requests to any website", risk: "high" };
  if (permission.startsWith("network:")) return { text: `Send HTTP requests to ${permission.slice("network:".length)}`, risk: "medium" };
  switch (permission) {
    case "secrets:read":
      return { text: "Read your secrets from the keychain", risk: "high" };
    case "fs:read":
      return { text: "Read files in this workspace", risk: "medium" };
    case "fs:write":
      return { text: "Create and change files in this workspace", risk: "high" };
    default:
      return { text: `Unknown permission “${permission}”`, risk: "high" };
  }
}
