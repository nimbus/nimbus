import fs from "node:fs/promises";
import path from "node:path";

import { extractFunctionDefinitions } from "./parser/function_definitions.mjs";
import {
  createCompileBindings,
  createRuntimeBindingDescriptors,
  ensureSupportedSource,
} from "./parser/helpers.mjs";
import { parseHttpRoutes } from "./parser/http_routes.mjs";
import { collectNodeApiUsages } from "./node_api_diagnostics.mjs";

async function parseModule(convexDir, filePath, schema, { debugNodeApis = false } = {}) {
  const source = await fs.readFile(filePath, "utf8");
  ensureSupportedSource(filePath, source);
  const runtimeEnvironment = detectModuleRuntimeEnvironment(source);
  const compileBindings = createCompileBindings(source);
  const runtimeBindings = createRuntimeBindingDescriptors(source, { runtimeEnvironment });
  validateNodeBuiltinUsage(filePath, source, runtimeEnvironment, debugNodeApis);

  const relativePath = path.relative(convexDir, filePath).replaceAll(path.sep, "/");
  const moduleName = relativePath.replace(/\.(tsx|ts)$/, "").replaceAll("/", ".");
  const parsedFunctions = await extractFunctionDefinitions(
    source,
    filePath,
    moduleName,
    schema,
    compileBindings,
    runtimeBindings,
  );
  const functions = parsedFunctions.map((fn) => ({
    ...fn,
    runtimeEnvironment,
  }));
  validateRuntimeEnvironment(filePath, runtimeEnvironment, functions);

  return { filePath, moduleName, source, runtimeEnvironment, functions };
}

function detectModuleRuntimeEnvironment(source) {
  const normalized = source.replace(/^\uFEFF/, "");
  const withoutLeadingTrivia = normalized.replace(
    /^(?:\s|\/\/[^\n\r]*(?:\r?\n|$)|\/\*[\s\S]*?\*\/)*/,
    "",
  );
  return /^["']use node["'];?/.test(withoutLeadingTrivia) ? "node" : "default";
}

function validateRuntimeEnvironment(filePath, runtimeEnvironment, functions) {
  if (runtimeEnvironment !== "node") {
    return;
  }
  const unsupported = functions.filter((fn) => fn.kind !== "action");
  if (unsupported.length === 0) {
    return;
  }
  const names = unsupported.map((fn) => `${fn.kind} ${fn.exportName}`).join(", ");
  throw new Error(
    `${path.relative(process.cwd(), filePath)} uses "use node", but the Node.js runtime is only supported for action functions. Move ${names} to a default-runtime module.`,
  );
}

function validateNodeBuiltinUsage(filePath, source, runtimeEnvironment, debugNodeApis) {
  if (runtimeEnvironment === "node" || debugNodeApis) {
    return;
  }
  const usages = collectNodeApiUsages(source);
  if (usages.length === 0) {
    return;
  }
  const specifiers = [...new Set(usages.map((usage) => usage.specifier))].join(", ");
  throw new Error(
    `${path.relative(process.cwd(), filePath)} imports Node.js builtin module(s) ${specifiers}. Add "use node" at the top of an action-only module, or rerun with --debug-node-apis for diagnostic details.`,
  );
}

export { parseHttpRoutes, parseModule };
