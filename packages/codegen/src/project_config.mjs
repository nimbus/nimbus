import fs from "node:fs/promises";
import path from "node:path";

import { fileExists } from "./app.mjs";

const DEFAULT_NODE_VERSION = "24";
const SUPPORTED_NODE_VERSIONS = new Set(["20", "22", "24", "26"]);
const SUPPORTED_NODE_VERSION_LABEL = [...SUPPORTED_NODE_VERSIONS]
  .map((version) => JSON.stringify(version))
  .join(", ");

function defaultProjectConfig() {
  return {
    functions: null,
    generateCommonJSApi: false,
    node: {
      externalPackages: [],
      nodeVersion: DEFAULT_NODE_VERSION,
      runtimeTarget: runtimeTargetForNodeVersion(DEFAULT_NODE_VERSION),
    },
  };
}

async function loadProjectConfig(appDir) {
  const configPath = path.join(appDir, "convex.json");
  if (!await fileExists(configPath)) {
    return defaultProjectConfig();
  }

  let parsed;
  try {
    parsed = JSON.parse(await fs.readFile(configPath, "utf8"));
  } catch (error) {
    throw new Error(
      `Invalid convex.json in ${appDir}: ${error instanceof Error ? error.message : String(error)}`,
    );
  }

  if (parsed === null || Array.isArray(parsed) || typeof parsed !== "object") {
    throw new Error(`Invalid convex.json in ${appDir}: expected a JSON object.`);
  }

  return {
    functions: parseFunctionsPath(parsed.functions, appDir),
    generateCommonJSApi: parseGenerateCommonJSApi(parsed.generateCommonJSApi, appDir),
    node: parseNodeConfig(parsed.node, appDir),
  };
}

// The `functions` setting relocates the app's function source directory
// (e.g. for Create React App projects, which cannot import from outside
// `src/`). It is a path relative to appDir; resolveSourceRoot in app.mjs
// resolves and validates the directory actually exists.
function parseFunctionsPath(rawFunctions, appDir) {
  if (rawFunctions === undefined) {
    return null;
  }
  if (typeof rawFunctions !== "string" || rawFunctions.trim().length === 0) {
    throw new Error(
      `Invalid convex.json in ${appDir}: "functions" must be a non-empty relative path string.`,
    );
  }
  if (path.isAbsolute(rawFunctions)) {
    throw new Error(
      `Invalid convex.json in ${appDir}: "functions" must be a path relative to ${appDir}, not an absolute path.`,
    );
  }
  const normalized = path.normalize(rawFunctions);
  if (normalized === ".." || normalized.startsWith(`..${path.sep}`)) {
    throw new Error(
      `Invalid convex.json in ${appDir}: "functions" must resolve inside ${appDir}, not escape it via "..".`,
    );
  }
  return rawFunctions;
}

function parseGenerateCommonJSApi(rawGenerateCommonJSApi, appDir) {
  if (rawGenerateCommonJSApi === undefined) {
    return false;
  }
  if (typeof rawGenerateCommonJSApi !== "boolean") {
    throw new Error(
      `Invalid convex.json in ${appDir}: "generateCommonJSApi" must be a boolean.`,
    );
  }
  return rawGenerateCommonJSApi;
}

function parseNodeConfig(rawNode, appDir) {
  if (rawNode === undefined) {
    return defaultProjectConfig().node;
  }
  if (rawNode === null || Array.isArray(rawNode) || typeof rawNode !== "object") {
    throw new Error(`Invalid convex.json in ${appDir}: "node" must be an object.`);
  }

  const nodeVersion = rawNode.nodeVersion ?? DEFAULT_NODE_VERSION;
  if (typeof nodeVersion !== "string" || !SUPPORTED_NODE_VERSIONS.has(nodeVersion)) {
    throw new Error(
      `Invalid convex.json in ${appDir}: "node.nodeVersion" must be one of ${SUPPORTED_NODE_VERSION_LABEL}.`,
    );
  }

  return {
    externalPackages: parseExternalPackages(rawNode.externalPackages, appDir),
    nodeVersion,
    runtimeTarget: runtimeTargetForNodeVersion(nodeVersion),
  };
}

function parseExternalPackages(rawExternalPackages, appDir) {
  if (rawExternalPackages === undefined) {
    return [];
  }
  if (!Array.isArray(rawExternalPackages)) {
    throw new Error(
      `Invalid convex.json in ${appDir}: "node.externalPackages" must be an array of package specifiers.`,
    );
  }
  const externalPackages = [];
  for (const packageName of rawExternalPackages) {
    if (typeof packageName !== "string" || packageName.length === 0) {
      throw new Error(
        `Invalid convex.json in ${appDir}: "node.externalPackages" entries must be non-empty strings.`,
      );
    }
    if (!externalPackages.includes(packageName)) {
      externalPackages.push(packageName);
    }
  }
  if (externalPackages.includes("*") && externalPackages.length !== 1) {
    throw new Error(
      `Invalid convex.json in ${appDir}: "node.externalPackages" must use "*" by itself when externalizing every Node action package.`,
    );
  }
  return externalPackages;
}

function runtimeTargetForNodeVersion(nodeVersion) {
  return `node${nodeVersion}`;
}

export {
  DEFAULT_NODE_VERSION,
  SUPPORTED_NODE_VERSIONS,
  defaultProjectConfig,
  loadProjectConfig,
  runtimeTargetForNodeVersion,
};
