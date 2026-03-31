import fs from "node:fs/promises";
import path from "node:path";

import { collectModuleFiles, resolveAppDirectory, sha256Hex } from "./app.mjs";
import { loadAuthConfig } from "./auth_config.mjs";
import { generateApiFile, generateDataModelFile, generateScheduledFunctionsFile, generateServerFile } from "./emit/generated_files.mjs";
import { generateRuntimeBundle } from "./emit/runtime_bundle.mjs";
import { parseHttpRoutes, parseModule } from "./parser.mjs";
import { loadSchemaDefinition } from "./schema.mjs";

async function generateConvexArtifacts({ appDir }) {
  const convexDir = path.join(appDir, "convex");
  const generatedDir = path.join(convexDir, "_generated");
  const internalDir = path.join(appDir, ".neovex", "convex");
  const schema = await loadSchemaDefinition(convexDir);
  const authConfig = await loadAuthConfig(convexDir);

  const moduleFiles = await collectModuleFiles(convexDir);
  const modules = [];
  const manifest = [];

  for (const filePath of moduleFiles) {
    const moduleInfo = await parseModule(convexDir, filePath, schema);
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
      });
    }
  }

  const httpRoutes = await parseHttpRoutes(convexDir, schema, modules);

  await fs.mkdir(generatedDir, { recursive: true });
  await fs.mkdir(internalDir, { recursive: true });
  await fs.writeFile(
    path.join(generatedDir, "api.ts"),
    generateApiFile(modules, schema),
    "utf8",
  );
  await fs.writeFile(
    path.join(generatedDir, "server.ts"),
    generateServerFile(),
    "utf8",
  );
  await fs.writeFile(
    path.join(generatedDir, "scheduled_functions.ts"),
    generateScheduledFunctionsFile(modules, schema),
    "utf8",
  );
  await fs.writeFile(
    path.join(generatedDir, "dataModel.d.ts"),
    generateDataModelFile(schema),
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
  };
}

async function runCliFromArgs(args = process.argv.slice(2)) {
  return generateConvexArtifacts({
    appDir: resolveAppDirectory(args),
  });
}

export { generateConvexArtifacts, runCliFromArgs };
