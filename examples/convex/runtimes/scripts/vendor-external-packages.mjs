#!/usr/bin/env node
// A real, standalone Convex/Nimbus app gets its own `npm install`-produced
// node_modules, so an externalized package like `nanoid` (see convex.json's
// node.externalPackages) always lives directly under the app's own
// node_modules/. Inside this monorepo, npm workspaces hoist that same
// package up to the repo root instead -- fine for the app's own `tsc`/`node`
// invocations (plain Node module resolution walks up and finds it there),
// but Nimbus's server-side codegen preflight runs in a sandboxed isolate
// whose filesystem-read grant is scoped to this app's own directory tree and
// deliberately cannot walk up to the monorepo root. This script closes that
// gap by giving this app a real local copy of each externalized package, so
// dev-in-monorepo resolution matches what a standalone deployed app already
// has. It runs as a prerequisite of `npm run codegen` (see package.json).
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";
import fs from "node:fs/promises";
import path from "node:path";

const appDir = fileURLToPath(new URL("..", import.meta.url));
const require = createRequire(import.meta.url);

async function readJson(filePath) {
  return JSON.parse(await fs.readFile(filePath, "utf8"));
}

async function findPackageRoot(resolvedEntryFile, packageName) {
  let dir = path.dirname(resolvedEntryFile);
  while (true) {
    const candidate = path.join(dir, "package.json");
    try {
      const manifest = await readJson(candidate);
      if (manifest.name === packageName) {
        return dir;
      }
    } catch {
      // no package.json here, or unreadable -- keep walking up
    }
    const parent = path.dirname(dir);
    if (parent === dir) {
      throw new Error(`could not find a package.json named "${packageName}" above ${resolvedEntryFile}`);
    }
    dir = parent;
  }
}

async function vendorPackage(packageName) {
  const localDir = path.join(appDir, "node_modules", packageName);
  const resolvedEntry = require.resolve(packageName, { paths: [appDir] });
  const sourceDir = await findPackageRoot(resolvedEntry, packageName);

  if (sourceDir === localDir) {
    // Already a real local copy (not hoisted) -- nothing to do.
    return;
  }

  const sourceVersion = (await readJson(path.join(sourceDir, "package.json"))).version;
  const localStat = await fs.lstat(localDir).catch(() => null);
  const localVersion = await readJson(path.join(localDir, "package.json"))
    .then((manifest) => manifest.version)
    .catch(() => null);
  if (localStat?.isDirectory() && !localStat.isSymbolicLink() && localVersion === sourceVersion) {
    return;
  }

  await fs.rm(localDir, { recursive: true, force: true });
  await fs.mkdir(path.dirname(localDir), { recursive: true });
  await fs.cp(sourceDir, localDir, { recursive: true, dereference: true });
  console.log(`vendor-external-packages: staged ${packageName}@${sourceVersion} locally (from ${sourceDir})`);
}

async function main() {
  const convexConfigPath = path.join(appDir, "convex.json");
  const convexConfig = await readJson(convexConfigPath).catch(() => ({}));
  const externalPackages = convexConfig.node?.externalPackages ?? [];
  const concretePackages = externalPackages.filter((name) => name !== "*");
  for (const packageName of concretePackages) {
    await vendorPackage(packageName);
  }
}

await main();
