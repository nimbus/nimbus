#!/usr/bin/env node
// Pre-build step: generate the TanStack Router route tree from src/routes
// before tsc/vite run. Mirrors the tanstackRouter() vite plugin generator
// invoked at dev/build time so that typechecking sees the file too.
import { fileURLToPath } from "node:url";
import { dirname, resolve } from "node:path";

import { Generator, configSchema } from "@tanstack/router-generator";

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = resolve(here, "..");

const parsed = configSchema.parse({
  target: "react",
  routesDirectory: resolve(pkgRoot, "src/routes"),
  generatedRouteTree: resolve(pkgRoot, "src/route-tree.gen.ts"),
  autoCodeSplitting: true,
  routeFileIgnorePattern: "\\.spec\\.(ts|tsx)$",
  tmpDir: resolve(pkgRoot, "node_modules/.tanstack-router"),
});

const generator = new Generator({ config: parsed, root: pkgRoot });

await generator.run();
console.log("[nimbus-ui] route tree generated");
