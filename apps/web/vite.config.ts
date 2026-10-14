import react from "@vitejs/plugin-react";
import { defineConfig } from "vitest/config";

const server = "http://127.0.0.1:7777";

/**
 * The server rejects foreign `Host`/`Origin` headers (DNS-rebinding protection), so the
 * dev proxy presents itself as the server's own origin.
 */
function proxyTo(target: string) {
  return {
    target,
    ws: true,
    changeOrigin: true,
    configure(proxy: { on(event: string, listener: (req: { setHeader(name: string, value: string): void }) => void): void }) {
      proxy.on("proxyReq", (req) => req.setHeader("origin", target));
      proxy.on("proxyReqWs", (req) => req.setHeader("origin", target));
    },
  };
}

export default defineConfig({
  plugins: [react()],
  // Relative asset paths: the Rust server embeds `dist/` and serves it from `/`.
  base: "./",
  build: {
    outDir: "dist",
    emptyOutDir: true,
    sourcemap: false,
    chunkSizeWarningLimit: 1500,
  },
  server: {
    host: "127.0.0.1",
    port: 5173,
    strictPort: true,
    proxy: {
      "/api": proxyTo(server),
      "/mcp": proxyTo(server),
    },
  },
  test: {
    include: ["src/**/*.test.ts"],
    environment: "node",
  },
});
