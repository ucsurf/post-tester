# AI agents and MCP

> **Status: early development.** Tools and options may change before 1.0.

post-tester supports the [Model Context Protocol](https://modelcontextprotocol.io)
in both directions:

- **As an MCP server**, AI agents can use your workspace: read requests, send
  them, run collections and call plugin tools.
- **As an MCP client**, you can connect to any MCP server and test it like an API.

Everything happens on your machine, and every action appears in **Live
Activity** as it happens, marked with 🤖 and the agent's name.

## Connect an agent

**Over stdio** (recommended; the agent starts post-tester itself):

```bash
claude mcp add post-tester -- pt mcp
```

For other agents, configure a stdio server with the command `pt` and the
argument `mcp`. If the app is open on the same workspace, the agent's work
appears in it live. Add `--workspace <dir>` if the agent does not start in your
workspace folder, and `--read-only` to stop the agent from changing files.

**Over HTTP**, while the app is running (`pt serve` or the desktop app):

- URL: `http://127.0.0.1:7777/mcp`
- Header: `Authorization: Bearer <token>`, using the token from the link `pt serve` prints

If the agent runs on another device and the app listens on your network
(`pt serve --host 0.0.0.0`), use `http://<this computer's IP address>:7777/mcp`.

## Tools for agents

| Tool | What it does |
|---|---|
| `get_workspace` | Workspace name, environments and the collection tree |
| `get_request` | One saved request |
| `send_request` | Send a saved request (`id`) or an inline request; returns status, timing, headers, body, test results and extracted variables |
| `run_collection` | Run a folder and return a summary per request |
| `save_request` | Create or update a request (not available with `--read-only`) |
| `list_environments` | Environment names and variable names (values only on request) |
| `get_recent_activity` | Recent requests and runs, from any source |
| `list_plugins` | Installed plugins and what they provide |
| `<plugin>__<tool>` | Tools provided by plugins |

Saved requests are also available as resources at
`post-tester://requests/{id}`.

Agents never see secret values. Requests reference secrets as
`{{secret:name}}`, and post-tester fills them in only when sending.

## Test an MCP server

In the app, open **MCP** and connect:

- **stdio:** a command and arguments, e.g. `npx -y @modelcontextprotocol/server-everything`
- **HTTP:** a URL and optional headers

You can then browse the server's tools, resources and prompts, call tools
with JSON arguments, and see every JSON-RPC message live. The same is
available over the local API (`/api/mcp/*`); see `crates/server/API.md`.
