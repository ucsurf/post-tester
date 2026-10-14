/**
 * Globals available in post-tester's plugin sandbox.
 *
 * Add them to a plugin project with `"types": ["@post-tester/plugin-sdk/globals"]`
 * in `tsconfig.json` (see the SDK's example `tsconfig.json`).
 *
 * - In every file: `console`.
 * - In test files (`*.test.ts`, run by `pt plugin test`): `test`, `it`,
 *   `describe`, `expect`, and `ctx`.
 *
 * @packageDocumentation
 */

import type { PluginContext } from "./index.js";

declare global {
  /**
   * Logs to Live Activity. Inside
   * plugin functions prefer `ctx.log`, which is the same thing.
   */
  var console: {
    log(...args: unknown[]): void;
    info(...args: unknown[]): void;
    debug(...args: unknown[]): void;
    warn(...args: unknown[]): void;
    error(...args: unknown[]): void;
  };

  /**
   * The same context plugin functions receive. Available in test files, e.g.
   * to compute an expected HMAC. Inside plugin code, use the `ctx` parameter.
   */
  var ctx: PluginContext;

  /**
   * Registers a test. Tests run one after another, in the order they are
   * registered; a test fails if `fn` throws or its promise rejects.
   *
   * @example
   * ```ts
   * test("adds two numbers", () => {
   *   expect(1 + 1).toBe(2);
   * });
   * ```
   */
  function test(name: string, fn: () => unknown | Promise<unknown>): void;

  /** Alias of {@link test}. */
  function it(name: string, fn: () => unknown | Promise<unknown>): void;

  /**
   * Groups tests for readability. `fn` runs immediately and must be
   * synchronous; it only registers tests. Test names are not prefixed.
   */
  function describe(name: string, fn: () => void): void;

  /**
   * Jest-style assertions. Every matcher throws an `Error` describing the
   * mismatch, which fails the test. Prefix with `.not` to negate.
   *
   * There is no `.resolves`/`.rejects`: `await` the value first, and test a
   * rejection with `try`/`catch` (see {@link Matchers.toThrow}).
   *
   * @example
   * ```ts
   * expect(await t.templateFunction("shout", { text: "hi" })).toBe("HI");
   * expect(result.requests).toHaveLength(2);
   * expect(html).not.toContain("<script>");
   * ```
   */
  function expect<T = unknown>(actual: T): Matchers & { not: Matchers };

  /** The assertions returned by {@link expect}. */
  interface Matchers {
    /** Strict identity (`Object.is`). Use for strings, numbers and booleans. */
    toBe(expected: unknown): void;
    /** Deep equality of plain values, arrays and objects (own enumerable keys). */
    toEqual(expected: unknown): void;
    /** The value is a string that matches the regular expression, or contains the substring. */
    toMatch(pattern: RegExp | string): void;
    /** A string contains the substring, or an array contains an item deeply equal to `item`. */
    toContain(item: unknown): void;
    /** `Boolean(value)` is `true`. */
    toBeTruthy(): void;
    /** `Boolean(value)` is `false`. */
    toBeFalsy(): void;
    /** The value is not `undefined`. */
    toBeDefined(): void;
    /** The value is `undefined`. */
    toBeUndefined(): void;
    /** The value is `null`. */
    toBeNull(): void;
    /** `value > n`. */
    toBeGreaterThan(n: number): void;
    /** `value >= n`. */
    toBeGreaterThanOrEqual(n: number): void;
    /** `value < n`. */
    toBeLessThan(n: number): void;
    /** `value <= n`. */
    toBeLessThanOrEqual(n: number): void;
    /** `value.length === n` (strings and arrays). */
    toHaveLength(n: number): void;
    /**
     * The value has the property `key` (no dotted paths), optionally deeply
     * equal to `value`.
     */
    toHaveProperty(key: string, value?: unknown): void;
    /**
     * The value is a **synchronous** function that throws; if `pattern` is
     * given, the error message must match it (regex) or contain it (string).
     *
     * For async functions, catch the error yourself:
     *
     * ```ts
     * let error: Error | undefined;
     * try { await t.templateFunction("totp", {}); } catch (e) { error = e as Error; }
     * expect(error?.message).toMatch(/missing argument "secret"/);
     * ```
     */
    toThrow(pattern?: RegExp | string): void;
  }
}

export {};
