import fs from "node:fs/promises";
import path from "node:path";

import { fileExists } from "./app.mjs";

const DEFAULT_NODE_VERSION = "22";
const SUPPORTED_NODE_VERSIONS = new Set(["20", "22", "24"]);

function defaultProjectConfig() {
  return {
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
    node: parseNodeConfig(parsed.node, appDir),
  };
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
      `Invalid convex.json in ${appDir}: "node.nodeVersion" must be one of "20", "22", or "24".`,
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
