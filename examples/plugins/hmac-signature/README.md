# hmac-signature

An **auth** plugin that signs every request with a shared secret using
HMAC-SHA256, a common scheme for webhook and partner APIs.

It adds three headers:

| Header | Value |
|---|---|
| `X-Key-Id` | The configured key id |
| `X-Timestamp` | Unix time in seconds |
| `X-Signature` | Hex HMAC-SHA256 of `timestamp`, `METHOD`, `URL` and `sha256(body)`, joined with `\n` |

## Usage

Store the secret once:

```bash
pt secret set partner-api-secret
```

Then select the auth method in a request (or a folder's `_folder.yaml`):

```yaml
auth:
  type: plugin
  name: hmac-signature/hmac
  config:
    keyId: my-key
    secret: "{{secret:partner-api-secret}}"
```

Templates in `config` are expanded before the plugin runs, so the secret never
appears in the request file. No permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
```
