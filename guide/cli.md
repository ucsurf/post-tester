# Command line: `pt`

> **Status: early development.** Commands and options may change before 1.0.

`pt` runs everything post-tester can do from a terminal: sending requests,
running collections in CI, load tests, the app server and the MCP server.
Every command shows what happens **live**, as it happens.

```text
$ pt send examples/get-ip
→ Get my IP
  GET https://httpbin.org/ip
  DNS 3 ms · Connect 152 ms · TLS 308 ms · Waiting 149 ms · Download 0 ms
← 200 OK  HTTP/2  31 B
  ✓ status == 200
  ✓ json($.origin) exists
  616 ms
```

## Workspaces

| Command | What it does |
|---|---|
| `pt init [dir] [--name N]` | Create a workspace with a sample request and a `local` environment |
| `pt env list` | List environments (the default is marked) |
| `pt env show <name>` | Show an environment's variables |
| `pt import <file\|-> [--into folder]` | Import a cURL command, Postman collection, OpenAPI 3 document, or a format provided by a plugin |
| `pt export [folder] --to <plugin>/<exporter> [-o file]` | Export requests with an exporter plugin (e.g. Markdown documentation) |

Commands find the workspace in the current folder or its parents. Use
`--workspace <dir>` (or `PT_WORKSPACE`) to point somewhere else.

## Sending requests

