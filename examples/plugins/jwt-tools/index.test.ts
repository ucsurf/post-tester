import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin from "./index.ts";

/** Encodes a JSON object as a base64url JWT segment. */
const segment = (value: unknown) => ctx.crypto.base64Encode(JSON.stringify(value)).replace(/\+/g, "-").replace(/\//g, "_").replace(/=+$/, "");
const jwt = (payload: unknown, header: unknown = { alg: "HS256", typ: "JWT" }, signature = "c2lnbmF0dXJl") =>
  `${segment(header)}.${segment(payload)}.${signature}`;

// A widely used example token (HS256, with iat but no exp).
const EXAMPLE =
  "eyJhbGciOiJIUzI1NiIsInR5cCI6IkpXVCJ9.eyJzdWIiOiIxMjM0NTY3ODkwIiwibmFtZSI6IkpvaG4gRG9lIiwiaWF0IjoxNTE2MjM5MDIyfQ.SflKxwRJSMeKKF2QT4fwpMeJf36POk6yJV_adQssw5c";

type Decoded = {
  header: Record<string, unknown>;
  payload: Record<string, unknown>;
  signatureVerified: boolean;
  issuedAt: string | null;
  expiresAt: string | null;
  expired: boolean;
  notYetValid: boolean;
  expiresInSeconds: number | null;
  warnings: string[];
};
const decode = async (input: Record<string, unknown>) => (await testPlugin(plugin).mcpTool("decode_jwt", input)) as Decoded;

test("decodes header and payload, never claims to verify", async () => {
  const result = await decode({ token: EXAMPLE });
  expect(result.header).toEqual({ alg: "HS256", typ: "JWT" });
  expect(result.payload).toEqual({ sub: "1234567890", name: "John Doe", iat: 1516239022 });
  expect(result.issuedAt).toBe("2018-01-18T01:30:22.000Z");
  expect(result.signatureVerified).toBe(false);
  expect(result.warnings).toEqual(["no exp claim: the token never expires"]);
});

test("reports expiry relative to now", async () => {
  const token = jwt({ sub: "ada", exp: 1_700_000_000 });
  const valid = await decode({ token, now: 1_699_999_900 });
  expect(valid.expired).toBe(false);
  expect(valid.expiresInSeconds).toBe(100);
  expect(valid.expiresAt).toBe("2023-11-14T22:13:20.000Z");

  const expired = await decode({ token: `Bearer ${token}`, now: 1_700_000_060 });
  expect(expired.expired).toBe(true);
  expect(expired.warnings).toContain("expired 60 s ago");
});

test("reports tokens that are not valid yet and unsigned tokens", async () => {
  const result = await decode({ token: jwt({ nbf: 2000, exp: 3000 }, { alg: "none" }, ""), now: 1000 });
  expect(result.notYetValid).toBe(true);
  expect(result.warnings).toContain("not valid for another 1000 s (nbf)");
  expect(result.warnings).toContain('alg is "none": the token is unsigned and should be rejected by servers');
});

test("decodes UTF-8 claims", async () => {
  const result = await decode({ token: jwt({ name: "Zoë 🚀", exp: 10 }), now: 0 });
  expect(result.payload.name).toBe("Zoë 🚀");
});

test("explains malformed tokens", async () => {
  const failure = async (input: Record<string, unknown>) => {
    try {
      await decode(input);
    } catch (e) {
      return (e as Error).message;
    }
    return "no error";
  };
  expect(await failure({})).toContain('"token" is required');
  expect(await failure({ token: "abc.def" })).toContain("has 2");
  expect(await failure({ token: "a+b.c.d" })).toContain("header is not base64url");
  expect(await failure({ token: `${segment({ alg: "HS256" })}.bm90IGpzb24.sig` })).toContain("payload is not valid JSON");
});

test("describes the tool for agents", async () => {
  const [tool] = (await testPlugin(plugin).describe()).mcpTools;
  expect(tool.name).toBe("decode_jwt");
  expect(tool.description).toContain("without verifying");
  expect(tool.inputSchema.required).toEqual(["token"]);
});
