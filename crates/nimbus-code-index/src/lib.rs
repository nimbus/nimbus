//! Deploy-time structural code-navigation index built with oxc (FSV7).
//!
//! Parses a module with oxc and extracts:
//! - exported bindings (a Convex function is `export const name = query(...)`),
//! - imports (from the ESM module record),
//! - outbound function references — `api.*` / `internal.*` member chains, which
//!   in Convex resolve to a function path `module:export` (e.g.
//!   `api.admin.users.create` -> `admin/users:create`).
//!
//! This is the structural foundation for console code navigation
//! (go-to-definition, callers, import/call graph). Pure (source -> index), no
//! I/O. oxc is pinned at `=0.136.0`.

use oxc::allocator::Allocator;
use oxc::ast::ast::{Expression, StaticMemberExpression};
use oxc::ast_visit::{Visit, walk};
use oxc::parser::Parser;
use oxc::span::SourceType;
use oxc::syntax::module_record::ExportExportName;
use serde::{Deserialize, Serialize};

/// An exported binding and the 1-based line of its export.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ExportedSymbol {
    pub name: String,
    pub line: u32,
}

/// An import: the module specifier and the local binding name.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ImportedSymbol {
    pub specifier: String,
    pub name: String,
}

/// An outbound reference to another function (`api.*` / `internal.*`), resolved
/// to its `module:export` path, with the 1-based line of the reference.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct FunctionReference {
    pub target: String,
    pub line: u32,
}

/// The structural index for one module.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct ModuleAnalysis {
    pub exports: Vec<ExportedSymbol>,
    pub imports: Vec<ImportedSymbol>,
    pub references: Vec<FunctionReference>,
    pub parse_errors: usize,
}

/// Analyze a TypeScript/JavaScript module's structure with oxc.
pub fn analyze_module(source: &str) -> ModuleAnalysis {
    let allocator = Allocator::default();
    let ret = Parser::new(&allocator, source, SourceType::ts()).parse();
    let module_record = &ret.module_record;

    let mut exports = Vec::new();
    for entry in &module_record.local_export_entries {
        if let ExportExportName::Name(name_span) = &entry.export_name {
            exports.push(ExportedSymbol {
                name: name_span.name.as_str().to_string(),
                line: line_of(source, name_span.span.start),
            });
        }
    }
    exports.sort_by(|a, b| a.name.cmp(&b.name).then(a.line.cmp(&b.line)));
    exports.dedup();

    let mut imports = Vec::new();
    for entry in &module_record.import_entries {
        imports.push(ImportedSymbol {
            specifier: entry.module_request.name.as_str().to_string(),
            name: entry.local_name.name.as_str().to_string(),
        });
    }
    imports.sort_by(|a, b| {
        a.specifier
            .cmp(&b.specifier)
            .then_with(|| a.name.cmp(&b.name))
    });
    imports.dedup();

    let mut collector = ReferenceCollector {
        source,
        references: Vec::new(),
    };
    collector.visit_program(&ret.program);
    let mut references = collector.references;
    references.sort_by(|a, b| a.target.cmp(&b.target).then(a.line.cmp(&b.line)));
    references.dedup();

    ModuleAnalysis {
        exports,
        imports,
        references,
        parse_errors: ret.diagnostics.len(),
    }
}

/// Walks the AST collecting `api.*` / `internal.*` member chains as function
/// references. Convex's `api`/`internal` objects mirror the file tree, so
/// `api.<a>.<b>.<c>` resolves to module `a/b`, export `c`.
struct ReferenceCollector<'s> {
    source: &'s str,
    references: Vec<FunctionReference>,
}

impl<'a, 's> Visit<'a> for ReferenceCollector<'s> {
    fn visit_static_member_expression(&mut self, it: &StaticMemberExpression<'a>) {
        if let Some((root, mut segments)) = flatten_member_chain(it)
            && (root == "api" || root == "internal")
            && segments.len() >= 2
        {
            let export = segments.pop().expect("len >= 2");
            let module = segments.join("/");
            self.references.push(FunctionReference {
                target: format!("{module}:{export}"),
                line: line_of(self.source, it.span.start),
            });
            // Do not recurse into a resolved api/internal chain — the inner
            // member expressions are part of this single reference.
            return;
        }
        walk::walk_static_member_expression(self, it);
    }
}

