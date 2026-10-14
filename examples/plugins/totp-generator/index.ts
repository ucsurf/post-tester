/**
 * TOTP (RFC 6238) as a template function: {{totp("JBSWY3DPEHPK3PXP")}}.
 *
 * TOTP needs HMAC-SHA1, but `ctx.crypto` only offers HMAC-SHA256 on text, so
 * SHA-1 and HMAC are implemented here on bytes. This keeps the plugin free of
 * permissions and dependencies.
 */
import { definePlugin } from "@post-tester/plugin-sdk";

export default definePlugin({
  templateFunctions: {
    totp: {
      description: "Returns the current TOTP code (RFC 6238, HMAC-SHA1) for a base32 secret, like an authenticator app.",
      args: [
        { name: "secret", type: "string", description: "The base32 secret shown when 2FA was set up (spaces and case are ignored)" },
        { name: "digits", type: "number", description: "Code length, 6 to 10", default: 6 },
        { name: "period", type: "number", description: "Seconds each code is valid", default: 30 },
        { name: "time", type: "number", description: "Unix time in seconds to generate the code for; defaults to now", optional: true },
      ],
      run: ({ secret, digits, period, time }) => {
        const unixTime = time === undefined ? Date.now() / 1000 : Number(time);
        return totp(base32Decode(String(secret)), unixTime, Number(digits), Number(period));
      },
    },
  },
});

/** The TOTP code for `key` at `unixTime` (seconds). */
export function totp(key: number[], unixTime: number, digits = 6, period = 30): string {
  if (!Number.isInteger(digits) || digits < 6 || digits > 10) {
    throw new Error(`digits must be a whole number from 6 to 10, got ${digits}`);
  }
  if (!(period > 0)) throw new Error(`period must be a positive number of seconds, got ${period}`);
  return hotp(key, Math.floor(unixTime / period), digits);
}

/** The HOTP code (RFC 4226) for `key` and `counter`. */
export function hotp(key: number[], counter: number, digits = 6): string {
  // The counter is a 64-bit big-endian integer. JavaScript bit operations are
  // 32-bit, so the high and low halves are written separately.
  const message = new Array<number>(8).fill(0);
  let high = Math.floor(counter / 2 ** 32);
  let low = counter >>> 0;
  for (let i = 7; i >= 4; i--) { message[i] = low & 0xff; low >>>= 8; }
  for (let i = 3; i >= 0; i--) { message[i] = high & 0xff; high >>>= 8; }

  const mac = hmacSha1(key, message);
  // Dynamic truncation (RFC 4226 section 5.3).
  const offset = mac[19] & 0x0f;
  const binary = ((mac[offset] & 0x7f) << 24) | (mac[offset + 1] << 16) | (mac[offset + 2] << 8) | mac[offset + 3];
  // Use modulo on a safe integer: 10 ** 10 exceeds 32 bits.
  return String(binary % 10 ** digits).padStart(digits, "0");
}

const BASE32_ALPHABET = "ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";

/** Decodes RFC 4648 base32. Ignores spaces, `-`, padding and case. */
export function base32Decode(text: string): number[] {
  const clean = text.replace(/[\s=-]/g, "").toUpperCase();
  if (clean.length === 0) throw new Error("the TOTP secret is empty; pass the base32 secret, e.g. {{totp(\"JBSWY3DPEHPK3PXP\")}}");
  const bytes: number[] = [];
  let buffer = 0;
  let bits = 0;
  for (const char of clean) {
    const value = BASE32_ALPHABET.indexOf(char);
    if (value < 0) throw new Error(`the TOTP secret is not valid base32: unexpected character "${char}" (allowed: A-Z and 2-7)`);
    buffer = (buffer << 5) | value;
    bits += 5;
    if (bits >= 8) {
      bits -= 8;
      bytes.push((buffer >>> bits) & 0xff);
    }
  }
  return bytes;
}

/** HMAC (RFC 2104) with SHA-1, on byte arrays. */
export function hmacSha1(key: number[], message: number[]): number[] {
  const blockSize = 64;
  let k = key.length > blockSize ? sha1(key) : key.slice();
  k = k.concat(new Array<number>(blockSize - k.length).fill(0));
  const inner = sha1(k.map((b) => b ^ 0x36).concat(message));
  return sha1(k.map((b) => b ^ 0x5c).concat(inner));
}

/** SHA-1 (FIPS 180-4) of a byte array. Returns 20 bytes. */
export function sha1(bytes: number[]): number[] {
  // Padding: 0x80, zeros, then the bit length as a 64-bit big-endian integer.
  const bitLength = bytes.length * 8;
  const padded = bytes.concat([0x80]);
  while (padded.length % 64 !== 56) padded.push(0);
  const high = Math.floor(bitLength / 2 ** 32);
  padded.push((high >>> 24) & 0xff, (high >>> 16) & 0xff, (high >>> 8) & 0xff, high & 0xff);
  padded.push((bitLength >>> 24) & 0xff, (bitLength >>> 16) & 0xff, (bitLength >>> 8) & 0xff, bitLength & 0xff);

  const h = [0x67452301, 0xefcdab89, 0x98badcfe, 0x10325476, 0xc3d2e1f0];
  const w = new Array<number>(80);
  const rotl = (x: number, n: number) => (x << n) | (x >>> (32 - n));

  for (let block = 0; block < padded.length; block += 64) {
    for (let i = 0; i < 16; i++) {
      const j = block + i * 4;
      w[i] = (padded[j] << 24) | (padded[j + 1] << 16) | (padded[j + 2] << 8) | padded[j + 3];
    }
    for (let i = 16; i < 80; i++) w[i] = rotl(w[i - 3] ^ w[i - 8] ^ w[i - 14] ^ w[i - 16], 1);

    let [a, b, c, d, e] = h;
    for (let i = 0; i < 80; i++) {
      let f: number;
      let k: number;
      if (i < 20) { f = (b & c) | (~b & d); k = 0x5a827999; }
      else if (i < 40) { f = b ^ c ^ d; k = 0x6ed9eba1; }
      else if (i < 60) { f = (b & c) | (b & d) | (c & d); k = 0x8f1bbcdc; }
      else { f = b ^ c ^ d; k = 0xca62c1d6; }
      const temp = (rotl(a, 5) + f + e + k + w[i]) | 0;
      e = d;
      d = c;
      c = rotl(b, 30);
      b = a;
      a = temp;
    }
    h[0] = (h[0] + a) | 0;
    h[1] = (h[1] + b) | 0;
    h[2] = (h[2] + c) | 0;
    h[3] = (h[3] + d) | 0;
    h[4] = (h[4] + e) | 0;
  }
  return h.flatMap((x) => [(x >>> 24) & 0xff, (x >>> 16) & 0xff, (x >>> 8) & 0xff, x & 0xff]);
}
