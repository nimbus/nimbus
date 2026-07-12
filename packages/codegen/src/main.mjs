import fs from "node:fs/promises";
import path from "node:path";

import {
  collectModuleFiles,
  resolveAppDirectory,
  resolveSourceRoot,
  tryResolveSourceRoot,
  sha256Hex,
} from "./app.mjs";
import { loadAuthConfig } from "./auth_config.mjs";
import { generateCloudFunctionsArtifacts } from "./cloud_functions.mjs";
import { generateApiCjsFile, generateApiFile, generateDataModelFile, generateScheduledFunctionsFile, generateServerFile } from "./emit/generated_files.mjs";
import {
  generateRuntimeBundle,
  generateRuntimeProgramBundle,
} from "./emit/runtime_bundle.mjs";
import {
  collectNodeApiDiagnostics,
  formatNodeApiDiagnostics,
} from "./node_api_diagnostics.mjs";
import { assertTenantBundleAdmission, packageNameFromSpecifier } from "./module_specifiers.mjs";
import {
  createNodeExternalPackageReport,
  stageNodeExternalPackages,
} from "./node_external_packages.mjs";
import { parseHttpRoutes, parseModule, validateCrossModuleRuntimeImports } from "./parser.mjs";
import { loadProjectConfig } from "./project_config.mjs";
import {
  runtimeLaneMetadata,
  runtimeMetadataForFunction,
} from "./runtime_metadata.mjs";
import { loadSchemaDefinition } from "./schema.mjs";

// 1-based line in the module source where a runtime handler's verbatim text
// begins, used to remap thrown-error stack frames back to the developer's own
// source (see emit/runtime_remap.mjs). The handler text is sliced from this
// same source, so a direct search locates it; returns null if it cannot be
// resolved (the remap then degrades gracefully to no location).
function handlerOriginLine(moduleSource, handlerText) {
  if (
    typeof moduleSource !== "string" ||
    typeof handlerText !== "string" ||
    handlerText.length === 0
  ) {
    return null;
  }
  const index = moduleSource.indexOf(handlerText);
  if (index < 0) {
    return null;
  }
  let line = 1;
  for (let i = 0; i < index; i++) {
    if (moduleSource.charCodeAt(i) === 10) {
      line++;
    }
  }
  return line;
}

// Runtime handlers are emitted verbatim into the bundle, where the preamble
// compiles every handler eagerly at module evaluation — one handler that is
// not valid JavaScript (e.g. TypeScript-only syntax such as `as` casts or
// type annotations, which codegen does not strip) would disable the entire
// bundle. Reject it here, at codegen time, naming the offending handler.
function assertRuntimeHandlerSyntax(fn, moduleInfo) {
  if (typeof fn.runtimeHandler !== "string" || fn.runtimeHandler.length === 0) {
    return;
  }
  try {
    // Mirrors the bundle preamble's compileRuntimeHandler expression shape.
    new Function(`return (${fn.runtimeHandler});`);
  } catch (error) {
    const line = handlerOriginLine(moduleInfo.source, fn.runtimeHandler);
    const location =
      line === null ? moduleInfo.moduleName : `${moduleInfo.moduleName}:${line}`;
    throw new Error(
      `convex function ${fn.name} has a handler that is not valid JavaScript (${location}): ${error.message}. ` +
        "TypeScript-only syntax (type annotations, `as` casts, generics) is not stripped from runtime handlers — rewrite the handler without it.",
    );
  }
}

const NODE_EXTERNAL_PACKAGE_BINDING_TYPES = new Set([
  "node_external_package_default",
  "node_external_package_namespace",
  "node_external_package_named",
]);

