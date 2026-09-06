use std::path::Path;
use std::rc::Rc;

use deno_ast::MediaType;
use deno_ast::ParseParams;
use deno_ast::SourceMapOption;

use crate::backends::v8::embedder::{JsErrorBox, ModuleCodeString, ModuleName, SourceMapData};
use crate::limits::RuntimeCompatibilityTarget;

type RuntimeExtensionTranspiler =
    dyn Fn(
        ModuleName,
        ModuleCodeString,
    ) -> Result<(ModuleCodeString, Option<SourceMapData>), JsErrorBox>;

deno_error::js_error_wrapper!(deno_ast::ParseDiagnostic, JsParseDiagnostic, "Error");
deno_error::js_error_wrapper!(deno_ast::TranspileError, JsTranspileError, "Error");

const NODE_EXTENSION_INTERNAL_DENO_PRELUDE_HEADER: &str = r#"
import { core as __nimbusCore } from "ext:core/mod.js";
import {
  denoGlobals as __nimbusInternalDenoGlobals,
  nodeGlobals as __nimbusInternalNodeGlobals,
  publicDenoPrototype as __nimbusPublicDenoPrototype,
} from "ext:nimbus_node22/internal_bootstrap.js";
"#;

const NODE_EXTENSION_INTERNAL_DENO_SCRIPT_PRELUDE_HEADER: &str = r#"
const __nimbusBootstrap = typeof __bootstrap !== "undefined" ? __bootstrap : globalThis.__bootstrap;
const __nimbusCore = __nimbusBootstrap.core;
const __nimbusInternalDenoGlobals =
  globalThis.__nimbusHiddenDenoGlobals ??
  __nimbusBootstrap.ext_node_denoGlobals ??
  globalThis.Object.create(null);
const __nimbusInternalNodeGlobals =
  globalThis.__nimbusHiddenNodeGlobals ??
  __nimbusBootstrap.ext_node_nodeGlobals ??
  globalThis.Object.create(null);
const __nimbusPublicDenoPrototype = globalThis.Deno ?? null;
"#;

const NODE_EXTENSION_INTERNAL_DENO_PRELUDE_BODY: &str = r#"
function __nimbusResolveDeno() {
  const deno = globalThis.__nimbusHiddenDenoGlobals ?? __nimbusInternalDenoGlobals;
  if (deno.core === undefined) {
    deno.core = __nimbusCore;
  }
  if (deno.build === undefined && __nimbusCore.build !== undefined) {
    deno.build = __nimbusCore.build;
  }
  if (deno.args === undefined) {
    deno.args = [];
  }
  if (deno.cwd === undefined) {
    deno.cwd = () => globalThis.process?.cwd?.() ?? "/";
  }
  if (deno.env === undefined) {
    deno.env = {
      get(name) {
        const operation = __nimbusCore.ops?.op_nimbus_runtime_env_get;
        if (typeof operation !== "function") {
          return undefined;
        }
        const result = operation(String(name));
        if (result?.status === "allowed") {
          return result.value;
        }
        if (result?.status === "missing" || result?.status === "denied") {
          return undefined;
        }
        throw new Error(result?.message ?? `runtime env capability denied for ${name}`);
      },
      toObject() {
        const operation = __nimbusCore.ops?.op_nimbus_runtime_env_snapshot;
        return typeof operation === "function" ? operation() : {};
      },
      set(name, value) {
        const processEnv = globalThis.process?.env;
        const marker = globalThis.Symbol.for("nimbus.processEnvProxy");
        if (processEnv?.[marker] === true) {
          processEnv[String(name)] = String(value);
        }
      },
      delete(name) {
        const processEnv = globalThis.process?.env;
        const marker = globalThis.Symbol.for("nimbus.processEnvProxy");
        if (processEnv?.[marker] === true) {
          delete globalThis.process.env[String(name)];
        }
      },
    };
  }
  if (deno.execPath === undefined) {
    deno.execPath = () => __nimbusCore.ops.op_nimbus_runtime_exec_path();
  }
  if (deno.version === undefined) {
    deno.version = __nimbusCore.ops.op_nimbus_runtime_versions();
  }
  if (
    __nimbusPublicDenoPrototype &&
    (typeof __nimbusPublicDenoPrototype === "object" ||
      typeof __nimbusPublicDenoPrototype === "function") &&
    deno.__proto__ === null
  ) {
    deno.__proto__ = __nimbusPublicDenoPrototype;
  }
  return deno;
}
const Deno = new globalThis.Proxy(globalThis.Object.create(null), {
  get(_target, prop) {
    return __nimbusResolveDeno()[prop];
  },
  set(_target, prop, value) {
    __nimbusResolveDeno()[prop] = value;
    return true;
  },
  has(_target, prop) {
    return prop in __nimbusResolveDeno();
  },
  ownKeys() {
    return globalThis.Reflect.ownKeys(__nimbusResolveDeno());
  },
  getOwnPropertyDescriptor(_target, prop) {
    const descriptor = globalThis.Object.getOwnPropertyDescriptor(
      __nimbusResolveDeno(),
      prop,
    );
    if (descriptor) {
      return descriptor;
    }
    const value = __nimbusResolveDeno()[prop];
    if (value === undefined) {
      return undefined;
    }
    return {
      value,
      configurable: true,
      enumerable: true,
      writable: true,
    };
  },
});
"#;

