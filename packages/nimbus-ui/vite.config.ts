import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

import {
  ROUTE_FILE_IGNORE_PATTERN,
  ROUTE_FILE_IGNORE_PREFIX,
} from "./scripts/route-ignore-pattern.mjs";

const proxyTarget = process.env.NIMBUS_UI_PROXY;

// Vite serves the SPA under /ui/* in production (embedded by nimbus-server)
// and under / on the dev server (port 5173) for component iteration with HMR.
export default defineConfig({
  base: "/ui/",
  plugins: [
    tanstackRouter({
      target: "react",
      autoCodeSplitting: true,
      routesDirectory: "src/routes",
      generatedRouteTree: "src/route-tree.gen.ts",
      routeFileIgnorePrefix: ROUTE_FILE_IGNORE_PREFIX,
      routeFileIgnorePattern: ROUTE_FILE_IGNORE_PATTERN,
    }),
    react(),
    tailwindcss(),
  ],
  build: {
    outDir: "dist",
    emptyOutDir: true,
    // CSP for /ui/* is `script-src 'self'` — keep all scripts external.
    modulePreload: { polyfill: false },
    sourcemap: false,
    rollupOptions: {
      output: {
        manualChunks: undefined,
      },
    },
  },
  server: {
    port: 5173,
    strictPort: true,
    // Point the HMR dev server at a running `nimbus dev` backend so the console
    // renders live data instead of an empty shell. Same-origin proxying keeps
    // the session cookie working, and rewriting `Origin` to the backend keeps
    // the server's local-origin allowlist happy for `/api` and the
    // `/convex/_nimbus` WebSocket (which it would otherwise reject with 403).
    //   NIMBUS_UI_PROXY=http://127.0.0.1:3210 npm run dev -w nimbus-ui
    proxy: proxyTarget
      ? {
          "/api": { target: proxyTarget, changeOrigin: true, headers: { origin: proxyTarget } },
          "/convex": {
            target: proxyTarget,
            changeOrigin: true,
            ws: true,
            headers: { origin: proxyTarget },
          },
          "/ui/launch": { target: proxyTarget, changeOrigin: true, headers: { origin: proxyTarget } },
        }
      : undefined,
  },
});
