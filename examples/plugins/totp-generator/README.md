# totp-generator

A **template function** plugin that generates time-based one-time passwords
(TOTP, RFC 6238): the same 6-digit codes an authenticator app shows. Use it to
call APIs that require a 2FA code.

## Usage

```yaml
method: POST
url: "{{baseUrl}}/login"
headers:
  - { key: X-OTP, value: '{{totp("JBSWY3DPEHPK3PXP")}}' }
body:
  type: json
  content: '{"user": "ada", "code": "{{totp("JBSWY3DPEHPK3PXP")}}"}'
```

Quotes inside `{{ }}` belong to the template, so the JSON body above stays
valid: it becomes `{"user": "ada", "code": "492039"}`.

| Argument | Default | Description |
|---|---|---|
| `secret` | (required) | The base32 secret shown when 2FA was set up |
| `digits` | `6` | Code length (6 to 10) |
| `period` | `30` | Seconds each code is valid |
| `time` | now | Unix time in seconds (useful for tests) |

Named arguments work too: `{{totp(secret: "JBSWY3DPEHPK3PXP", digits: 8)}}`.

> Template function arguments are literal values, so the secret is written in
> the request. Use a test account's secret, not a personal one.

## How it works

`ctx.crypto` offers HMAC-SHA256 only, so the plugin implements SHA-1 and
HMAC-SHA1 itself (`index.ts`). The tests check them against the official
vectors from FIPS 180, RFC 4226 and RFC 6238. No permissions are needed.

## Development

```bash
pt plugin validate
pt plugin test
pt plugin install .
```
