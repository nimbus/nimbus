import fs from "node:fs/promises";
import path from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { build } from "esbuild";

const packageRoot = fileURLToPath(new URL("./", import.meta.url));
const entryPoint = fileURLToPath(new URL("./src/browser.ts", import.meta.url));
const distDir = path.join(packageRoot, "dist");
const servedDir = path.resolve(packageRoot, "../../demos/convex/vendor");
const distFile = path.join(distDir, "browser.bundle.js");
const servedFile = path.join(servedDir, "browser.bundle.js");

export async function buildBrowserBundle() {
  await fs.mkdir(distDir, { recursive: true });
  await build({
    entryPoints: [entryPoint],
    bundle: true,
    format: "iife",
    globalName: "convex",
    outfile: distFile,
    logLevel: "silent",
    platform: "browser",
    target: "es2022",
  });

  await fs.mkdir(servedDir, { recursive: true });
  await fs.copyFile(distFile, servedFile);

  return { distFile, servedFile };
}

async function main() {
  const { distFile: builtDistFile, servedFile: builtServedFile } = await buildBrowserBundle();
  console.log(`wrote ${builtDistFile}`);
  console.log(`wrote ${builtServedFile}`);
}

if (process.argv[1] && import.meta.url === pathToFileURL(process.argv[1]).href) {
  main().catch((error) => {
    console.error(error);
    process.exit(1);
  });
}
