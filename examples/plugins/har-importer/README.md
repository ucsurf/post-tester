# har-importer

An **importer** plugin for HAR 1.2 files (HTTP Archive). Browser developer
tools ("Save all as HAR") and most debugging proxies can export them.

For each entry in `log.entries`:

| HAR | post-tester |
|---|---|
| `request.method`, `request.url` | `method`, `url` (without the query string) |
| Query string in the URL | `query` parameters |
| `request.headers` | `headers`, without HTTP/2 pseudo-headers, `Host`, `Content-Length`, `Connection` and `Cookie` |
| `postData` (JSON) | `body: { type: json }` |
| `postData` (form with `params`) | `body: { type: form }` |
| `postData` (anything else) | `body: { type: text }` with its content type |

Requests are grouped into one folder per host, and named `METHOD /path`.
Detection checks for a JSON document with a `log.entries` array. No
permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
```
