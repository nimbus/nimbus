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
          `${usage.file} imports package ${JSON.stringify(usage.specifier)}, but that package is not externalized.`,
          "Nimbus does not yet bundle npm packages into Convex function artifacts.",
          `Add ${JSON.stringify(usage.packageName)} to convex.json node.externalPackages, or set node.externalPackages to ["*"].`,
        ].join(" "),
      );
    }
    const resolved = await resolver.resolve(usage.specifier, usage);
    addPackageResolution(packagesByKey, resolved);
  }

  const packages = await Promise.all(
    [...packagesByKey.values()]
      .sort((left, right) => left.packageName.localeCompare(right.packageName))
      .map(async (entry) => ({
        packageName: entry.packageName,
        packageRoot: entry.packageRoot === null
          ? null
          : path.relative(appDir, entry.packageRoot).replaceAll(path.sep, "/"),
        stagedPackageRoot: entry.packageRoot === null
          ? null
          : path.relative(appDir, path.join(internalDir, "node_modules", entry.packageName))
            .replaceAll(path.sep, "/"),
        // The subpath (relative to the package root) of a browser-safe entry
        // point, when the package declares one via package.json's "browser"
        // export condition or the legacy "browser" field -- e.g. nanoid ships
        // both a Node build (`index.js`, imports `node:crypto`) and a browser
        // build (`index.browser.js`, uses `crypto.getRandomValues`). Nimbus's
        // default runtime is a web-standard isolate with no Node builtins, so
        // a default-lane import of this package must load the browser build
        // instead of whatever plain package-name resolution would otherwise
        // pick (see the specifier rewrite in main.mjs). Null when the package
        // declares no such alternate entry.
        browserEntry: entry.packageRoot === null
          ? null
          : await resolveBrowserEntry(entry.packageRoot),
        sizeBytes: entry.sizeBytes,
        resolvedSpecifiers: [...entry.resolvedSpecifiers].sort(),
        importers: [...entry.importers].sort((left, right) =>
          left.file.localeCompare(right.file)
          || left.specifier.localeCompare(right.specifier)
          || left.kind.localeCompare(right.kind)
        ),
      })),
  );

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

// Resolves a package's browser-safe entry point, if it declares one, by
// reading its own package.json -- codegen-time only, using plain Node fs
// (not the sandboxed runtime's module loader, which does not apply a
// "browser" export condition when it resolves a bare package specifier).
// Prefers the "exports"."."."browser" condition (the modern standard);
// falls back to the legacy top-level "browser" field, which maps the
// resolved main/module entry file to a browser-safe replacement.
async function resolveBrowserEntry(packageRoot) {
  let manifest;
  try {
    manifest = JSON.parse(await fs.readFile(path.join(packageRoot, "package.json"), "utf8"));
  } catch {
    return null;
  }
  return browserExportCondition(manifest) ?? legacyBrowserField(manifest);
}

function browserExportCondition(manifest) {
  const exportsField = manifest.exports;
  if (exportsField === null || typeof exportsField !== "object" || Array.isArray(exportsField)) {
    return null;
  }
  const rootExport = Object.prototype.hasOwnProperty.call(exportsField, ".")
    ? exportsField["."]
    : exportsField;
  if (rootExport === null || typeof rootExport !== "object" || Array.isArray(rootExport)) {
    return null;
  }
  return typeof rootExport.browser === "string" ? rootExport.browser : null;
}

function legacyBrowserField(manifest) {
  const browserField = manifest.browser;
  if (browserField === null || typeof browserField !== "object" || Array.isArray(browserField)) {
    return null;
  }
  for (const candidate of [manifest.main, manifest.module]) {
    if (typeof candidate !== "string") {
      continue;
    }
    const normalized = candidate.startsWith("./") ? candidate : `./${candidate}`;
    const replacement = browserField[normalized] ?? browserField[candidate];
    if (typeof replacement === "string") {
      return replacement;
    }
  }
  return null;
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
    // A hand-rolled recursive copy, not `fs.cp` -- `fs.cp`'s Node-compat
    // shim runs its own extra permission check ahead of the plain
    // read/write calls below (that check is independent of, and stricter
    // than, the sandboxed runtime's own filesystem capability grant that
    // every other codegen read/write already relies on), so it can reject a
    // copy this same sandboxed run is otherwise fully permitted to perform.
    // Dereferencing symlinks (via `stat`, not `lstat`) is also the correct
    // behavior here regardless: the staged tree is a self-contained artifact
    // read by a sandboxed runtime scoped to the app's own directory, and a
    // preserved symlink pointing outside that scope would be unreadable
    // there even if this copy could preserve it.
    await copyPackageTree(packageRoot, stagedPackageRoot);
  }
}

async function copyPackageTree(sourceDir, destDir) {
  await fs.mkdir(destDir, { recursive: true });
  const entries = await fs.readdir(sourceDir, { withFileTypes: true });
  for (const entry of entries) {
    const sourcePath = path.join(sourceDir, entry.name);
    const destPath = path.join(destDir, entry.name);
    const stat = entry.isSymbolicLink() ? await fs.stat(sourcePath) : entry;
    if (stat.isDirectory()) {
      await copyPackageTree(sourcePath, destPath);
    } else if (stat.isFile()) {
      await fs.copyFile(sourcePath, destPath);
    }
  }
}

// Despite the "node" in the name, this covers external package usage on
// both the Node-compatible lane and the default lane: both stage packages
// into the same generated_root/node_modules directory and resolve them via
// the same dynamic import() at runtime (see emit/runtime_bundle_preamble.mjs
// and compile_bindings.mjs), so a real, browser-compatible package works
// identically from either kind of module. Only "use bun" modules are
// excluded here -- the Bun/JSC program bundle is a flat, non-module script
// that cannot resolve a dynamic import() the way the shared V8 bundle does
// (see emit/runtime_bundle.mjs's generateRuntimeProgramBundle).
function collectNodeExternalPackageUsages(modules, sourceDir) {
  const usages = [];
  for (const moduleInfo of modules) {
    if (moduleInfo.runtimeEnvironment !== "node" && moduleInfo.runtimeEnvironment !== "default") {
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
