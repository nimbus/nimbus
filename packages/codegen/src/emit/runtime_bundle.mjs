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
//
// That cross-lane risk only exists when the bundle is genuinely mixed: some
// runtime-bearing surfaces on the default lane, others on the node lane,
// sharing one bundle.mjs. A bundle whose surfaces are ALL node-lane has no
// such risk — every function invoked against it already requires the node
// lane, so the bundle is never loaded anywhere else. Functions are not the
// only surface that counts: HTTP routes (convex/http.ts httpActions) always
// execute on the default web-standard lane, so a manifest with any route at
// all is loaded by a web isolate even when every function is "use node" —
// see isSingleRuntimeNodeManifest below. For the all-node case we also emit bare
// top-level imports of every Node binding specifier the manifest uses,
// purely for their load-time side effects (see collectEagerNodeRuntimeImports
// below): ES module semantics evaluate that module graph once, before the
// rest of the bundle runs, and the later dynamic import() of the same
// specifier inside compileRuntimeHandler (at first invocation) resolves from
// that same cached module record instead of re-running its top-level code.
// This restores "a package that throws or captures state at init does so at
// deploy" for single-runtime Node bundles, while leaving genuinely mixed
// bundles on the fully lazy path.
function generateRuntimeBundle(manifest) {
  const bundleSource = buildRuntimeBundleSource(
    JSON.stringify(manifest, null, 2),
    {
      module: true,
    },
  );
  const eagerImports = collectEagerNodeRuntimeImports(manifest);
  if (eagerImports.length === 0) {
    return bundleSource;
  }
  const importStatements = eagerImports
    .map((specifier) => `import ${JSON.stringify(specifier)};`)
    .join("\n");
  return `${importStatements}\n${bundleSource}`;
}

function collectEagerNodeRuntimeImports(manifest) {
  return isSingleRuntimeNodeManifest(manifest)
    ? collectNodeRuntimeSpecifiers(manifest)
    : [];
}

// "Single-runtime Node" means every runtime-bearing surface of the manifest
// loads on the node lane. HTTP routes carry no runtime_environment because
// httpActions always run on the default web-standard runtime, so any route
// makes the bundle load in a web isolate — where an eager top-level
// `import "node:*"` would fail module linking for the whole bundle.
function isSingleRuntimeNodeManifest(manifest) {
  const functions = manifest.functions ?? [];
  const routes = manifest.routes ?? [];
  return (
    functions.length > 0 &&
    functions.every((fn) => fn.runtime_environment === "node") &&
    routes.length === 0
  );
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
    inlineRuntimeHandlerFactories: buildInlineRuntimeHandlerFactories(manifest),
  });
}

function buildInlineRuntimeHandlerFactories(manifest) {
  return (manifest.functions ?? [])
    .filter(
      (definition) =>
        typeof definition.runtime_handler === "string" &&
        definition.runtime_handler.length > 0,
    )
    .map((definition) => {
      const bindingNames = Object.keys(definition.runtime_bindings ?? {});
      for (const name of bindingNames) {
        if (!/^[A-Za-z_$][A-Za-z0-9_$]*$/.test(name)) {
          throw new Error(
            `runtime program bundle binding is not a JavaScript identifier: ${name}`,
          );
        }
      }
      const factoryParameters = bindingNames.join(", ");
      return `[${JSON.stringify(definition.name)}, async function(definition) {
  const runtimeBindings = await materializeRuntimeBindings(definition.runtime_bindings);
  const bindingValues = ${JSON.stringify(bindingNames)}.map((name) => runtimeBindings[name]);
  const invoke = ((${factoryParameters}) => (ctx, args) =>
    (${definition.runtime_handler})(ctx, args)
  )(...bindingValues);
  const handlerOrigin = {
    module: typeof definition.module === "string" ? definition.module : null,
    line:
      typeof definition.runtime_handler_line === "number"
        ? definition.runtime_handler_line
        : null,
  };
  return (ctx, args) =>
    nimbusWrapRuntimeInvoke(invoke, [], ctx, args, handlerOrigin);
}]`;
    })
    .join(",\n");
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
  return [
    ...[...builtinSpecifiers].sort(),
    ...[...externalPackageSpecifiers].sort(),
  ];
}

function collectNodeRuntimeDescriptors(
  value,
  { builtinSpecifiers, externalPackageSpecifiers },
) {
  if (value === null || typeof value !== "object") {
    return;
  }
  if (
    (value.type === "node_builtin_default" ||
      value.type === "node_builtin_namespace" ||
      value.type === "node_builtin_named") &&
    typeof value.specifier === "string"
  ) {
    builtinSpecifiers.add(value.specifier);
    return;
  }
  if (
    (value.type === "node_external_package_default" ||
      value.type === "node_external_package_namespace" ||
      value.type === "node_external_package_named") &&
    typeof value.specifier === "string"
  ) {
    externalPackageSpecifiers.add(value.specifier);
    return;
  }
  for (const child of Object.values(value)) {
    collectNodeRuntimeDescriptors(child, {
      builtinSpecifiers,
      externalPackageSpecifiers,
    });
  }
}

export { generateRuntimeBundle, generateRuntimeProgramBundle };
