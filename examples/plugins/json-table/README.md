# json-table

A **viewer** plugin that shows JSON responses as HTML tables:

| Response | Shown as |
|---|---|
| Array of objects | One row per item, one column per key |
| Object | Key/value rows |
| Array of values | One value per row |

Nested objects and arrays are shown as compact JSON. Every key and value is
HTML-escaped, so a malicious response cannot inject markup or scripts.

No permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
```
