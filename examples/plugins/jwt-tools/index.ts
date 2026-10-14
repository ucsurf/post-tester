/**
 * An MCP tool for AI agents: decode a JWT and explain its time claims.
 *
 * When an API answers 401, the token is often expired, not yet valid, or for
 * the wrong audience. This tool shows the header and payload and computes the
 * expiry, so an agent can tell which. It does NOT verify the signature
 * (that needs the issuer's key), and says so in its output.
 *
 * Agents see it as `jwt_tools__decode_jwt`.
 */
import { definePlugin } from "@post-tester/plugin-sdk";
import type { PluginContext } from "@post-tester/plugin-sdk";

/** Decodes one base64url JWT segment into text. */
const decodeSegment = (segment: string, ctx: PluginContext): string => {
  // base64url → standard base64 with padding, which ctx.crypto decodes as UTF-8.
  const base64 = segment.replace(/-/g, "+").replace(/_/g, "/");
  return ctx.crypto.base64Decode(base64 + "=".repeat((4 - (base64.length % 4)) % 4));
};

const parseSegment = (segment: string, part: string, ctx: PluginContext): Record<string, unknown> => {
  if (!/^[A-Za-z0-9_-]*$/.test(segment)) throw new Error(`the JWT ${part} is not base64url`);
  let value: unknown;
  try {
    value = JSON.parse(decodeSegment(segment, ctx));
  } catch {
    throw new Error(`the JWT ${part} is not valid JSON`);
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) throw new Error(`the JWT ${part} is not a JSON object`);
  return value as Record<string, unknown>;
};

/** A NumericDate claim (seconds since the epoch) as an ISO date, if present and valid. */
const isoDate = (seconds: unknown): string | null =>
  typeof seconds === "number" && Number.isFinite(seconds) ? new Date(seconds * 1000).toISOString() : null;

export function decodeJwt(token: string, nowSeconds: number, ctx: PluginContext) {
  const trimmed = token.trim().replace(/^Bearer\s+/i, "");
  const parts = trimmed.split(".");
  if (parts.length !== 3) {
    throw new Error(`a JWT has 3 parts separated by "." (header.payload.signature), this has ${parts.length}. JWE (encrypted, 5 parts) tokens cannot be decoded without the key`);
  }
  const header = parseSegment(parts[0], "header", ctx);
  const payload = parseSegment(parts[1], "payload", ctx);
  const { exp, nbf, iat } = payload;

  const warnings: string[] = [];
  if (String(header.alg).toLowerCase() === "none") warnings.push('alg is "none": the token is unsigned and should be rejected by servers');
  if (parts[2] === "" && String(header.alg).toLowerCase() !== "none") warnings.push("the signature is empty");
  if (exp === undefined) warnings.push("no exp claim: the token never expires");
  else if (typeof exp !== "number") warnings.push("exp is not a number of seconds");
  const expired = typeof exp === "number" && nowSeconds >= exp;
  if (expired) warnings.push(`expired ${Math.round(nowSeconds - (exp as number))} s ago`);
  const notYetValid = typeof nbf === "number" && nowSeconds < nbf;
  if (notYetValid) warnings.push(`not valid for another ${Math.round((nbf as number) - nowSeconds)} s (nbf)`);

  return {
    header,
    payload,
    signatureVerified: false,
    issuedAt: isoDate(iat),
    notBefore: isoDate(nbf),
    expiresAt: isoDate(exp),
    expired,
    notYetValid,
    expiresInSeconds: typeof exp === "number" ? Math.round(exp - nowSeconds) : null,
    warnings,
  };
}

export default definePlugin({
  mcpTools: {
    decode_jwt: {
      description:
        "Decodes a JSON Web Token (JWT) without verifying its signature. Returns the header, the payload (claims such as sub, aud, iss, scope), " +
        "the issued/not-before/expiry times as ISO dates, whether it is expired, and warnings. Use it to debug 401/403 responses.",
      inputSchema: {
        type: "object",
        properties: {
          token: { type: "string", description: 'The JWT, with or without a "Bearer " prefix' },
          now: { type: "number", description: "Unix time in seconds to check expiry against; defaults to the current time" },
        },
        required: ["token"],
      },
      run: ({ token, now }, ctx) => {
        if (typeof token !== "string" || !token.trim()) throw new Error('"token" is required: pass the JWT as a string');
        return decodeJwt(token, typeof now === "number" ? now : Date.now() / 1000, ctx);
      },
    },
  },
});
