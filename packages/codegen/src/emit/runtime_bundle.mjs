import { buildRuntimeBundleSource } from "./runtime_bundle_parts.mjs";

// The generated bundle resolves Node builtin/external-package bindings lazily,
// via dynamic import() inside compileRuntimeHandler (see
// runtime_bundle_preamble.mjs), rather than static top-level imports. This
// matters because crates/nimbus-convex loads exactly one bundle.mjs per app
// and shares it across every V8-based runtime lane (the default web-standard
// isolate and every node* lane alike) — a static top-level `import ... from
// "node:x"` emitted for one function's Node dependency would fail module
// linking for the whole bundle, including default-runtime functions that
// never touch Node builtins at all. Lazy resolution means a function only
// ever triggers resolution of the specifiers it actually uses.
function generateRuntimeBundle(manifest) {
  return buildRuntimeBundleSource(JSON.stringify(manifest, null, 2), {
    module: true,
  });
}

// The Bun/JSC program bundle is a flat, non-module script (no import/export
// statements, see the `module: false` option below), so it cannot rely on
// dynamic import() the way the default module bundle does; Node runtime
// imports remain unsupported there and are rejected loudly at codegen time.
function generateRuntimeProgramBundle(manifest) {
  const importSpecifiers = collectNodeRuntimeSpecifiers(manifest);
  if (importSpecifiers.length > 0) {
    throw new Error(
      `runtime program bundle cannot materialize Node runtime imports: ${importSpecifiers.join(", ")}`,
    );
  }
  return buildRuntimeBundleSource(JSON.stringify(manifest, null, 2), {
    module: false,
  });
}

function collectNodeRuntimeSpecifiers(manifest) {
  const builtinSpecifiers = new Set();
  const externalPackageSpecifiers = new Set();
  for (const fn of manifest.functions ?? []) {
    collectNodeRuntimeDescriptors(fn.runtime_bindings, {
      builtinSpecifiers,
      externalPackageSpecifiers,
    });
  }
  return [...[...builtinSpecifiers].sort(), ...[...externalPackageSpecifiers].sort()];
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

export { generateRuntimeBundle, generateRuntimeProgramBundle };
