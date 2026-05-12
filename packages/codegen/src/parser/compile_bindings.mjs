import { convexValidators } from "../schema.mjs";
import {
  isExternalPackageSpecifier,
  isNodeBuiltinSpecifier,
} from "../module_specifiers.mjs";

function createCompileBindings(source) {
  return collectKnownImportBindings(source, "compileValue");
}

function createRuntimeBindingDescriptors(source, { runtimeEnvironment = "default" } = {}) {
  const bindings = {
    ...collectKnownImportBindings(source, "runtimeDescriptor"),
  };
  if (runtimeEnvironment === "node") {
    Object.assign(
      bindings,
      collectNodeBuiltinRuntimeBindings(source),
      collectNodeExternalPackageRuntimeBindings(source),
    );
  }
  return bindings;
}

function collectKnownImportBindings(source, field) {
  const bindings = {};
  const importPattern = /import\s*\{([\s\S]*?)\}\s*from\s*["']([^"']+)["'];?/g;
  for (const match of source.matchAll(importPattern)) {
    const specifiers = match[1]
      .split(",")
      .map((specifier) => specifier.trim())
      .filter(Boolean);
    const sourcePath = match[2];
    for (const specifier of specifiers) {
      const [rawImportedName, rawLocalName = rawImportedName] = specifier
        .split(/\s+as\s+/)
        .map((part) => part.trim());
      const importedName = rawImportedName.replace(/^type\s+/, "").trim();
      const localName = rawLocalName.replace(/^type\s+/, "").trim();
      const binding = createKnownImportBindingRecord(sourcePath, importedName)?.[field];
      if (binding !== undefined) {
        bindings[localName] = binding;
      }
    }
  }
  return bindings;
}

function collectNodeBuiltinRuntimeBindings(source) {
  const bindings = {};
  const importPattern = /\bimport\s+([\s\S]*?)\s+from\s*["']([^"']+)["'];?/g;
  for (const match of source.matchAll(importPattern)) {
    const clause = match[1].trim();
    const rawSourcePath = match[2];
    if (!isNodeBuiltinSpecifier(rawSourcePath)) {
      continue;
    }
    const sourcePath = normalizeNodeBuiltinSpecifier(rawSourcePath);
    for (const binding of parseImportClauseBindings(clause)) {
      bindings[binding.localName] = {
        type: nodeRuntimeBindingType("node_builtin", binding.kind),
        specifier: sourcePath,
        ...(binding.kind === "named" ? { imported_name: binding.importedName } : {}),
      };
    }
  }
  return bindings;
}

function collectNodeExternalPackageRuntimeBindings(source) {
  const bindings = {};
  const importPattern = /\bimport\s+([\s\S]*?)\s+from\s*["']([^"']+)["'];?/g;
  for (const match of source.matchAll(importPattern)) {
    const clause = match[1].trim();
    const sourcePath = match[2];
    if (!isExternalPackageSpecifier(sourcePath)) {
      continue;
    }
    for (const binding of parseImportClauseBindings(clause)) {
      bindings[binding.localName] = {
        type: nodeRuntimeBindingType("node_external_package", binding.kind),
        specifier: sourcePath,
        ...(binding.kind === "named" ? { imported_name: binding.importedName } : {}),
      };
    }
  }
  return bindings;
}

function nodeRuntimeBindingType(prefix, kind) {
  if (kind === "named") {
    return `${prefix}_named`;
  }
  if (kind === "default") {
    return `${prefix}_default`;
  }
  return `${prefix}_namespace`;
}

