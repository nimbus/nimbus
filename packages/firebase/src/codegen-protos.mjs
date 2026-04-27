import assert from "node:assert/strict";
import { spawnSync } from "node:child_process";
import fs from "node:fs";
import fsp from "node:fs/promises";
import os from "node:os";
import path from "node:path";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const packageRoot = fileURLToPath(new URL("../", import.meta.url));
const repoRoot = fileURLToPath(new URL("../../../", import.meta.url));
const protoRoot = path.join(repoRoot, "crates", "neovex-server", "proto");
const googleProtoRoot = path.join(protoRoot, "google");
const outputRoot = path.join(packageRoot, "src", "gen");
const protocPlugin = resolvePluginBinary();

await main();

async function main() {
  const protoc = resolveProtocBinary();
  const protoFiles = await listProtoFiles(googleProtoRoot);
  assert.ok(protoFiles.length > 0, "No vendored Firestore proto files were found.");

  await fsp.rm(outputRoot, { recursive: true, force: true });
  await fsp.mkdir(outputRoot, { recursive: true });

  const args = [
    `--plugin=protoc-gen-es=${protocPlugin}`,
    `--proto_path=${protoRoot}`,
    `--es_out=${outputRoot}`,
    "--es_opt=target=ts,json_types=true,import_extension=none",
    ...protoFiles,
  ];
  const result = spawnSync(protoc, args, {
    cwd: packageRoot,
    stdio: "inherit",
  });
  if (result.status !== 0) {
    throw new Error(`protoc exited with status ${result.status ?? "unknown"}`);
  }
}

function resolvePluginBinary() {
  const packageJsonPath = require.resolve("@bufbuild/protoc-gen-es/package.json");
  const packageJson = JSON.parse(fs.readFileSync(packageJsonPath, "utf8"));
  const pluginRelativePath =
    typeof packageJson.bin === "string"
      ? packageJson.bin
      : packageJson.bin?.["protoc-gen-es"];
  assert.ok(pluginRelativePath, "Unable to resolve protoc-gen-es binary.");
  return path.join(path.dirname(packageJsonPath), pluginRelativePath);
}

function resolveProtocBinary() {
  if (process.env.PROTOC) {
    return process.env.PROTOC;
  }

  const vendoredCrateName = vendoredProtocCrateName();
  const pinnedVersion = readPinnedCargoPackageVersion(vendoredCrateName);
  const cargoHome = process.env.CARGO_HOME ?? path.join(os.homedir(), ".cargo");
  const registrySrc = path.join(cargoHome, "registry", "src");
  const executableName = process.platform === "win32" ? "protoc.exe" : "protoc";
  const candidates = [];

  if (fs.existsSync(registrySrc)) {
    for (const registryEntry of fs.readdirSync(registrySrc, { withFileTypes: true })) {
      if (!registryEntry.isDirectory()) {
        continue;
      }
      const registryPath = path.join(registrySrc, registryEntry.name);
      if (pinnedVersion) {
        const exactCandidate = path.join(
          registryPath,
          `${vendoredCrateName}-${pinnedVersion}`,
          "bin",
          executableName,
        );
        if (fs.existsSync(exactCandidate)) {
          return exactCandidate;
        }
      }
      for (const crateEntry of fs.readdirSync(registryPath, { withFileTypes: true })) {
        if (!crateEntry.isDirectory() || !crateEntry.name.startsWith(`${vendoredCrateName}-`)) {
          continue;
        }
        const candidate = path.join(registryPath, crateEntry.name, "bin", executableName);
        if (fs.existsSync(candidate)) {
          candidates.push(candidate);
        }
      }
    }
  }

  if (candidates.length > 0) {
    candidates.sort((left, right) => right.localeCompare(left));
    return candidates[0];
  }

  throw new Error(
    [
      "Unable to find the vendored protoc binary used by neovex-server.",
      "Set PROTOC explicitly or run a cargo command that fetches the",
      `"${vendoredCrateName}" package into your Cargo registry first.`,
    ].join(" "),
  );
}

function readPinnedCargoPackageVersion(packageName) {
  const cargoLockPath = path.join(repoRoot, "Cargo.lock");
  if (!fs.existsSync(cargoLockPath)) {
    return null;
  }
  const cargoLock = fs.readFileSync(cargoLockPath, "utf8");
  const escapedName = packageName.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  const match = cargoLock.match(
    new RegExp(`name = "${escapedName}"\\nversion = "([^"]+)"`),
  );
  return match?.[1] ?? null;
}

function vendoredProtocCrateName() {
  if (process.platform === "darwin") {
    if (process.arch === "arm64") {
      return "protoc-bin-vendored-macos-aarch_64";
    }
    if (process.arch === "x64") {
      return "protoc-bin-vendored-macos-x86_64";
    }
  }
  if (process.platform === "linux") {
    if (process.arch === "arm64") {
      return "protoc-bin-vendored-linux-aarch_64";
    }
    if (process.arch === "x64") {
      return "protoc-bin-vendored-linux-x86_64";
    }
    if (process.arch === "ia32") {
      return "protoc-bin-vendored-linux-x86_32";
    }
    if (process.arch === "ppc64") {
      return "protoc-bin-vendored-linux-ppcle_64";
    }
    if (process.arch === "s390x") {
      return "protoc-bin-vendored-linux-s390_64";
    }
  }
  if (process.platform === "win32") {
    return "protoc-bin-vendored-win32";
  }
  throw new Error(
    `Unsupported platform for vendored protoc resolution: ${process.platform}/${process.arch}`,
  );
}

async function listProtoFiles(root) {
  const files = [];
  await walk(root, files);
  files.sort((left, right) => left.localeCompare(right));
  return files;
}

async function walk(directory, files) {
  for (const entry of await fsp.readdir(directory, { withFileTypes: true })) {
    const entryPath = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      await walk(entryPath, files);
      continue;
    }
    if (!entry.isFile() || !entry.name.endsWith(".proto")) {
      continue;
    }
    files.push(path.relative(protoRoot, entryPath));
  }
}
