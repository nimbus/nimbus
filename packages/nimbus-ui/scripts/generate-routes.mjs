#!/usr/bin/env node
// Pre-build step: generate the TanStack Router route tree from src/routes
// before tsc/vite run. Mirrors the tanstackRouter() vite plugin generator
// invoked at dev/build time so that typechecking sees the file too.
import { fileURLToPath } from "node:url";
import { readdir, readFile } from "node:fs/promises";
import { basename, dirname, join, relative, resolve } from "node:path";

import { Generator, configSchema } from "@tanstack/router-generator";

import {
  ROUTE_FILE_IGNORE_PATTERN,
  ROUTE_FILE_IGNORE_PREFIX,
} from "./route-ignore-pattern.mjs";

const here = dirname(fileURLToPath(import.meta.url));
const pkgRoot = resolve(here, "..");
const routesDirectory = resolve(pkgRoot, "src/routes");

async function routeSources(root, found = []) {
  for (const entry of await readdir(root, { withFileTypes: true })) {
    const candidate = join(root, entry.name);
    if (entry.isDirectory()) await routeSources(candidate, found);
    else if (/\.(?:ts|tsx)$/u.test(entry.name)) found.push(candidate);
  }
  return found;
}

async function verifyRouteFileOwnership() {
  const errors = [];
  for (const sourcePath of await routeSources(routesDirectory)) {
    const name = basename(sourcePath);
    const ignored = name.startsWith(ROUTE_FILE_IGNORE_PREFIX) || /\.spec\.(?:ts|tsx)$/u.test(name);
    const declaresRoute = /export\s+const\s+Route\b/u.test(await readFile(sourcePath, "utf8"));
    const displayPath = relative(pkgRoot, sourcePath);
    if (!ignored && !declaresRoute) {
      errors.push(`${displayPath} is support code and must use the ${ROUTE_FILE_IGNORE_PREFIX} prefix`);
    } else if (ignored && declaresRoute) {
      errors.push(`${displayPath} declares Route but its name excludes it from route generation`);
    }
  }
  if (errors.length > 0) throw new Error(`route file ownership is invalid:\n  ${errors.join("\n  ")}`);
}

await verifyRouteFileOwnership();

const parsed = configSchema.parse({
  target: "react",
  routesDirectory,
  generatedRouteTree: resolve(pkgRoot, "src/route-tree.gen.ts"),
  autoCodeSplitting: true,
  routeFileIgnorePrefix: ROUTE_FILE_IGNORE_PREFIX,
  routeFileIgnorePattern: ROUTE_FILE_IGNORE_PATTERN,
  tmpDir: resolve(pkgRoot, "node_modules/.tanstack-router"),
});

const generator = new Generator({ config: parsed, root: pkgRoot });

await generator.run();
console.log("[nimbus-ui] route tree generated");
