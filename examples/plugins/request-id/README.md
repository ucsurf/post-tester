# request-id

A **hook** plugin:

- **beforeRequest** adds an `X-Request-Id: <uuid>` header to every request, so
  you can find the request in your server's logs. A request that already sets
  `X-Request-Id` keeps its own value.
- **afterResponse** writes a warning to Live Activity when a response takes
  longer than 1000 ms. Set the variable `slowResponseMs` (in an environment or
  the workspace) to change the threshold.

## Usage

Install it; it runs for every request:

```bash
pt plugin install ./examples/plugins/request-id
```

No permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
```
