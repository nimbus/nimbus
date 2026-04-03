import { convexValidators } from "../schema.mjs";

function createCompileBindings(source) {
  return collectKnownImportBindings(source, "compileValue");
}

function createRuntimeBindingDescriptors(source) {
  return collectKnownImportBindings(source, "runtimeDescriptor");
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

function createKnownImportBindingRecord(sourcePath, importedName) {
  if (sourcePath === "convex/server" || sourcePath === "neovex/server") {
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
