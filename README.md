<div align="center">

<picture>
  <source media="(prefers-color-scheme: dark)" srcset="assets/logo-dark.svg">
  <img src="assets/logo-light.svg" alt="post-tester" width="520">
</picture>

<p>
  <strong>An open, local-first API client for building, sending, testing and automating API requests, with everything visible live.</strong>
</p>

<p>
  <img src="https://img.shields.io/badge/status-early%20development-orange" alt="Status: early development">
  <img src="https://img.shields.io/badge/platforms-Windows%20%7C%20macOS%20%7C%20Linux%20%7C%20Web%20%7C%20Mobile-6366f1" alt="Platforms">
  <img src="https://img.shields.io/badge/plugins-TypeScript%20%2F%20JavaScript-14b8a6" alt="Plugins: TypeScript / JavaScript">
  <img src="https://img.shields.io/badge/MCP-server%20%2B%20client-0f172a" alt="MCP: server + client">
</p>

<p>
  <a href="#-features">Features</a> ·
  <a href="#-live-activity">Live Activity</a> ·
  <a href="#-plugins">Plugins</a> ·
  <a href="#-ai--mcp">AI &amp; MCP</a> ·
  <a href="guide/README.md">Guide</a>
</p>

</div>

---

> [!WARNING]
> **post-tester is in early development and not ready for use yet.**
> Everything below describes the planned design, which may change before the first release.

## ✨ Features

<table>
  <tr>
    <td width="50%" valign="top">
      <h3>🏠 Local-first</h3>
      Your work lives in plain files on your machine. No account, no forced cloud.
    </td>
    <td width="50%" valign="top">
      <h3>🌿 Git-friendly</h3>
      One request per file, in readable YAML. Review changes in pull requests like code.
    </td>
  </tr>
  <tr>
    <td valign="top">
      <h3>📡 Live by default</h3>
      See everything as it happens, from a one-line summary down to the raw bytes.
    </td>
    <td valign="top">
      <h3>🔌 Plugins for everyone</h3>
      Add auth methods, importers, protocols and viewers in TypeScript, sandboxed and permission-based.
    </td>
  </tr>
  <tr>
    <td valign="top">
      <h3>🤖 AI-ready</h3>
      AI agents can use post-tester through MCP, and post-tester can test MCP servers.
    </td>
    <td valign="top">
      <h3>🖥️ Runs everywhere</h3>
      Desktop, browser, team server, mobile and CI, all powered by one engine.
    </td>
  </tr>
</table>

### Protocols

| Protocol | | Protocol | |
|---|:-:|---|:-:|
| HTTP / REST | ✅ planned | gRPC | ✅ planned |
| GraphQL | ✅ planned | MQTT | ✅ planned |
| WebSocket | ✅ planned | MCP | ✅ planned |
| Server-Sent Events | ✅ planned | Your own | 🔌 via plugin |

### Testing & automation

- **Scripts and tests** that run before requests and after responses
- **Collection runner** for the app, the terminal and CI pipelines
- **Flows** that chain requests together, e.g. log in → get a token → call the API
- **Mock server** built from your collections
- **Load testing** from any request or flow
- **Response diff** between runs or environments

## 📡 Live Activity

Every action appears the moment it happens, whether you, an AI agent, the CLI, a plugin or a schedule started it.

```text
LIVE ACTIVITY                                       [⏸ Pause]  [Filter ▾]  [Follow ●]
──────────────────────────────────────────────────────────────────────────────────────
👤 You         POST /auth/login           200  ▓▓▓░░░░░  182 ms   1.2 KB        ▸
🤖 AI agent    GET  /users?page=2         200  ▓▓░░░░░░   96 ms   8.4 KB        ▸
▶  Run "Checkout flow"                    3/7  ●●●○○○○   running…               ▾
      ✓ Create cart            201    88 ms
      ✓ Add item               200    54 ms
      ⟳ Pay                    …      waiting for response (1.4 s)
🔌 my-plugin   created ticket #42                                                ▸
```

Click any row to see more, still updating live:

| Level | You see |
|---|---|
| **Summary** | Who started it, name, status, duration, size |
| **Details** | Timing breakdown (DNS · TCP · TLS · waiting · download), headers, body, logs, test results |
| **Raw** | The exact data sent and received |

## 🔌 Plugins

A plugin is a folder with a `plugin.json` and a TypeScript file:

```ts
import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  templateFunctions: {
    totp: {
      description: "Generate a 6-digit TOTP code from a base32 secret.",
      args: [{ name: "secret", type: "string" }],
      run: ({ secret }) => generateTotp(secret),
    },
  },
});
```

Then use it in any request: `{{totp(secret:"authenticator")}}`

- **Sandboxed.** Plugins can't touch your system unless you allow it.
- **Permission-based.** Plugins declare what they need, and you approve it on install.
- **AI-agent friendly.** The guide, typed SDK and validator are designed so an AI agent can write a working plugin on the first try.

👉 **[Read the plugin guide](guide/plugins/README.md)**

## 🤖 AI & MCP

post-tester supports the [Model Context Protocol](https://modelcontextprotocol.io) in **both directions**:

<table>
  <tr>
    <td width="50%" valign="top">
      <h4>As an MCP server</h4>
      AI agents can browse your collections, send requests and run tests, locally on your machine.
      <br><br>
      Every action an agent takes appears in <strong>Live Activity</strong>, and secrets are never shown to agents.
    </td>
    <td width="50%" valign="top">
      <h4>As an MCP client</h4>
      Connect to any MCP server, explore its tools, resources and prompts, call them, and write tests for them.
      <br><br>
      Every message is shown live.
    </td>
  </tr>
</table>

## 📚 Documentation

| Guide | Description |
|---|---|
| [Overview](guide/README.md) | Core ideas, concepts and workspace layout |
| [Building plugins](guide/plugins/README.md) | Everything needed to create a plugin, for people and AI agents |

## 🤝 Contributing

post-tester is at a very early stage. Ideas, feedback and issues are welcome.
Contribution guidelines will be added as the project takes shape.
