import fs from "node:fs/promises";
import path from "node:path";
import { createRequire } from "node:module";

import {
  collectModuleSpecifiers,
  isExternalPackageSpecifier,
  packageNameFromSpecifier,
} from "./module_specifiers.mjs";

const REPORT_VERSION = 1;
const CONVEX_CLOUD_EXTERNAL_PACKAGE_LIMITS = Object.freeze({
  zippedBytes: 45 * 1024 * 1024,
  unzippedBytes: 240 * 1024 * 1024,
});

async function createNodeExternalPackageReport({
  appDir,
  internalDir,
  modules,
  projectConfig,
  sourceDir,
}) {
  const mode = externalPackageMode(projectConfig.node.externalPackages);
  const nodeImports = collectNodeExternalPackageUsages(modules, sourceDir);
  const resolver = createPackageResolver(appDir);
  const configuredPackages = mode === "explicit"
    ? projectConfig.node.externalPackages
    : [];
  const packagesByKey = new Map();

  for (const packageSpecifier of configuredPackages) {
    const packageName = packageNameFromSpecifier(packageSpecifier);
    if (packageName === null) {
      throw new Error(
        `Invalid convex.json in ${appDir}: node.externalPackages entry ${JSON.stringify(packageSpecifier)} is not a package specifier.`,
      );
    }
    const resolved = await resolver.resolve(packageSpecifier, {
      configured: true,
      importer: "convex.json",
      kind: "config",
      packageName,
      specifier: packageSpecifier,
    });
    addPackageResolution(packagesByKey, resolved);
  }

  for (const usage of nodeImports) {
    if (!isExternalPackageAllowed(projectConfig.node.externalPackages, usage)) {
      throw new Error(
        [
          `${usage.file} imports package ${JSON.stringify(usage.specifier)} from a Node action module, but that package is not externalized.`,
          "Nimbus does not yet bundle npm packages into Convex function artifacts.",
          `Add ${JSON.stringify(usage.packageName)} to convex.json node.externalPackages, or set node.externalPackages to ["*"].`,
        ].join(" "),
      );
    }
    const resolved = await resolver.resolve(usage.specifier, usage);
    addPackageResolution(packagesByKey, resolved);
  }

  const packages = [...packagesByKey.values()]
    .sort((left, right) => left.packageName.localeCompare(right.packageName))
    .map((entry) => ({
      packageName: entry.packageName,
      packageRoot: entry.packageRoot === null
        ? null
        : path.relative(appDir, entry.packageRoot).replaceAll(path.sep, "/"),
      stagedPackageRoot: entry.packageRoot === null
        ? null
        : path.relative(appDir, path.join(internalDir, "node_modules", entry.packageName))
          .replaceAll(path.sep, "/"),
      sizeBytes: entry.sizeBytes,
      resolvedSpecifiers: [...entry.resolvedSpecifiers].sort(),
      importers: [...entry.importers].sort((left, right) =>
        left.file.localeCompare(right.file)
        || left.specifier.localeCompare(right.specifier)
        || left.kind.localeCompare(right.kind)
      ),
    }));

  return {
    version: REPORT_VERSION,
    mode,
    configuredExternalPackages: projectConfig.node.externalPackages,
    limits: {
      convexCloudReference: CONVEX_CLOUD_EXTERNAL_PACKAGE_LIMITS,
      enforcedByNimbus: false,
    },
    stagingRoot: path.relative(appDir, path.join(internalDir, "node_modules"))
      .replaceAll(path.sep, "/"),
    packages,
  };
}

async function stageNodeExternalPackages(appDir, report) {
  await fs.rm(path.join(appDir, report.stagingRoot), { force: true, recursive: true });
  for (const entry of report.packages) {
    if (entry.packageRoot === null || entry.stagedPackageRoot === null) {
      continue;
    }
    const packageRoot = path.join(appDir, entry.packageRoot);
    const stagedPackageRoot = path.join(appDir, entry.stagedPackageRoot);
    await fs.mkdir(path.dirname(stagedPackageRoot), { recursive: true });
    await fs.cp(packageRoot, stagedPackageRoot, {
      dereference: false,
      errorOnExist: false,
      force: true,
      recursive: true,
    });
  }
}

