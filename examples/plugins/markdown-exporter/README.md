# markdown-exporter

An **exporter** plugin that turns a collection into Markdown API
documentation, e.g. for a wiki or a repository's `docs/` folder.

Each request gets a section with its method and URL, description, auth type,
query parameters, headers, body and tests. Variables such as `{{baseUrl}}`
are kept as written, and auth is described by type only, so secrets never end
up in the document.

No permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
```
