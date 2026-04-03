import { defineConfig, loadEnv } from "vite";

export default defineConfig(({ command, mode }) => {
  const env = loadEnv(mode, ".", "");
  const backend = env.NEOVEX_DEV_BACKEND ?? "http://localhost:8080";

  return {
    base: command === "build" ? "/demos/convex/http/dist/" : "/",
    server: {
      proxy: {
        "/api": backend,
        "/convex/demo": backend,
      },
    },
  };
});
