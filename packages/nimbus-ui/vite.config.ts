import { defineConfig } from "vite";
import react from "@vitejs/plugin-react";

// Vite serves the SPA under /ui/* in production (embedded by nimbus-server)
// and under / on the dev server (port 5173) for component iteration with HMR.
export default defineConfig({
  base: "/ui/",
  plugins: [react()],
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