pub(crate) fn extension_transpiler_for_target(
    target: RuntimeCompatibilityTarget,
) -> Option<Rc<RuntimeExtensionTranspiler>> {
    match target {
        RuntimeCompatibilityTarget::BunJsc | RuntimeCompatibilityTarget::WasmComponent => None,
        // WebStandardIsolate now evaluates the shared global scope, which loads .ts ext
        // scripts (deno_telemetry), so it needs the same extension transpiler as Node.
        RuntimeCompatibilityTarget::WebStandardIsolate
        | RuntimeCompatibilityTarget::Node20
        | RuntimeCompatibilityTarget::Node22
        | RuntimeCompatibilityTarget::Node24
        | RuntimeCompatibilityTarget::Node26 => Some(Rc::new(maybe_transpile_source)),
    }
}

fn maybe_transpile_source(
    name: ModuleName,
    source: ModuleCodeString,
) -> Result<(ModuleCodeString, Option<SourceMapData>), JsErrorBox> {
    let source = rewrite_node_extension_source(&name, source.to_string())?;

    // Match Deno's extension transpilation contract so Node22 startup and live
    // runtime composition can consume the same TypeScript-backed ext modules.
    let media_type = if name.starts_with("node:") {
        MediaType::TypeScript
    } else {
        MediaType::from_path(Path::new(&name))
    };

    match media_type {
        MediaType::TypeScript => {}
        MediaType::JavaScript | MediaType::Mjs => return Ok((source.into(), None)),
        _ => {
            return Err(JsErrorBox::generic(format!(
                "unsupported media type for runtime extension transpilation {media_type:?} for file {name}",
            )));
        }
    }

    let specifier = deno_core::url::Url::parse(&name).map_err(|error| {
        JsErrorBox::generic(format!(
            "invalid runtime extension specifier {name}: {error}"
        ))
    })?;
    let parsed = deno_ast::parse_module(ParseParams {
        specifier,
        text: source.into(),
        media_type,
        capture_tokens: false,
        scope_analysis: false,
        maybe_syntax: None,
    })
    .map_err(|error| JsErrorBox::from_err(JsParseDiagnostic(error)))?;

    let transpiled_source = parsed
        .transpile(
            &deno_ast::TranspileOptions {
                imports_not_used_as_values: deno_ast::ImportsNotUsedAsValues::Remove,
                ..Default::default()
            },
            &deno_ast::TranspileModuleOptions::default(),
            &deno_ast::EmitOptions {
                source_map: if cfg!(debug_assertions) {
                    SourceMapOption::Separate
                } else {
                    SourceMapOption::None
                },
                ..Default::default()
            },
        )
        .map_err(|error| JsErrorBox::from_err(JsTranspileError(error)))?
        .into_source();

    let maybe_source_map = transpiled_source
        .source_map
        .map(|source_map| source_map.into_bytes().into());
    Ok((transpiled_source.text.into(), maybe_source_map))
}

fn rewrite_node_extension_source(name: &str, source: String) -> Result<String, JsErrorBox> {
    if !name.starts_with("ext:deno_node/") && !name.starts_with("node:") {
        return Ok(source);
    }

    let mut source = source;
    if name == "ext:deno_node/worker_threads.ts" {
        source = inject_worker_share_env_metadata(source)?;
    }

    // Keep Deno's Node polyfills bound to Nimbus-owned Node bootstrap state.
    // Node22 mode now retains a managed public `globalThis.Deno` because Deno
    // 2.8's lazy Node polyfills consult it after startup, but extension code
    // still resolves through this proxy so the substrate is not tenant-owned.
    let source = source
        .replace(
            "globalThis.__bootstrap.ext_node_denoGlobals",
            "__nimbusInternalDenoGlobals",
        )
        .replace(
            "globalThis.__bootstrap.ext_node_nodeGlobals",
            "__nimbusInternalNodeGlobals",
        )
        .replace("globalThis.Deno", "Deno");
    if name.starts_with("ext:deno_node/")
        && let Some(source) = inject_node_lazy_script_prelude(&source)
    {
        return Ok(source);
    }

    Ok(format!(
        "{NODE_EXTENSION_INTERNAL_DENO_PRELUDE_HEADER}{NODE_EXTENSION_INTERNAL_DENO_PRELUDE_BODY}{source}"
    ))
}

