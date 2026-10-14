/**
 * Hooks that run around every request:
 * - beforeRequest: adds `X-Request-Id`, so server logs can be matched to Live Activity.
 * - afterResponse: logs a warning when a response is slower than a threshold.
 */
import { definePlugin } from "@post-tester/plugin-sdk";

export const HEADER = "X-Request-Id";
/** Variable that overrides the slow-response threshold, e.g. `slowResponseMs: 500`. */
export const THRESHOLD_VARIABLE = "slowResponseMs";
export const DEFAULT_THRESHOLD_MS = 1000;

export default definePlugin({
  hooks: {
    beforeRequest: (request, ctx) => {
      // Respect an id set by the request itself (e.g. to replay a specific trace).
      if (!request.headers.has(HEADER)) request.headers.set(HEADER, ctx.random.uuid());
      return request;
    },

    afterResponse: (response, ctx) => {
      const configured = Number(ctx.variables.get(THRESHOLD_VARIABLE));
      const threshold = Number.isFinite(configured) && configured > 0 ? configured : DEFAULT_THRESHOLD_MS;
      if (response.timeMs > threshold) {
        ctx.log.warn(`Slow response: ${Math.round(response.timeMs)} ms (threshold ${threshold} ms, status ${response.status})`);
      }
      // The response is passed on unchanged.
      return response;
    },
  },
});
