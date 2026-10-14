import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin, { stringToSign } from "./index.ts";

const config = { keyId: "key-1", secret: "s3cret" };

test("ctx.crypto.hmacSha256 matches the well-known test vector", () => {
  expect(ctx.crypto.hmacSha256("key", "The quick brown fox jumps over the lazy dog")).toBe(
    "f7bc83f430538424b13298e6aa6fb143ef4d59a14946175997479dbc2d1a3cd8",
  );
});

test("adds key id, timestamp and a verifiable signature", async () => {
  const request = await testPlugin(plugin).auth(
    "hmac",
    { method: "post", url: "https://api.example.com/orders?id=7", body: '{"qty":2}' },
    config,
  );
  const headers = Object.fromEntries(request.headers);
  expect(headers["X-Key-Id"]).toBe("key-1");
  expect(headers["X-Timestamp"]).toMatch(/^\d{10}$/);

  // Verify like a server would: rebuild the text and compare signatures.
  const expected = ctx.crypto.hmacSha256("s3cret", stringToSign(headers["X-Timestamp"], "POST", request.url, request.body, ctx));
  expect(headers["X-Signature"]).toBe(expected);
  expect(headers["X-Signature"]).toMatch(/^[0-9a-f]{64}$/);
});

test("the signed text covers method, URL, timestamp and body", () => {
  const text = stringToSign("1700000000", "get", "https://a.dev/x", "", ctx);
  expect(text).toBe(
    "1700000000\nGET\nhttps://a.dev/x\ne3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
  );
});

test("a different body gives a different signature", async () => {
  const t = testPlugin(plugin);
  const sign = async (body: string) => Object.fromEntries((await t.auth("hmac", { method: "POST", body }, config)).headers)["X-Signature"];
  expect(await sign("a")).not.toBe(await sign("b"));
});

test("missing config explains how to fix it", async () => {
  let error: Error | undefined;
  try {
    await testPlugin(plugin).auth("hmac", {}, { keyId: "key-1" });
  } catch (e) {
    error = e as Error;
  }
  expect(error?.message).toContain('missing "secret"');
});
