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

const NODE_EXTENSION_INTERNAL_DENO_PRELUDE: &str = r#"
import { core as __neovexCore } from "ext:core/mod.js";
import {
  denoGlobals as __neovexInternalDenoGlobals,
  nodeGlobals as __neovexInternalNodeGlobals,
  publicDenoPrototype as __neovexPublicDenoPrototype,
} from "ext:neovex_node22/internal_bootstrap.js";

function __neovexResolveDeno() {
  const deno = __neovexInternalDenoGlobals;
  if (deno.core === undefined) {
    deno.core = __neovexCore;
  }
  if (deno.build === undefined && __neovexCore.build !== undefined) {
    deno.build = __neovexCore.build;
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
        return globalThis.process?.env?.[name];
      },
      toObject() {
        return { ...(globalThis.process?.env ?? {}) };
      },
      set(name, value) {
        if (globalThis.process?.env) {
          globalThis.process.env[String(name)] = String(value);
        }
      },
      delete(name) {
        if (globalThis.process?.env) {
          delete globalThis.process.env[String(name)];
        }
      },
    };
  }
  if (deno.execPath === undefined) {
    deno.execPath = () => __neovexCore.ops.op_neovex_runtime_exec_path();
  }
  if (deno.version === undefined) {
    deno.version = {
      deno: "2.7.14-neovex",
      v8: "147.4.0-locker.1",
      typescript: "0.0.0-neovex",
    };
  }
  if (
    __neovexPublicDenoPrototype &&
    (typeof __neovexPublicDenoPrototype === "object" ||
      typeof __neovexPublicDenoPrototype === "function") &&
    deno.__proto__ === null
  ) {
    deno.__proto__ = __neovexPublicDenoPrototype;
  }
  return deno;
}
const Deno = new globalThis.Proxy(globalThis.Object.create(null), {
  get(_target, prop) {
    return __neovexResolveDeno()[prop];
  },
  set(_target, prop, value) {
    __neovexResolveDeno()[prop] = value;
    return true;
  },
  has(_target, prop) {
    return prop in __neovexResolveDeno();
  },
  ownKeys() {
    return globalThis.Reflect.ownKeys(__neovexResolveDeno());
  },
  getOwnPropertyDescriptor(_target, prop) {
    const descriptor = globalThis.Object.getOwnPropertyDescriptor(
      __neovexResolveDeno(),
      prop,
    );
    if (descriptor) {
      return descriptor;
    }
    const value = __neovexResolveDeno()[prop];
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
        RuntimeCompatibilityTarget::WebStandardIsolate => None,
        RuntimeCompatibilityTarget::Node22 => Some(Rc::new(maybe_transpile_source)),
    }
}

fn maybe_transpile_source(
    name: ModuleName,
    source: ModuleCodeString,
) -> Result<(ModuleCodeString, Option<SourceMapData>), JsErrorBox> {
    let source = rewrite_node_extension_source(&name, source.to_string());

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
        _ => panic!(
            "unsupported media type for runtime extension transpilation {media_type:?} for file {name}",
        ),
    }

    let parsed = deno_ast::parse_module(ParseParams {
        specifier: deno_core::url::Url::parse(&name).unwrap(),
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

fn rewrite_node_extension_source(name: &str, source: String) -> String {
    if !name.starts_with("ext:deno_node/") && !name.starts_with("node:") {
        return source;
    }

    // Keep Deno's Node polyfills bound to the hidden `__bootstrap` substrate
    // instead of the public `globalThis.Deno` contract that user bundles
    // should not observe in Node22 mode.
    let source = source
        .replace(
            "globalThis.__bootstrap.ext_node_denoGlobals",
            "__neovexInternalDenoGlobals",
        )
        .replace(
            "globalThis.__bootstrap.ext_node_nodeGlobals",
            "__neovexInternalNodeGlobals",
        )
        .replace("globalThis.Deno", "Deno");
    format!("{NODE_EXTENSION_INTERNAL_DENO_PRELUDE}{source}")
}