function parseImportClauseBindings(clause) {
  if (clause.startsWith("*")) {
    const match = /^\*\s+as\s+([A-Za-z_$][\w$]*)$/.exec(clause);
    return match ? [{ kind: "namespace", localName: match[1] }] : [];
  }

  const bindings = [];
  const namedStart = clause.indexOf("{");
  if (namedStart > 0) {
    const defaultLocal = clause.slice(0, namedStart).replace(/,$/, "").trim();
    if (defaultLocal) {
      bindings.push({ kind: "default", localName: defaultLocal });
    }
  } else if (namedStart === -1 && clause) {
    bindings.push({ kind: "default", localName: clause });
  }

  if (namedStart !== -1) {
    const namedEnd = clause.lastIndexOf("}");
    const namedClause = namedEnd === -1 ? "" : clause.slice(namedStart + 1, namedEnd);
    for (const specifier of namedClause.split(",").map((part) => part.trim()).filter(Boolean)) {
      const [importedName, localName = importedName] = specifier
        .split(/\s+as\s+/)
        .map((part) => part.trim());
      bindings.push({
        kind: "named",
        importedName,
        localName,
      });
    }
  }
  return bindings;
}

function normalizeNodeBuiltinSpecifier(specifier) {
  return specifier.startsWith("node:") ? specifier : `node:${specifier}`;
}

function createKnownImportBindingRecord(sourcePath, importedName) {
  if (sourcePath === "convex/server" || sourcePath === "nimbus/server") {
    if (importedName === "paginationOptsValidator") {
      return { compileValue: createPaginationOptionsValidator() };
    }
    if (importedName === "paginationResultValidator") {
      return { compileValue: createPaginationResultValidator };
    }
  }
  if (sourcePath.endsWith("/_generated/api")) {
    if (importedName === "api") {
      return createGeneratedReferenceBinding({ visibility: "public" });
    }
    if (importedName === "internal") {
      return createGeneratedReferenceBinding({ visibility: "internal" });
    }
  }
  if (sourcePath.endsWith("/_generated/scheduled_functions")) {
    if (importedName === "scheduledFunctions") {
      return createGeneratedReferenceBinding({
        visibility: "public",
        kind: "mutation",
      });
    }
    if (importedName === "internalScheduledFunctions") {
      return createGeneratedReferenceBinding({
        visibility: "internal",
        kind: "mutation",
      });
    }
  }
  return undefined;
}

function createGeneratedReferenceBinding(config) {
  return {
    compileValue: createGeneratedReferenceTree(config),
    runtimeDescriptor: {
      type: "generated_reference_tree",
      visibility: config.visibility,
      ...(config.kind === undefined ? {} : { reference_kind: config.kind }),
    },
  };
}

function createPaginationOptionsValidator() {
  return convexValidators.object({
    numItems: convexValidators.number(),
    cursor: convexValidators.union(convexValidators.string(), convexValidators.null()),
    endCursor: convexValidators.optional(
      convexValidators.union(convexValidators.string(), convexValidators.null()),
    ),
    id: convexValidators.optional(convexValidators.number()),
    maximumRowsRead: convexValidators.optional(convexValidators.number()),
    maximumBytesRead: convexValidators.optional(convexValidators.number()),
  });
}

function createPaginationResultValidator(itemValidator) {
  return convexValidators.object({
    page: convexValidators.array(itemValidator),
    continueCursor: convexValidators.string(),
    isDone: convexValidators.boolean(),
    splitCursor: convexValidators.optional(
      convexValidators.union(convexValidators.string(), convexValidators.null()),
    ),
    pageStatus: convexValidators.optional(
      convexValidators.union(
        convexValidators.literal("SplitRecommended"),
        convexValidators.literal("SplitRequired"),
        convexValidators.null(),
      ),
    ),
  });
}

function createGeneratedReferenceTree(config, pathParts = []) {
  return new Proxy(
    {},
    {
      get(_target, property) {
        if (property === "kind" && pathParts.length > 0 && config.kind !== undefined) {
          return config.kind;
        }
        if (property === "name" && pathParts.length > 0) {
          return referenceNameFromPath(pathParts);
        }
        if (property === "visibility" && pathParts.length > 0) {
          return config.visibility;
        }
        if (typeof property === "symbol") {
          return undefined;
        }
        return createGeneratedReferenceTree(config, [
          ...pathParts,
          String(property),
        ]);
      },
    },
  );
}

function referenceNameFromPath(pathParts) {
  return pathParts.length > 1
    ? `${pathParts.slice(0, -1).join(".")}:${pathParts.at(-1)}`
    : pathParts[0];
}

export { createCompileBindings, createRuntimeBindingDescriptors };
