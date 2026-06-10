use std::path::Path;

use crate::backends::v8::embedder::JsErrorBox;

use super::bundle::{rewrite_bundle_path, rewrite_bundle_string};
use super::types::RuntimeTestSpawnPlan;

pub(super) fn render_eval_execution(
    plan: &RuntimeTestSpawnPlan,
    bundle_dir: &Path,
    source: &str,
    print_result: bool,
    input_type_module: bool,
) -> std::result::Result<String, JsErrorBox> {
    let rendered_source = render_eval_source(plan, bundle_dir, source);
    if input_type_module {
        render_module_eval(bundle_dir, rendered_source, print_result)
    } else {
        render_commonjs_eval(plan, bundle_dir, rendered_source, print_result)
    }
}

fn render_eval_source(plan: &RuntimeTestSpawnPlan, bundle_dir: &Path, source: &str) -> String {
    if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
        if plan.permission_restricted {
            source.to_string()
        } else {
            rewrite_bundle_string(source, source_bundle_root, bundle_dir)
        }
    } else {
        source.to_string()
    }
}

fn render_module_eval(
    bundle_dir: &Path,
    rendered_source: String,
    print_result: bool,
) -> std::result::Result<String, JsErrorBox> {
    let eval_module_path = bundle_dir.join("__nimbus_eval__.mjs");
    let eval_require_base_path = eval_module_path.to_string_lossy().into_owned();
    let mut rendered_source = rendered_source;

    if rewrite_module_test_imports(&mut rendered_source) {
        let eval_require_base_path = serde_json::to_string(&eval_require_base_path)
            .expect("eval require base path should serialize");
        rendered_source = format!(
            "import {{ createRequire as __nimbusCreateRequire }} from \"node:module\";\nconst __nimbusEvalRequire = __nimbusCreateRequire({eval_require_base_path});\n{rendered_source}"
        );
    }

    write_eval_file(&eval_module_path, &rendered_source)?;
    Ok(format!(
        r#"
const __nimbusEvalModuleUrl = require("node:url").pathToFileURL({}).href;
const __nimbusEvalResult = await import(__nimbusEvalModuleUrl);
{}"#,
        serde_json::to_string(&eval_module_path.to_string_lossy().into_owned())
            .expect("eval module path should serialize"),
        render_print_result_block(print_result)
    ))
}

fn rewrite_module_test_imports(rendered_source: &mut String) -> bool {
    let mut needs_eval_require = false;
    for (needle, replacement) in [
        (
            "import test from \"test\";",
            "const test = __nimbusEvalRequire(\"test\");",
        ),
        (
            "import test from 'test';",
            "const test = __nimbusEvalRequire('test');",
        ),
        (
            "import(\"test\")",
            "Promise.resolve({ default: __nimbusEvalRequire(\"test\") })",
        ),
        (
            "import('test')",
            "Promise.resolve({ default: __nimbusEvalRequire('test') })",
        ),
    ] {
        if rendered_source.contains(needle) {
            *rendered_source = rendered_source.replace(needle, replacement);
            needs_eval_require = true;
        }
    }
    needs_eval_require
}

fn render_commonjs_eval(
    plan: &RuntimeTestSpawnPlan,
    bundle_dir: &Path,
    rendered_source: String,
    print_result: bool,
) -> std::result::Result<String, JsErrorBox> {
    let eval_require_base_path = commonjs_eval_require_base_path(plan, bundle_dir);
    if rendered_source.contains("import(") || rendered_source.contains("import (") {
        render_commonjs_dynamic_import_eval(
            bundle_dir,
            &eval_require_base_path,
            rendered_source,
            print_result,
        )
    } else {
        Ok(render_commonjs_inline_eval(
            &eval_require_base_path,
            &rendered_source,
            print_result,
        ))
    }
}

