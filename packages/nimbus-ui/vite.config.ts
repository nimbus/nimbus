import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";
import tailwindcss from "@tailwindcss/vite";
import { tanstackRouter } from "@tanstack/router-plugin/vite";

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
  },
});