// Nimbus's default runtime is a web-standard V8 isolate with no Node
// builtins; its dynamic import() resolves a bare package specifier the same
// way "use node" lane imports do (plain node_modules resolution, no
// "browser" export condition applied), so a default-lane import of a package
// whose main entry uses a Node builtin (e.g. nanoid's `index.js` imports
// `crypto`) fails at invocation time even though the package also ships a
// browser-safe build. createNodeExternalPackageReport already resolved each
// package's browser-safe entry point, if any (see resolveBrowserEntry in
// node_external_packages.mjs); this rewrites only *default-lane, bare
// package-root* binding specifiers to reference that entry file directly by
// its staged relative path -- a plain relative import, which bypasses
// package.json "exports" subpath gating entirely, unlike a rewritten bare
// specifier would. "use node" lane bindings and subpath imports (e.g.
// "nanoid/async") are left untouched: a subpath import already names a
// specific file the developer chose, not the package's ambiguous main entry.
function rewriteDefaultLaneExternalPackageSpecifiers(manifest, nodeExternalPackageReport, internalDir, appDir) {
  const browserEntryByPackageName = new Map(
    nodeExternalPackageReport.packages
      .filter((entry) => entry.browserEntry !== null && entry.stagedPackageRoot !== null)
      .map((entry) => [entry.packageName, entry]),
  );
  if (browserEntryByPackageName.size === 0) {
    return;
  }
  for (const fn of manifest) {
    if (fn.runtime_environment !== "default" || fn.runtime_bindings === undefined) {
      continue;
    }
    for (const descriptor of Object.values(fn.runtime_bindings)) {
      if (!NODE_EXTERNAL_PACKAGE_BINDING_TYPES.has(descriptor.type)) {
        continue;
      }
      if (packageNameFromSpecifier(descriptor.specifier) !== descriptor.specifier) {
        continue; // a subpath specifier (e.g. "nanoid/async"), not a bare package root import
      }
      const entry = browserEntryByPackageName.get(descriptor.specifier);
      if (entry === undefined) {
        continue;
      }
      const browserFileAbs = path.join(appDir, entry.stagedPackageRoot, entry.browserEntry);
      const relFromBundle = path.relative(internalDir, browserFileAbs).replaceAll(path.sep, "/");
      descriptor.specifier = relFromBundle.startsWith(".") ? relFromBundle : `./${relFromBundle}`;
    }
  }
}