fn commonjs_eval_require_base_path(plan: &RuntimeTestSpawnPlan, bundle_dir: &Path) -> String {
    plan.cwd
        .as_deref()
        .map(|cwd| {
            let base_path = cwd.join("$deno$eval.cjs");
            if let Some(source_bundle_root) = plan.source_bundle_root.as_deref() {
                if plan.permission_restricted {
                    base_path
                } else {
                    rewrite_bundle_path(&base_path, source_bundle_root, bundle_dir)
                }
            } else {
                bundle_dir.join("$deno$eval.cjs")
            }
        })
        .unwrap_or_else(|| bundle_dir.join("$deno$eval.cjs"))
        .to_string_lossy()
        .into_owned()
}

fn render_commonjs_dynamic_import_eval(
    bundle_dir: &Path,
    eval_require_base_path: &str,
    rendered_source: String,
    print_result: bool,
) -> std::result::Result<String, JsErrorBox> {
    let rendered_source = rendered_source
        .replace(
            "import(\"test\")",
            "Promise.resolve({ default: __nimbusEvalRequire(\"test\") })",
        )
        .replace(
            "import('test')",
            "Promise.resolve({ default: __nimbusEvalRequire('test') })",
        );
    let rendered_source = format!(
        "const __nimbusEvalRequire = require(\"node:module\").createRequire({});\n{}",
        serde_json::to_string(eval_require_base_path)
            .expect("eval require base path should serialize"),
        rendered_source
    );
    let eval_file_path = bundle_dir.join("$deno$eval.cjs");
    write_eval_file(&eval_file_path, &rendered_source)?;

    Ok(format!(
        r#"
const __nimbusEvalFilename = {filename};
const __nimbusEvalRequire = require("node:module").createRequire(__nimbusEvalFilename);
let __nimbusEvalResult = __nimbusEvalRequire(__nimbusEvalFilename);
if (
  __nimbusEvalResult &&
  typeof __nimbusEvalResult.then === "function"
) {{
  __nimbusEvalResult = await __nimbusEvalResult;
}}
{print_result_block}"#,
        filename = serde_json::to_string(&eval_file_path.to_string_lossy().into_owned())
            .expect("eval require base path should serialize"),
        print_result_block = render_print_result_block(print_result),
    ))
}

fn render_commonjs_inline_eval(
    eval_require_base_path: &str,
    rendered_source: &str,
    print_result: bool,
) -> String {
    format!(
        r#"
const __nimbusEvalSource = {source};
const __nimbusEvalFilename = {filename};
const __nimbusEvalDirname = require("node:path").dirname(__nimbusEvalFilename);
const __nimbusEvalRequire = require("node:module").createRequire(__nimbusEvalFilename);
const __nimbusEvalModule = {{
  exports: {{}},
  filename: __nimbusEvalFilename,
  path: __nimbusEvalDirname,
  paths: require("node:module")._nodeModulePaths(__nimbusEvalDirname),
}};
let __nimbusEvalResult = ((require, module, exports, __filename, __dirname) => eval(__nimbusEvalSource))(
  __nimbusEvalRequire,
  __nimbusEvalModule,
  __nimbusEvalModule.exports,
  __nimbusEvalFilename,
  __nimbusEvalDirname,
);
if (
  __nimbusEvalResult &&
  typeof __nimbusEvalResult.then === "function"
) {{
  __nimbusEvalResult = await __nimbusEvalResult;
}}
{print_result_block}"#,
        source = serde_json::to_string(rendered_source).expect("eval source should serialize"),
        filename = serde_json::to_string(eval_require_base_path)
            .expect("eval require base path should serialize"),
        print_result_block = render_print_result_block(print_result),
    )
}

fn render_print_result_block(print_result: bool) -> &'static str {
    if print_result {
        r#"
if (__nimbusEvalResult !== undefined) {
  stdout += `${captureChunk(__nimbusEvalResult)}
`;
}
"#
    } else {
        ""
    }
}

fn write_eval_file(path: &Path, source: &str) -> std::result::Result<(), JsErrorBox> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|error| {
            JsErrorBox::generic(format!(
                "node_compat eval module parent should build: {error}"
            ))
        })?;
    }
    std::fs::write(path, source).map_err(|error| {
        JsErrorBox::generic(format!("node_compat eval module should write: {error}"))
    })
}