function collectNodeExternalPackageUsages(modules, sourceDir) {
  const usages = [];
  for (const moduleInfo of modules) {
    if (moduleInfo.runtimeEnvironment !== "node") {
      continue;
    }
    const file = path.relative(sourceDir, moduleInfo.filePath).replaceAll(path.sep, "/");
    for (const { kind, specifier } of collectModuleSpecifiers(moduleInfo.source)) {
      if (!isExternalPackageSpecifier(specifier)) {
        continue;
      }
      usages.push({
        file,
        kind,
        packageName: packageNameFromSpecifier(specifier),
        specifier,
      });
    }
  }
  return usages;
}

function externalPackageMode(externalPackages) {
  if (externalPackages.length === 0) {
    return "none";
  }
  return externalPackages.length === 1 && externalPackages[0] === "*" ? "all" : "explicit";
}

function isExternalPackageAllowed(externalPackages, usage) {
  if (externalPackages.length === 1 && externalPackages[0] === "*") {
    return true;
  }
  return externalPackages.includes(usage.specifier)
    || externalPackages.includes(usage.packageName);
}

function createPackageResolver(appDir) {
  const appRequire = createRequire(path.join(appDir, "package.json"));
  return {
    async resolve(specifier, usage) {
      let resolvedPath;
      try {
        resolvedPath = appRequire.resolve(specifier);
      } catch (error) {
        throw new Error(
          [
            `${usage.importer ?? usage.file} externalizes package ${JSON.stringify(specifier)}, but it was not resolvable from local node_modules.`,
            "Run your package manager install command so Nimbus can validate and stage the same package version Convex would derive locally.",
            `Resolver error: ${error instanceof Error ? error.message : String(error)}`,
          ].join(" "),
        );
      }
      const packageRoot = await findPackageRoot(resolvedPath, appDir);
      const sizeBytes = packageRoot === null ? 0 : await directorySizeBytes(packageRoot);
      return {
        importers: new Set([{
          file: usage.importer ?? usage.file,
          kind: usage.kind,
          specifier: usage.specifier ?? specifier,
        }]),
        packageName: usage.packageName ?? packageNameFromSpecifier(specifier),
        packageRoot,
        resolvedSpecifiers: new Set([specifier]),
        sizeBytes,
      };
    },
  };
}

function addPackageResolution(packagesByKey, resolved) {
  const key = resolved.packageName;
  const existing = packagesByKey.get(key);
  if (existing === undefined) {
    packagesByKey.set(key, resolved);
    return;
  }
  for (const importer of resolved.importers) {
    existing.importers.add(importer);
  }
  for (const specifier of resolved.resolvedSpecifiers) {
    existing.resolvedSpecifiers.add(specifier);
  }
  existing.sizeBytes = Math.max(existing.sizeBytes, resolved.sizeBytes);
  existing.packageRoot ??= resolved.packageRoot;
}

async function findPackageRoot(resolvedPath, appDir) {
  let current = path.dirname(resolvedPath);
  const root = path.parse(appDir).root;
  while (current !== root) {
    if (await fileExists(path.join(current, "package.json"))) {
      return current;
    }
    current = path.dirname(current);
  }
  return null;
}

async function directorySizeBytes(directoryPath) {
  let total = 0;
  const entries = await fs.readdir(directoryPath, { withFileTypes: true });
  for (const entry of entries) {
    const entryPath = path.join(directoryPath, entry.name);
    if (entry.isDirectory()) {
      total += await directorySizeBytes(entryPath);
    } else if (entry.isFile()) {
      total += (await fs.stat(entryPath)).size;
    }
  }
  return total;
}

async function fileExists(filePath) {
  try {
    const stat = await fs.stat(filePath);
    return stat.isFile();
  } catch (error) {
    if (error && typeof error === "object" && error.code === "ENOENT") {
      return false;
    }
    throw error;
  }
}

export {
  collectNodeExternalPackageUsages,
  createNodeExternalPackageReport,
  externalPackageMode,
  stageNodeExternalPackages,
};
