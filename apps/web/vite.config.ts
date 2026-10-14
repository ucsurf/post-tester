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
    // CodeMirror (~400 KB) loads with the first editor; React and the shell load first.
    chunkSizeWarningLimit: 450,
    rolldownOptions: {
      output: {
        codeSplitting: {
          groups: [
            { name: "react", test: /node_modules[\\/](react|react-dom|scheduler)[\\/]/ },
            // The JSON/JavaScript grammars stay with the full code editor (not needed by the URL bar).
            {
              name: "codemirror",
              test: /node_modules[\\/](?!@codemirror[\\/]lang-|@lezer[\\/](?:javascript|json)[\\/])(?:@codemirror|@lezer|codemirror|style-mod|w3c-keyname|crelt|@marijn)[\\/]/,
            },
          ],
        },
      },
    },
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