fn inject_worker_share_env_metadata(source: String) -> Result<String, JsErrorBox> {
    const WORKER_METADATA_ENV_FIELD: &str = "      env: env_,\n      argv: argv_,";
    const WORKER_METADATA_WITH_SHARE_ENV: &str =
        "      env: env_,\n      shareEnv: envOpt === SHARE_ENV,\n      argv: argv_,";
    let anchor_count = source.matches(WORKER_METADATA_ENV_FIELD).count();
    if anchor_count != 1 {
        return Err(JsErrorBox::generic(format!(
            "worker_threads SHARE_ENV metadata rewrite expected exactly one env/argv metadata anchor, found {anchor_count}",
        )));
    }
    Ok(source.replacen(WORKER_METADATA_ENV_FIELD, WORKER_METADATA_WITH_SHARE_ENV, 1))
}

fn inject_node_lazy_script_prelude(source: &str) -> Option<String> {
    const IIFE_OPENINGS: [&str; 2] = ["(function () {", "(function(){"];
    for opening in IIFE_OPENINGS {
        if let Some(offset) = source.find(opening) {
            let insert_at = offset + opening.len();
            let mut rewritten = String::with_capacity(
                source.len()
                    + NODE_EXTENSION_INTERNAL_DENO_SCRIPT_PRELUDE_HEADER.len()
                    + NODE_EXTENSION_INTERNAL_DENO_PRELUDE_BODY.len()
                    + 1,
            );
            rewritten.push_str(&source[..insert_at]);
            rewritten.push('\n');
            rewritten.push_str(NODE_EXTENSION_INTERNAL_DENO_SCRIPT_PRELUDE_HEADER);
            rewritten.push_str(NODE_EXTENSION_INTERNAL_DENO_PRELUDE_BODY);
            rewritten.push_str(&source[insert_at..]);
            return Some(rewritten);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn extension_transpiler_rejects_unsupported_media_type_without_panicking() {
        let error = maybe_transpile_source(
            String::from("ext:nimbus_runtime/bootstrap/data.json").into(),
            String::from("{}").into(),
        )
        .expect_err("unsupported extension media type should return an error");

        let message = error.to_string();
        assert!(
            message.contains("unsupported media type for runtime extension transpilation"),
            "unexpected transpiler error: {message}"
        );
    }

    #[test]
    fn extension_transpiler_rejects_invalid_specifier_without_panicking() {
        let error = maybe_transpile_source(
            String::from("runtime_extension.ts").into(),
            String::from("export {};").into(),
        )
        .expect_err("malformed extension specifier should return an error");

        let message = error.to_string();
        assert!(
            message.contains("invalid runtime extension specifier"),
            "unexpected transpiler error: {message}"
        );
    }

    #[test]
    fn worker_threads_rewrite_preserves_share_env_metadata() {
        let source = r#"
const serializedWorkerMetadata = serializeJsMessageData({
      workerData: options?.workerData,
      environmentData: environmentData,
      env: env_,
      argv: argv_,
}, options?.transferList ?? []);
"#;

        let rewritten =
            rewrite_node_extension_source("ext:deno_node/worker_threads.ts", source.to_string())
                .expect("worker_threads metadata rewrite should succeed");

        assert!(
            rewritten.contains("shareEnv: envOpt === SHARE_ENV"),
            "worker_threads rewrite must keep SHARE_ENV visible to Nimbus worker bootstrap: {rewritten}"
        );
    }

    #[test]
    fn worker_threads_rewrite_rejects_missing_share_env_anchor() {
        let error = rewrite_node_extension_source(
            "ext:deno_node/worker_threads.ts",
            "const serializedWorkerMetadata = serializeJsMessageData({});".to_string(),
        )
        .expect_err("missing worker metadata anchor should fail closed");

        let message = error.to_string();
        assert!(
            message.contains("expected exactly one env/argv metadata anchor, found 0"),
            "unexpected rewrite error: {message}"
        );
    }

    #[test]
    fn worker_threads_rewrite_rejects_duplicate_share_env_anchor() {
        let anchor = "      env: env_,\n      argv: argv_,";
        let error = rewrite_node_extension_source(
            "ext:deno_node/worker_threads.ts",
            format!("const a = {{\n{anchor}\n}};\nconst b = {{\n{anchor}\n}};"),
        )
        .expect_err("duplicate worker metadata anchors should fail closed");

        let message = error.to_string();
        assert!(
            message.contains("expected exactly one env/argv metadata anchor, found 2"),
            "unexpected rewrite error: {message}"
        );
    }
}
