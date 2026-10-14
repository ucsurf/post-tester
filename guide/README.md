# post-tester Guide

> **Status: early development.** Features and APIs described here may change
> before the first stable release.

post-tester is an open, local-first API client for building, sending, testing
and automating API requests across many protocols.

## Core ideas

- **Local-first.** Your work lives in plain files on your machine. No account required.
- **Git-friendly.** One request per file, in readable YAML, so changes are easy to review.
- **Live by default.** Everything that happens is visible in real time, from a
  one-line summary down to the raw bytes.
- **Extensible.** Plugins can add auth methods, importers, protocols, viewers and more.
- **AI-ready.** AI agents can use post-tester through MCP, and post-tester can
  connect to and test MCP servers.

## Concepts

| Concept | What it is |
|---|---|
| **Workspace** | A folder containing your collections, environments and settings |
| **Collection** | A folder of saved requests, which can be nested |
| **Request** | One API call (HTTP, GraphQL, gRPC, WebSocket, SSE, MCP, …) |
| **Environment** | A named set of variables, such as `staging` or `production` |
| **Secret** | A sensitive value kept in your system keychain, referenced as `{{secret:name}}` |
| **Live Activity** | A real-time view of everything happening: requests, test runs, plugins and AI agents |
| **Plugin** | A small package that adds new capabilities |

## Workspace layout

```
my-workspace/
├── post-tester.yaml        # workspace settings and installed plugins
├── collections/
│   └── auth/
│       ├── _folder.yaml    # settings shared by the folder (auth, headers)
│       └── login.yaml      # one request per file
├── environments/
│   └── staging.yaml        # variables; secrets are referenced, never stored
└── .post-tester/           # local data (history, cache); do not commit
```

## Live Activity

Every action appears in the Live Activity panel as it happens, with:

- **Who started it:** you, an AI agent, the CLI, a plugin or a schedule
- **A clean summary:** name, status, duration and size
- **Details on click:** timing breakdown, headers, body, logs and test results
- **Raw view:** the exact data sent and received

## Guides

- [Plugins](plugins/README.md): how to build a plugin (for people and AI agents)
