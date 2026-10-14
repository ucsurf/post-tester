/**
 * An auth method that signs each request with a shared secret.
 *
 * The signature is HMAC-SHA256 (hex) over this text, one item per line:
 *
 *     <unix timestamp in seconds>
 *     <METHOD>
 *     <full URL>
 *     <SHA-256 of the body, hex>
 *
 * and is sent as:
 *
 *     X-Key-Id:    <keyId>
 *     X-Timestamp: <timestamp>
 *     X-Signature: <signature>
 *
 * Hashing the body (instead of signing it directly) keeps the signed text
 * small and unambiguous, since bodies may contain newlines.
 */
import { definePlugin } from "@post-tester/plugin-sdk";
import type { PluginContext } from "@post-tester/plugin-sdk";

/** Builds the text that is signed. Exported so servers and tests can reproduce it. */
export function stringToSign(timestamp: string, method: string, url: string, body: string, ctx: PluginContext): string {
  return [timestamp, method.toUpperCase(), url, ctx.crypto.sha256(body)].join("\n");
}

export default definePlugin({
  auth: {
    hmac: {
      displayName: "HMAC-SHA256 signature",
      fields: [
        { name: "keyId", type: "string", label: "Key ID" },
        { name: "secret", type: "secret", label: "Signing secret" },
      ],
      apply: (request, config, ctx) => {
        const keyId = String(config.keyId ?? "").trim();
        const secret = String(config.secret ?? "");
        if (!keyId) throw new Error('auth config is missing "keyId"; set it in the request\'s auth config');
        if (!secret) {
          throw new Error('auth config is missing "secret"; store it with `pt secret set <name>` and use secret: "{{secret:<name>}}"');
        }
        // Seconds since the epoch, like most signature schemes; servers usually
        // reject timestamps older than a few minutes to prevent replays.
        const timestamp = String(Math.floor(Date.now() / 1000));
        const signature = ctx.crypto.hmacSha256(secret, stringToSign(timestamp, request.method, request.url, request.body, ctx));
        request.headers.set("X-Key-Id", keyId);
        request.headers.set("X-Timestamp", timestamp);
        request.headers.set("X-Signature", signature);
        return request;
      },
    },
  },
});
