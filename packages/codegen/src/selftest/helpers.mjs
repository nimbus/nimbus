import fs from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { spawnSync } from "node:child_process";
import { fileURLToPath } from "node:url";

const cliPath = fileURLToPath(new URL("../cli.mjs", import.meta.url));

async function createAppFixture(files, { sourceDir = "convex", rootFiles = {} } = {}) {
  const appDir = await fs.mkdtemp(path.join(os.tmpdir(), "nimbus_codegen_"));
  await fs.mkdir(path.join(appDir, sourceDir), { recursive: true });
  for (const [fileName, source] of Object.entries(rootFiles)) {
    const filePath = path.join(appDir, fileName);
    await fs.mkdir(path.dirname(filePath), { recursive: true });
    await fs.writeFile(filePath, source, "utf8");
  }
  for (const [fileName, source] of Object.entries(files)) {
    const filePath = path.join(appDir, sourceDir, fileName);
    await fs.mkdir(path.dirname(filePath), { recursive: true });
    await fs.writeFile(filePath, source, "utf8");
  }
  return appDir;
}

function runCli(appDir, extraArgs = []) {
  return spawnSync(process.execPath, [cliPath, "--app", appDir, ...extraArgs], {
    encoding: "utf8",
  });
}

async function readGeneratedFile(appDir, fileName, { sourceDir = "convex" } = {}) {
  return fs.readFile(path.join(appDir, sourceDir, "_generated", fileName), "utf8");
}

async function readConvexFile(appDir, fileName) {
  return fs.readFile(path.join(appDir, ".nimbus", "convex", fileName), "utf8");
}

async function readConvexJson(appDir, fileName) {
  return JSON.parse(await readConvexFile(appDir, fileName));
}

async function readCloudFunctionsFile(appDir, fileName) {
  return fs.readFile(path.join(appDir, ".nimbus", "firebase", fileName), "utf8");
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
