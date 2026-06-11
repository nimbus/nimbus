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
import { generateApiFile, generateDataModelFile, generateScheduledFunctionsFile, generateServerFile } from "./emit/generated_files.mjs";
import {
  generateRuntimeBundle,
  generateRuntimeProgramBundle,
} from "./emit/runtime_bundle.mjs";
import {
  collectNodeApiDiagnostics,
  formatNodeApiDiagnostics,
} from "./node_api_diagnostics.mjs";
import { assertTenantBundleAdmission } from "./module_specifiers.mjs";
import {
  createNodeExternalPackageReport,
  stageNodeExternalPackages,
} from "./node_external_packages.mjs";
import { parseHttpRoutes, parseModule } from "./parser.mjs";
import { loadProjectConfig } from "./project_config.mjs";
import {
  runtimeLaneMetadata,
  runtimeMetadataForFunction,
} from "./runtime_metadata.mjs";
import { loadSchemaDefinition } from "./schema.mjs";

async function generateConvexArtifacts({ appDir, sourceRoot, debugNodeApis = false, onInfo } = {}) {
  const resolvedSourceRoot = sourceRoot ?? await resolveSourceRoot(appDir);
  const sourceDir = resolvedSourceRoot.sourceDirPath;
  const packageNamespace = resolvedSourceRoot.packageNamespace;
  const generatedDir = path.join(sourceDir, "_generated");
  const internalDir = path.join(appDir, ".nimbus", "convex");
  const projectConfig = await loadProjectConfig(appDir);
  const schema = await loadSchemaDefinition(sourceDir);
  const authConfig = await loadAuthConfig(sourceDir);

  const moduleFiles = await collectModuleFiles(sourceDir);
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
  const sourceRoot = await tryResolveSourceRoot(appDir);
  const cloudFunctions = await generateCloudFunctionsArtifacts({ appDir, onInfo });

  if (sourceRoot?.detectedBothRoots) {
    onInfo?.(`Detected both nimbus/ and convex/ in ${appDir}; using nimbus/.`);
  }

  if (sourceRoot === null && cloudFunctions === null) {
    await resolveSourceRoot(appDir);
  }

  const convex = sourceRoot
    ? await generateConvexArtifacts({ appDir, sourceRoot, debugNodeApis, onInfo })
    : null;
  return { appDir, cloudFunctions, convex };
}

export { generateConvexArtifacts, runCliFromArgs };
