# jwt-tools

An **MCP tool** plugin that lets AI agents decode JSON Web Tokens while
debugging authentication, e.g. to tell whether a `401` comes from an expired
token.

Agents connected to post-tester's MCP server see it as
**`jwt_tools__decode_jwt`**.

## Input

| Field | Required | Description |
|---|---|---|
| `token` | yes | The JWT, with or without `Bearer ` |
| `now` | no | Unix time in seconds to compare against (default: now) |

## Output

```json
{
  "header": { "alg": "HS256", "typ": "JWT" },
  "payload": { "sub": "ada", "exp": 1700000000 },
  "signatureVerified": false,
  "issuedAt": null,
  "notBefore": null,
  "expiresAt": "2023-11-14T22:13:20.000Z",
  "expired": true,
  "notYetValid": false,
  "expiresInSeconds": -60,
  "warnings": ["expired 60 s ago"]
}
```

The signature is **not** verified (that needs the issuer's key);
`signatureVerified` is always `false` so agents do not assume otherwise. No
permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
```
