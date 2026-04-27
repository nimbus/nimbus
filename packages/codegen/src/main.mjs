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
import { generateRuntimeBundle } from "./emit/runtime_bundle.mjs";
import { parseHttpRoutes, parseModule } from "./parser.mjs";
import { loadSchemaDefinition } from "./schema.mjs";

async function generateConvexArtifacts({ appDir, sourceRoot }) {
  const resolvedSourceRoot = sourceRoot ?? await resolveSourceRoot(appDir);
  const sourceDir = resolvedSourceRoot.sourceDirPath;
  const packageNamespace = resolvedSourceRoot.packageNamespace;
  const generatedDir = path.join(sourceDir, "_generated");
  const internalDir = path.join(appDir, ".neovex", "convex");
  const schema = await loadSchemaDefinition(sourceDir);
  const authConfig = await loadAuthConfig(sourceDir);

  const moduleFiles = await collectModuleFiles(sourceDir);
  const modules = [];
  const manifest = [];

  for (const filePath of moduleFiles) {
    const moduleInfo = await parseModule(sourceDir, filePath, schema);
    modules.push(moduleInfo);
    for (const fn of moduleInfo.functions) {
      if (fn.kind === "http_action") {
        continue;
      }
      manifest.push({
        name: fn.name,
        export: fn.exportName,
        module: moduleInfo.moduleName,
        kind: fn.kind,
        visibility: fn.visibility,
        schedulable: fn.kind === "mutation",
        plan: fn.plan,
        runtime_handler: fn.runtimeHandler ?? null,
        runtime_bindings: fn.runtimeHandler ? (fn.runtimeBindings ?? {}) : undefined,
      });
    }
  }

  const httpRoutes = await parseHttpRoutes(sourceDir, schema, modules);

  await fs.mkdir(generatedDir, { recursive: true });
  await fs.mkdir(internalDir, { recursive: true });
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
    `${JSON.stringify({ functions: manifest }, null, 2)}\n`,
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

  const runtimeBundle = generateRuntimeBundle({
    functions: manifest,
    routes: httpRoutes,
  });
  await fs.writeFile(path.join(internalDir, "bundle.mjs"), runtimeBundle, "utf8");
  await fs.writeFile(
    path.join(internalDir, "bundle.sha256"),
    `${sha256Hex(runtimeBundle)}\n`,
    "utf8",
  );

  return {
    appDir,
    httpRoutes,
    manifest,
    modules,
    schema,
    authConfig,
    sourceRoot: resolvedSourceRoot,
  };
}

async function runCliFromArgs(args = process.argv.slice(2), { onInfo } = {}) {
  const appDir = resolveAppDirectory(args);
  const sourceRoot = await tryResolveSourceRoot(appDir);
  const cloudFunctions = await generateCloudFunctionsArtifacts({ appDir, onInfo });

  if (sourceRoot?.detectedBothRoots) {
    onInfo?.(`Detected both neovex/ and convex/ in ${appDir}; using neovex/.`);
  }

  if (sourceRoot === null && cloudFunctions === null) {
    await resolveSourceRoot(appDir);
  }

  const convex = sourceRoot
    ? await generateConvexArtifacts({ appDir, sourceRoot })
    : null;
  return { appDir, cloudFunctions, convex };
}

export { generateConvexArtifacts, runCliFromArgs };
