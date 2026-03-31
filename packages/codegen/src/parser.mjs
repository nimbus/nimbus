import fs from "node:fs/promises";
import path from "node:path";

import { extractFunctionDefinitions } from "./parser/function_definitions.mjs";
import { createCompileBindings, ensureSupportedSource } from "./parser/helpers.mjs";
import { parseHttpRoutes } from "./parser/http_routes.mjs";

async function parseModule(convexDir, filePath, schema) {
  const source = await fs.readFile(filePath, "utf8");
  ensureSupportedSource(filePath, source);
  const compileBindings = createCompileBindings(source);

  const relativePath = path.relative(convexDir, filePath).replaceAll(path.sep, "/");
  const moduleName = relativePath.replace(/\.(tsx|ts)$/, "").replaceAll("/", ".");
  const functions = await extractFunctionDefinitions(
    source,
    filePath,
    moduleName,
    schema,
    compileBindings,
  );

  return { filePath, moduleName, functions };
}

export { parseHttpRoutes, parseModule };