| Command | What it does |
|---|---|
| `pt send <id\|url>` | Send a saved request (e.g. `auth/login`) or a URL |
| `pt load <id\|url> -c 10 -n 500` | Send a request 500 times, 10 at a time, and report latency percentiles. Connections are kept open and reused; add `--fresh-connections` to open a new one (DNS, TCP, TLS) for every request |
| `pt codegen <id\|url> -t python-requests` | Print code that sends the request (`curl`, `javascript-fetch`, `python-requests`, or a plugin's `plugin/name`) |

A URL may leave out the scheme (`pt send localhost:3000/users` uses `http://`)
or start with a variable (`pt send '{{baseUrl}}/users'`).
Options for a URL: `-X METHOD`, `-H 'Name: value'` (repeatable), `-d BODY`
(JSON bodies get `Content-Type: application/json`). Common options:
`-e <environment>`, `-v` (show headers), `--json` (machine-readable result),
`--no-body`.

## Running collections and CI

```bash
pt run                         # every request in the workspace
pt run auth                    # every request in collections/auth/
pt run -r auth/login -r users/me
pt run --bail --iterations 3 --delay 200
pt run --reporter junit --output results.xml
pt run --reporter json > results.json
```

Requests run in file-name order (prefix files with `01-`, `02-` to control it).
Variables extracted by one request (`extract:`) are available to the next.

**Exit codes** (for every command): `0` everything passed · `1` a test failed or
a request failed · `2` a usage or setup error. With `--json` or
`--reporter json`, stdout holds only the JSON document; warnings go to stderr.
JUnit reports list one test case per request, with the failed tests in `<failure>`.

**Secrets in CI:** set `PT_SECRET_<NAME>` environment variables, e.g.
`PT_SECRET_API_KEY` for `{{secret:api-key}}`. They take precedence over the keychain.

## Mock API

Add example responses to a request, then serve them:

```yaml
name: Get user
url: "{{baseUrl}}/users/{{id}}"
examples:
  - name: found
    body: '{"id": "{{id}}", "name": "Ada"}'
  - name: missing
    status: 404
    body: '{"error": "not found"}'
    delay_ms: 300
```

```bash
pt mock                       # every request with examples, on http://127.0.0.1:4010
pt mock users --port 5000     # only the users folder
curl localhost:4010/users/42                              # → the first example
curl -H 'X-Mock-Example: missing' localhost:4010/users/42 # → a specific example
```

Path segments written as `{{name}}` or `:name` match anything and can be used
in the example body. In the app, **Save as example** on a response adds it to
the request. Every mock hit appears in Live Activity.

- Query strings are ignored when matching. When several paths match, the more
  specific one wins: `/users/me` before `/users/{{id}}`.
- `HEAD` requests are answered by `GET` examples, without a body.
- Responses allow cross-origin requests (CORS), and `OPTIONS` preflight
  requests are answered, so a front-end running in a browser can use the mock.
- An unknown `X-Mock-Example` name returns `404` with the list of examples.
- Example bodies may use variables and values such as `{{$uuid}}`, but never
  secrets. Request bodies are limited to 16 MB.
- Examples are read when `pt mock` starts; restart it after editing them.

## Secrets

```bash
pt secret set api-key            # reads the value from stdin
pt secret delete api-key
```

Secrets are stored in the OS keychain and referenced in requests as
`{{secret:api-key}}`. They are never written to workspace files, and are
shown as `•••` in previews and generated code.

## The app and AI agents

| Command | What it does |
|---|---|
| `pt serve [--port 7777] [--host 127.0.0.1] [--no-open] [--read-only]` | Start the local server and open the app in your browser |
| `pt mock [folder] [--port 4010]` | Serve saved example responses as a mock API |
| `pt mcp [--read-only]` | Serve MCP over stdio for AI agents (see [mcp.md](mcp.md)) |

While the app is open, everything started from a terminal (`pt send`, `pt run`,
`pt mock`) or by an AI agent over `pt mcp` in the same workspace also appears
in its Live Activity, as it happens.

`pt serve` prints a link that contains a session token. The server listens on
`127.0.0.1` and rejects requests without the token or from other websites.

To use the app from another device, listen on your network with
`--host 0.0.0.0` (or one of this computer's addresses) and open
`http://<this computer's IP address>:7777/?token=…` there. Anyone on the
network who has the token can then use post-tester, including sending
requests from your computer. Only IP addresses and `localhost` are accepted in
the address; other host names are rejected to block DNS rebinding.

`pt serve` and `pt mock` stop on Ctrl+C: they stop accepting connections and
give requests in flight up to 5 seconds to finish. Press Ctrl+C again to stop
at once.

## Plugins

| Command | What it does |
|---|---|
| `pt plugin new <type> <name>` | Create a plugin from a working template |
| `pt plugin validate [dir]` | List every problem with a fix |
| `pt plugin test [dir]` | Run the plugin's `*.test.ts` files |
| `pt plugin install <dir> [--yes]` | Add it to the workspace and review its permissions |
| `pt plugin approve <name>` | Approve the permissions a plugin asks for |
| `pt plugin list` / `pt plugin remove <name>` | Manage installed plugins |

See the [plugin guide](plugins/README.md).

## Request files

A request is a YAML file in `collections/`. Only `url` is required.

```yaml
name: Create user
method: POST
url: "{{baseUrl}}/users"
headers:
  - { key: X-Trace, value: "{{$uuid}}" }
body:
  type: json            # none, text, json, form, graphql
  content: '{"name": "Ada"}'
auth:
  type: bearer          # none, basic, bearer, api_key, plugin
  token: "{{secret:api-token}}"
tests:
  - status == 201
  - json($.id) exists
  - header(content-type) contains json
  - time < 500
extract:
  userId: json($.id)
scripts:
  after_response: |
    pm.test("has a name", () => pm.expect(pm.response.json().name).to.equal("Ada"));
options:
  timeout_ms: 30000
  follow_redirects: true
  verify_tls: true
```

**Test subjects:** `status`, `time`, `size`, `body`, `header(name)`,
`json($.path)`. **Operators:** `==` `!=` `<` `<=` `>` `>=` `contains`
`exists` `matches`, and the negations `!contains` `!exists` `!matches`.

**Template values:** `{{variable}}`, `{{secret:name}}`, `{{$uuid}}`,
`{{$timestamp}}`, `{{$timestampMs}}`, `{{$isoTimestamp}}`, `{{$randomInt}}`,
and functions from plugins such as `{{totp(secret: "...")}}`.

Folders can hold a `_folder.yaml` with `headers`, `auth` and `variables`
shared by every request inside them.