async function generateConvexArtifacts({
  appDir,
  sourceRoot,
  projectConfig: providedProjectConfig,
  debugNodeApis = false,
  onInfo,
} = {}) {
  // Loaded before source-root resolution: convex.json's "functions" setting
  // can relocate the source directory, so the config that decides where to
  // look must come first — and both reads should be the same read, not two
  // separate convex.json parses that could race a concurrent edit under
  // `nimbus dev`'s watch loop.
  const projectConfig = providedProjectConfig ?? await loadProjectConfig(appDir);
  const resolvedSourceRoot =
    sourceRoot ?? await resolveSourceRoot(appDir, { functionsOverride: projectConfig.functions });
  const sourceDir = resolvedSourceRoot.sourceDirPath;
  const packageNamespace = resolvedSourceRoot.packageNamespace;
  const generatedDir = path.join(sourceDir, "_generated");
  const internalDir = path.join(appDir, ".nimbus", "convex");
  const schema = await loadSchemaDefinition(sourceDir);
  const authConfig = await loadAuthConfig(sourceDir);

  const moduleFiles = await collectModuleFiles(sourceDir);
  await validateCrossModuleRuntimeImports(sourceDir, moduleFiles);
  const modules = [];
  const manifest = [];

  for (const filePath of moduleFiles) {
    const moduleInfo = await parseModule(sourceDir, filePath, schema, { debugNodeApis });
    assertTenantBundleAdmission(moduleInfo.source, {
      file: path.relative(sourceDir, filePath).replaceAll(path.sep, "/"),
    });
    modules.push(moduleInfo);
    for (const fn of moduleInfo.functions) {
      if (fn.kind === "http_action") {
        continue;
      }
      assertRuntimeHandlerSyntax(fn, moduleInfo);
      const runtimeMetadata = runtimeMetadataForFunction({
        runtimeEnvironment: fn.runtimeEnvironment,
        projectConfig,
      });
      manifest.push({
        name: fn.name,
        export: fn.exportName,
        module: moduleInfo.moduleName,
        kind: fn.kind,
        visibility: fn.visibility,
        schedulable: fn.kind === "mutation",
        runtime_environment: fn.runtimeEnvironment,
        ...runtimeMetadata,
        node_version:
          fn.runtimeEnvironment === "node" ? projectConfig.node.nodeVersion : null,
        node_runtime_target:
          fn.runtimeEnvironment === "node" ? projectConfig.node.runtimeTarget : null,
        plan: fn.plan,
        runtime_handler: fn.runtimeHandler ?? null,
        runtime_handler_line: fn.runtimeHandler
          ? handlerOriginLine(moduleInfo.source, fn.runtimeHandler)
          : null,
        runtime_bindings: fn.runtimeHandler ? (fn.runtimeBindings ?? {}) : undefined,
      });
    }
  }

  const nodeExternalPackageReport = await createNodeExternalPackageReport({
    appDir,
    internalDir,
    modules,
    projectConfig,
    sourceDir,
  });
  rewriteDefaultLaneExternalPackageSpecifiers(manifest, nodeExternalPackageReport, internalDir, appDir);

  const httpRoutes = await parseHttpRoutes(sourceDir, schema, modules);
  if (debugNodeApis) {
    onInfo?.(formatNodeApiDiagnostics(collectNodeApiDiagnostics(modules, sourceDir)));
  }

  await fs.mkdir(generatedDir, { recursive: true });
  await fs.mkdir(internalDir, { recursive: true });
  await stageNodeExternalPackages(appDir, nodeExternalPackageReport);
  await fs.writeFile(
    path.join(generatedDir, "api.ts"),
    generateApiFile(modules, schema, packageNamespace),
    "utf8",
  );
  const apiCjsPath = path.join(generatedDir, "api_cjs.cjs");
  if (projectConfig.generateCommonJSApi) {
    await fs.writeFile(apiCjsPath, generateApiCjsFile(modules, packageNamespace), "utf8");
  } else {
    await fs.rm(apiCjsPath, { force: true });
  }
  await fs.writeFile(
    path.join(generatedDir, "server.ts"),
    generateServerFile(packageNamespace),
    "utf8",
  );
  await fs.writeFile(
    path.join(generatedDir, "scheduled_functions.ts"),
    generateScheduledFunctionsFile(modules, schema, packageNamespace),
    "utf8",
  );
  await fs.writeFile(
    path.join(generatedDir, "dataModel.d.ts"),
    generateDataModelFile(schema, packageNamespace),
    "utf8",
  );
  await fs.writeFile(
    path.join(internalDir, "functions.json"),
    `${JSON.stringify({
      node: projectConfig.node,
      runtime_lanes: runtimeLaneMetadata(projectConfig),
      functions: manifest,
    }, null, 2)}\n`,
    "utf8",
  );
  await fs.writeFile(
    path.join(internalDir, "node_external_packages.json"),
    `${JSON.stringify(nodeExternalPackageReport, null, 2)}\n`,
    "utf8",
  );
  await fs.writeFile(
    path.join(internalDir, "schema.json"),
    `${JSON.stringify(schema, null, 2)}\n`,
    "utf8",
  );
  await fs.writeFile(
    path.join(internalDir, "http_routes.json"),
    `${JSON.stringify({ routes: httpRoutes }, null, 2)}\n`,
    "utf8",
  );
  await fs.writeFile(
    path.join(internalDir, "auth.config.json"),
    `${JSON.stringify(authConfig, null, 2)}\n`,
    "utf8",
  );

  const bunRuntimeFunctions = manifest.filter((definition) => definition.runtime_environment === "bun");
  const v8RuntimeFunctions = manifest.filter((definition) => definition.runtime_environment !== "bun");
  const runtimeBundle = generateRuntimeBundle({
    functions: v8RuntimeFunctions,
    routes: httpRoutes,
  });
  await fs.writeFile(path.join(internalDir, "bundle.mjs"), runtimeBundle, "utf8");
  await fs.writeFile(
    path.join(internalDir, "bundle.sha256"),
    `${sha256Hex(runtimeBundle)}\n`,
    "utf8",
  );

  const bunProgramBundlePath = path.join(internalDir, "bun_program_bundle.js");
  const bunProgramBundleHashPath = path.join(internalDir, "bun_program_bundle.sha256");
  if (bunRuntimeFunctions.length > 0) {
    const bunProgramBundle = generateRuntimeProgramBundle({
      functions: bunRuntimeFunctions,
      routes: [],
    });
    await fs.writeFile(bunProgramBundlePath, bunProgramBundle, "utf8");
    await fs.writeFile(bunProgramBundleHashPath, `${sha256Hex(bunProgramBundle)}\n`, "utf8");
  } else {
    await fs.rm(bunProgramBundlePath, { force: true });
    await fs.rm(bunProgramBundleHashPath, { force: true });
  }

  return {
    appDir,
    httpRoutes,
    manifest,
    modules,
    nodeExternalPackageReport,
    projectConfig,
    schema,
    authConfig,
    sourceRoot: resolvedSourceRoot,
  };
}

async function runCliFromArgs(args = process.argv.slice(2), { onInfo } = {}) {
  const appDir = resolveAppDirectory(args);
  const debugNodeApis = args.includes("--debug-node-apis");
  const projectConfig = await loadProjectConfig(appDir);
  const sourceRoot = await tryResolveSourceRoot(appDir, {
    functionsOverride: projectConfig.functions,
  });
  const cloudFunctions = await generateCloudFunctionsArtifacts({ appDir, onInfo });

  if (sourceRoot?.detectedBothRoots) {
    onInfo?.(`Detected both nimbus/ and convex/ in ${appDir}; using nimbus/.`);
  }

  if (sourceRoot === null && cloudFunctions === null) {
    await resolveSourceRoot(appDir, { functionsOverride: projectConfig.functions });
  }

  const convex = sourceRoot
    ? await generateConvexArtifacts({ appDir, sourceRoot, projectConfig, debugNodeApis, onInfo })
    : null;
  return { appDir, cloudFunctions, convex };
}

export { generateConvexArtifacts, runCliFromArgs };
