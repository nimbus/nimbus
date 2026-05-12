import { buildRuntimeBundleSource } from "./runtime_bundle_parts.mjs";

function generateRuntimeBundle(manifest) {
  const { importPreamble } = collectNodeRuntimeImports(manifest);
  return buildRuntimeBundleSource(JSON.stringify(manifest, null, 2), importPreamble);
}

function collectNodeRuntimeImports(manifest) {
  const builtinSpecifiers = new Set();
  const externalPackageSpecifiers = new Set();
  for (const fn of manifest.functions ?? []) {
    collectNodeRuntimeDescriptors(fn.runtime_bindings, {
      builtinSpecifiers,
      externalPackageSpecifiers,
    });
  }
  return {
    importPreamble: [
      ...createImportMapPreamble({
        importNamePrefix: "__neovexNodeBuiltin",
        mapName: "__neovexNodeBuiltinModules",
        specifiers: builtinSpecifiers,
      }),
      ...createImportMapPreamble({
        importNamePrefix: "__neovexNodeExternalPackage",
        mapName: "__neovexNodeExternalPackages",
        specifiers: externalPackageSpecifiers,
      }),
    ].join("\n"),
  };
}

function createImportMapPreamble({ importNamePrefix, mapName, specifiers }) {
  const sorted = [...specifiers].sort();
  const importNames = new Map(sorted.map((specifier, index) => [specifier, `${importNamePrefix}${index}`]));
  const imports = sorted.map((specifier) => `import * as ${importNames.get(specifier)} from ${JSON.stringify(specifier)};`);
  const entries = sorted.map((specifier) => `[${JSON.stringify(specifier)}, ${importNames.get(specifier)}]`);
  const bindings = entries.length === 0
    ? `const ${mapName} = new Map();`
    : `const ${mapName} = new Map([\n  ${entries.join(",\n  ")}\n]);`;
  return [...imports, bindings];
}

function collectNodeRuntimeDescriptors(value, { builtinSpecifiers, externalPackageSpecifiers }) {
  if (value === null || typeof value !== "object") {
    return;
  }
  if (
    (
      value.type === "node_builtin_default"
      || value.type === "node_builtin_namespace"
      || value.type === "node_builtin_named"
    )
    && typeof value.specifier === "string"
  ) {
    builtinSpecifiers.add(value.specifier);
    return;
  }
  if (
    (
      value.type === "node_external_package_default"
      || value.type === "node_external_package_namespace"
      || value.type === "node_external_package_named"
    )
    && typeof value.specifier === "string"
  ) {
    externalPackageSpecifiers.add(value.specifier);
    return;
  }
  for (const child of Object.values(value)) {
    collectNodeRuntimeDescriptors(child, { builtinSpecifiers, externalPackageSpecifiers });
  }
}

export { generateRuntimeBundle };
