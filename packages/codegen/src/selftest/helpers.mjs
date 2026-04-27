import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const cliPath = fileURLToPath(new URL("../cli.mjs", import.meta.url));

async function createAppFixture(files, { sourceDir = "convex" } = {}) {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "neovex_codegen_"));
  await fs.mkdir(path.join(appDir, sourceDir), { recursive: true });
  for (const [fileName, source] of Object.entries(files)) {
    await fs.writeFile(path.join(appDir, sourceDir, fileName), source, "utf8");
  }
  return appDir;
}

function runCli(appDir) {
  return spawnSync(process.execPath, [cliPath, "--app", appDir], {
    encoding: "utf8",
  });
}

async function readGeneratedFile(appDir, fileName, { sourceDir = "convex" } = {}) {
  return fs.readFile(path.join(appDir, sourceDir, "_generated", fileName), "utf8");
}

async function readConvexFile(appDir, fileName) {
  return fs.readFile(path.join(appDir, ".neovex", "convex", fileName), "utf8");
}

async function readConvexJson(appDir, fileName) {
  return JSON.parse(await readConvexFile(appDir, fileName));
}

async function readCloudFunctionsFile(appDir, fileName) {
  return fs.readFile(path.join(appDir, ".neovex", "firebase", fileName), "utf8");
}

async function readCloudFunctionsJson(appDir, fileName) {
  return JSON.parse(await readCloudFunctionsFile(appDir, fileName));
}

export {
  createAppFixture,
  readCloudFunctionsFile,
  readCloudFunctionsJson,
  readConvexFile,
  readConvexJson,
  readGeneratedFile,
  runCli,
};
