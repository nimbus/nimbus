#!/usr/bin/env node

import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

const repoRoot = path.resolve(fileURLToPath(new URL("../", import.meta.url)));

function usage() {
  console.error("Usage: node scripts/convex-demo-overlay.mjs <convex-demos-dir> <demo>");
  process.exit(1);
}

function shouldCopy(sourcePath) {
  const basename = path.basename(sourcePath);
  return basename !== "node_modules" && basename !== ".neovex" && basename !== "_generated";
}

async function safeSymlink(target, linkPath) {
  await fs.rm(linkPath, { force: true, recursive: true });
  await fs.symlink(target, linkPath, "dir");
}

export async function prepareOverlay(convexDemosDir, demoName) {
  const sourceDir = path.resolve(convexDemosDir, demoName);
  const overlayDir = await fs.mkdtemp(path.join(os.tmpdir(), `neovex-convex-demo-${demoName}-`));

  await fs.cp(sourceDir, overlayDir, {
    recursive: true,
    filter: shouldCopy,
  });

  const nodeModulesDir = path.join(overlayDir, "node_modules");
  const neovexScopeDir = path.join(nodeModulesDir, "@neovex");
  await fs.mkdir(neovexScopeDir, { recursive: true });

  await safeSymlink(path.join(repoRoot, "packages", "convex"), path.join(nodeModulesDir, "convex"));
  await safeSymlink(path.join(repoRoot, "packages", "neovex"), path.join(nodeModulesDir, "neovex"));
  await safeSymlink(
    path.join(repoRoot, "packages", "codegen"),
    path.join(neovexScopeDir, "codegen"),
  );

  return overlayDir;
}

async function main() {
  const [convexDemosDir, demoName] = process.argv.slice(2);
  if (!convexDemosDir || !demoName) {
    usage();
  }

  const overlayDir = await prepareOverlay(convexDemosDir, demoName);
  console.log(overlayDir);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
