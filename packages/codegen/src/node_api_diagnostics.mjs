import path from "node:path";

import {
  canonicalNodeSpecifier,
  collectModuleSpecifiers,
  isNodeBuiltinSpecifier,
} from "./module_specifiers.mjs";

function collectNodeApiDiagnostics(modules, sourceDir) {
  const diagnostics = [];
  for (const moduleInfo of modules) {
    if (moduleInfo.runtimeEnvironment === "node") {
      continue;
    }
    const usages = collectNodeApiUsages(moduleInfo.source);
    if (usages.length === 0) {
      continue;
    }
    diagnostics.push({
      file: path.relative(sourceDir, moduleInfo.filePath).replaceAll(path.sep, "/"),
      module: moduleInfo.moduleName,
      usages,
    });
  }
  return diagnostics;
}

function collectNodeApiUsages(source) {
  const usages = [];
  for (const { kind, specifier } of collectModuleSpecifiers(source)) {
    if (isNodeBuiltinSpecifier(specifier)) {
      usages.push({
        kind,
        specifier,
        canonical: canonicalNodeSpecifier(specifier),
      });
    }
  }
  return usages;
}

function formatNodeApiDiagnostics(diagnostics) {
  if (diagnostics.length === 0) {
    return "No Node.js builtin API usage was found in default-runtime Convex modules.";
  }

  const lines = [
    "Node.js builtin API usage was found in Convex modules that do not opt into the Node.js runtime.",
    "Add \"use node\" at the top of action-only modules, or move the Node-specific import behind an action module.",
  ];
  for (const diagnostic of diagnostics) {
    lines.push(`- ${diagnostic.file} (${diagnostic.module})`);
    for (const usage of diagnostic.usages) {
      lines.push(`  ${usage.kind}: ${usage.specifier} (canonical: ${usage.canonical})`);
    }
  }
  return lines.join("\n");
}

export {
  collectNodeApiDiagnostics,
  collectNodeApiUsages,
  formatNodeApiDiagnostics,
};
