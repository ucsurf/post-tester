import { testPlugin } from "@post-tester/plugin-sdk/testing";
import plugin, { base32Decode, hmacSha1, hotp, sha1 } from "./index.ts";

const hex = (bytes: number[]) => bytes.map((b) => b.toString(16).padStart(2, "0")).join("");
const ascii = (text: string) => Array.from(text, (c) => c.charCodeAt(0));

// The RFC 4226 / RFC 6238 secret "12345678901234567890" (ASCII), in base32.
const RFC_SECRET = "GEZDGNBVGY3TQOJQGEZDGNBVGY3TQOJQ";

test("SHA-1 matches the FIPS 180 examples", () => {
  expect(hex(sha1(ascii("abc")))).toBe("a9993e364706816aba3e25717850c26c9cd0d89d");
  expect(hex(sha1([]))).toBe("da39a3ee5e6b4b0d3255bfef95601890afd80709");
  expect(hex(sha1(ascii("abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq")))).toBe("84983e441c3bd26ebaae4aa1f95129e5e54670f1");
});

test("HMAC-SHA1 matches RFC 4226 appendix D (counter 0)", () => {
  const counter0 = [0, 0, 0, 0, 0, 0, 0, 0];
  expect(hex(hmacSha1(ascii("12345678901234567890"), counter0))).toBe("cc93cf18508d94934c64b65d8ba7667fb7cde4b0");
});

test("base32 decodes the RFC secret", () => {
  expect(base32Decode(RFC_SECRET)).toEqual(ascii("12345678901234567890"));
  expect(base32Decode("gezd gnbv-gy3t qojq====")).toEqual(ascii("1234567890"));
});

test("HOTP matches RFC 4226 appendix D", () => {
  const expected = ["755224", "287082", "359152", "969429", "338314", "254676", "287922", "162583", "399871", "520489"];
  const key = ascii("12345678901234567890");
  expected.forEach((code, counter) => expect(hotp(key, counter)).toBe(code));
});

test("TOTP matches RFC 6238 appendix B (SHA-1, 8 digits)", async () => {
  const t = testPlugin(plugin);
  const vectors: Array<[number, string]> = [
    [59, "94287082"],
    [1111111109, "07081804"],
    [1111111111, "14050471"],
    [1234567890, "89005924"],
    [2000000000, "69279037"],
    [20000000000, "65353130"],
  ];
  for (const [time, code] of vectors) {
    expect(await t.templateFunction("totp", { secret: RFC_SECRET, digits: 8, time })).toBe(code);
  }
});

test("positional arguments work like {{totp(\"...\")}} and default to 6 digits", async () => {
  const code = await testPlugin(plugin).templateFunction("totp", { 0: RFC_SECRET });
  expect(code).toMatch(/^\d{6}$/);
});

test("invalid secrets explain what is wrong", async () => {
  let error: Error | undefined;
  try {
    await testPlugin(plugin).templateFunction("totp", { secret: "not base32!" });
  } catch (e) {
    error = e as Error;
  }
  expect(error?.message).toContain("not valid base32");
});

test("a missing secret is reported", async () => {
  let error: Error | undefined;
  try {
    await testPlugin(plugin).templateFunction("totp", {});
  } catch (e) {
    error = e as Error;
  }
  expect(error?.message).toContain('missing argument "secret"');
});
