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
import { collectModuleSpecifiers } from "./module_specifiers.mjs";

function moduleNameFromRelativePath(relativePath) {
  return relativePath.replace(/\.(tsx|ts)$/, "").replaceAll("/", ".");
}

async function parseModule(convexDir, filePath, schema, { debugNodeApis = false } = {}) {
  const source = await fs.readFile(filePath, "utf8");
  ensureSupportedSource(filePath, source);
  const runtimeEnvironment = detectModuleRuntimeEnvironment(source);
  const compileBindings = createCompileBindings(source);
  const runtimeBindings = createRuntimeBindingDescriptors(source, { runtimeEnvironment });
  validateNodeBuiltinUsage(filePath, source, runtimeEnvironment, debugNodeApis);

  const relativePath = path.relative(convexDir, filePath).replaceAll(path.sep, "/");
  const moduleName = moduleNameFromRelativePath(relativePath);
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
  if (/^["']use node["'];?/.test(withoutLeadingTrivia)) {
    return "node";
  }
  if (/^["']use bun["'];?/.test(withoutLeadingTrivia)) {
    return "bun";
  }
  return "default";
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

// A "use node" module and a default-runtime module execute on different V8
// runtime lanes (see emit/runtime_bundle_preamble.mjs) and cannot share
// module state — a static/dynamic import or require() that reaches directly
// across that boundary is a codegen-time mistake, not a supported pattern.
// The supported way to call a "use node" action from elsewhere is through the
// generated API reference (ctx.runAction(internal.<module>.<name>, ...)),
// which crosses the boundary through the engine's mutation/action path
// instead of a JS module import.
//
// This runs as a lexical pre-pass over every module file, before the full
// parseModule/extractFunctionDefinitions pass, so that both a *used* import
// (which would otherwise surface much later as an opaque "unsupported export
// shape" error out of the compile-time plan resolver) and an *unused* one
// (which would otherwise codegen silently) get the same clear, purpose-built
// error, naming the offending import.
async function validateCrossModuleRuntimeImports(sourceDir, moduleFilePaths) {
  const descriptorsByFilePath = new Map();
  for (const filePath of moduleFilePaths) {
    const source = await fs.readFile(filePath, "utf8");
    const relativePath = path.relative(sourceDir, filePath).replaceAll(path.sep, "/");
    descriptorsByFilePath.set(filePath, {
      filePath,
      moduleName: moduleNameFromRelativePath(relativePath),
      runtimeEnvironment: detectModuleRuntimeEnvironment(source),
      source,
    });
  }
  for (const moduleInfo of descriptorsByFilePath.values()) {
    if (moduleInfo.runtimeEnvironment === "node") {
      continue;
    }
    for (const specifier of collectRelativeValueImportSpecifiers(moduleInfo.source)) {
      const target = resolveRelativeModuleImport(moduleInfo.filePath, specifier, descriptorsByFilePath);
      if (target !== null && target.runtimeEnvironment === "node") {
        throw new Error(
          `${path.relative(process.cwd(), moduleInfo.filePath)} imports "${specifier}" from ` +
            `${path.relative(process.cwd(), target.filePath)}, but that module begins with "use node". ` +
            `A default-runtime module cannot import a "use node" module directly -- they execute on ` +
            `separate runtimes and cannot share module state. Call the action through the generated API ` +
            `reference instead, e.g. ctx.runAction(internal.${target.moduleName}.<exportName>, args).`,
        );
      }
    }
  }
}

// Matches import/require specifiers the same way node_api_diagnostics.mjs's
// collectNodeApiUsages does (via the shared collectModuleSpecifiers regex
// set), but keeps only relative (local file) specifiers, and excludes
// whole-statement `import type ...` / `export type ... from` forms: those are
// erased by the TypeScript compiler and never reach the runtime bundle, so
// they carry no cross-runtime hazard.
function collectRelativeValueImportSpecifiers(source) {
  const specifiers = [];
  for (const { specifier } of collectModuleSpecifiers(source)) {
    if (!specifier.startsWith(".")) {
      continue;
    }
    specifiers.push(specifier);
  }
  const typeOnlySpecifiers = new Set();
  const typeOnlyPatterns = [
    /\bimport\s+type\s+[^"'()]*?\s+from\s+["']([^"']+)["']/g,
    /\bexport\s+type\s+[^"'()]*?\s+from\s+["']([^"']+)["']/g,
  ];
  for (const regex of typeOnlyPatterns) {
    for (const match of source.matchAll(regex)) {
      typeOnlySpecifiers.add(match[1]);
    }
  }
  return [...new Set(specifiers)].filter((specifier) => !typeOnlySpecifiers.has(specifier));
}

const RELATIVE_MODULE_EXTENSION_CANDIDATES = [".ts", ".tsx"];

function resolveRelativeModuleImport(fromFilePath, specifier, descriptorsByFilePath) {
  const resolvedBase = path.resolve(path.dirname(fromFilePath), specifier);
  const candidates = [
    resolvedBase,
    ...RELATIVE_MODULE_EXTENSION_CANDIDATES.map((extension) => `${resolvedBase}${extension}`),
    ...RELATIVE_MODULE_EXTENSION_CANDIDATES.map((extension) => path.join(resolvedBase, `index${extension}`)),
  ];
  for (const candidate of candidates) {
    const moduleInfo = descriptorsByFilePath.get(candidate);
    if (moduleInfo) {
      return moduleInfo;
    }
  }
  return null;
}

export { parseHttpRoutes, parseModule, validateCrossModuleRuntimeImports };