/// Flatten a static member expression into `(root identifier, [segments...])`
/// in source order, or `None` if the base is not a plain identifier.
fn flatten_member_chain(member: &StaticMemberExpression) -> Option<(String, Vec<String>)> {
    let mut segments = vec![member.property.name.as_str().to_string()];
    let mut current = &member.object;
    loop {
        match current {
            Expression::StaticMemberExpression(inner) => {
                segments.push(inner.property.name.as_str().to_string());
                current = &inner.object;
            }
            Expression::Identifier(identifier) => {
                let root = identifier.name.as_str().to_string();
                segments.reverse();
                return Some((root, segments));
            }
            _ => return None,
        }
    }
}

/// 1-based line number for a byte `offset` into `source`.
fn line_of(source: &str, offset: u32) -> u32 {
    let end = (offset as usize).min(source.len());
    1 + source.as_bytes()[..end]
        .iter()
        .filter(|&&b| b == b'\n')
        .count() as u32
}

#[cfg(test)]
mod tests {
    use super::*;

    const CONVEX_MODULE: &str = r#"import { v } from "convex/values";
import { query, mutation } from "./_generated/server";
import { api, internal } from "./_generated/api";

export const list = query({
  args: {},
  handler: async (ctx) => await ctx.db.query("messages").take(20),
});

export const notify = mutation({
  args: { body: v.string() },
  handler: async (ctx, { body }) => {
    const recent = await ctx.runQuery(api.messages.list, {});
    await ctx.runMutation(internal.admin.users.touch, { body });
    return recent;
  },
});
"#;

    #[test]
    fn extracts_exported_functions_with_lines() {
        let analysis = analyze_module(CONVEX_MODULE);
        let names: Vec<&str> = analysis.exports.iter().map(|e| e.name.as_str()).collect();
        assert_eq!(names, vec!["list", "notify"]);
        assert_eq!(analysis.parse_errors, 0);
        let list = analysis
            .exports
            .iter()
            .find(|e| e.name == "list")
            .expect("list");
        let notify = analysis
            .exports
            .iter()
            .find(|e| e.name == "notify")
            .expect("notify");
        assert!(list.line >= 1 && list.line < notify.line);
    }

    #[test]
    fn extracts_imports_with_specifier_and_local_name() {
        let analysis = analyze_module(CONVEX_MODULE);
        let specifiers: Vec<&str> = analysis
            .imports
            .iter()
            .map(|i| i.specifier.as_str())
            .collect();
        assert!(specifiers.contains(&"convex/values"));
        assert!(specifiers.contains(&"./_generated/server"));
        let names: Vec<&str> = analysis.imports.iter().map(|i| i.name.as_str()).collect();
        assert!(names.contains(&"query") && names.contains(&"v"));
    }

    #[test]
    fn extracts_api_and_internal_function_references() {
        let analysis = analyze_module(CONVEX_MODULE);
        let targets: Vec<&str> = analysis
            .references
            .iter()
            .map(|r| r.target.as_str())
            .collect();
        // api.messages.list -> messages:list ; internal.admin.users.touch -> admin/users:touch
        assert!(
            targets.contains(&"messages:list"),
            "expected messages:list in {targets:?}"
        );
        assert!(
            targets.contains(&"admin/users:touch"),
            "expected admin/users:touch in {targets:?}"
        );
    }

    #[test]
    fn reports_parse_errors_for_broken_source() {
        // oxc is error-tolerant (recovers a partial AST); assert on diagnostics.
        assert!(analyze_module("export const = = =;\n").parse_errors > 0);
    }

    #[test]
    fn clean_module_has_no_parse_errors_and_no_spurious_refs() {
        let analysis = analyze_module("export const n = 1;\nconst x = obj.a.b;\n");
        assert_eq!(analysis.parse_errors, 0);
        // obj.a.b is not an api/internal chain -> no references.
        assert!(analysis.references.is_empty());
    }
}