#[cfg(test)]
mod tests {
    use std::collections::BTreeMap;
    use std::path::PathBuf;

    use tempfile::tempdir;

    use super::*;
    use crate::runtime::bootstrap::ops::test_runtime::types::{
        RuntimeTestSpawnMode, RuntimeTestSpawnPlan,
    };

    fn plan(
        cwd: Option<PathBuf>,
        source_bundle_root: Option<PathBuf>,
        permission_restricted: bool,
    ) -> RuntimeTestSpawnPlan {
        RuntimeTestSpawnPlan {
            command: "node".to_string(),
            mode: RuntimeTestSpawnMode::LegacyInspectorFlagError,
            cwd,
            env: Some(BTreeMap::new()),
            stdin_bytes: None,
            exec_argv: Vec::new(),
            source_bundle_root,
            preload_env_file: None,
            permission_restricted,
            process_title: None,
            expose_gc: false,
            inspector_open: None,
        }
    }

    #[test]
    fn module_eval_writes_rewritten_test_import_module() {
        let bundle = tempdir().expect("bundle dir should create");
        let plan = plan(None, None, false);

        let rendered = render_eval_execution(
            &plan,
            bundle.path(),
            "import test from \"test\";\nexport default test;",
            true,
            true,
        )
        .expect("module eval should render");

        let eval_module = std::fs::read_to_string(bundle.path().join("__nimbus_eval__.mjs"))
            .expect("eval module should be written");
        assert!(rendered.contains("__nimbusEvalModuleUrl"));
        assert!(rendered.contains("captureChunk(__nimbusEvalResult)"));
        assert!(eval_module.contains("__nimbusCreateRequire"));
        assert!(eval_module.contains("const test = __nimbusEvalRequire(\"test\");"));
        assert!(!eval_module.contains("import test from \"test\";"));
    }

    #[test]
    fn commonjs_dynamic_import_eval_writes_require_backed_module() {
        let bundle = tempdir().expect("bundle dir should create");
        let plan = plan(None, None, false);

        let rendered = render_eval_execution(
            &plan,
            bundle.path(),
            "const mod = await import(\"test\"); mod.default();",
            false,
            false,
        )
        .expect("commonjs dynamic import eval should render");

        let eval_module = std::fs::read_to_string(bundle.path().join("$deno$eval.cjs"))
            .expect("commonjs eval module should be written");
        assert!(rendered.contains("__nimbusEvalRequire(__nimbusEvalFilename)"));
        assert!(!rendered.contains("captureChunk(__nimbusEvalResult)"));
        assert!(eval_module.contains("createRequire"));
        assert!(
            eval_module.contains("Promise.resolve({ default: __nimbusEvalRequire(\"test\") })")
        );
    }

    #[test]
    fn commonjs_inline_eval_rewrites_source_bundle_paths_when_unrestricted() {
        let source_root = tempdir().expect("source root should create");
        let bundle = tempdir().expect("bundle dir should create");
        let source_file = source_root.path().join("fixtures/source.js");
        let parent = source_file
            .parent()
            .expect("source file should have parent");
        std::fs::create_dir_all(parent).expect("source parent should create");
        std::fs::write(&source_file, "module.exports = 1;").expect("source file should write");
        let plan = plan(
            Some(source_root.path().to_path_buf()),
            Some(source_root.path().to_path_buf()),
            false,
        );
        let source_file = source_file.to_string_lossy().into_owned();

        let rendered = render_eval_execution(
            &plan,
            bundle.path(),
            &format!(
                "require({});",
                serde_json::to_string(&source_file).expect("path should serialize")
            ),
            false,
            false,
        )
        .expect("commonjs inline eval should render");

        let bundle_path = bundle.path().to_string_lossy();
        let source_root_path = source_root.path().to_string_lossy();
        assert!(rendered.contains(bundle_path.as_ref()));
        assert!(!rendered.contains(source_root_path.as_ref()));
    }
}
