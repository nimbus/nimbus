use std::collections::{BTreeMap, BTreeSet};
use std::env;
use std::fs;
use std::path::{Path, PathBuf};

use proc_macro2::Span;
use serde::Serialize;
use syn::punctuated::Punctuated;
use syn::spanned::Spanned;
use syn::visit::{self, Visit};
use syn::{
    Arm, Attribute, Expr, ExprCall, ExprMethodCall, ExprPath, ExprStruct, Field, FieldValue, FnArg,
    ForeignItem, ForeignItemFn, ImplItem, ImplItemFn, ImplItemType, Item, ItemFn, ItemImpl,
    ItemMod, ItemStruct, ItemType, ItemUse, Lit, LitStr, Macro, ReturnType, Stmt, Token, TraitItem,
    TraitItemFn, TraitItemType, Type, UseTree, Variant,
};

const FIXTURE_PATH: &str =
    "__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs";

#[derive(Debug, Default, Serialize)]
struct ScanOutput {
    authorities: Vec<Occurrence>,
    risks: Vec<Occurrence>,
    composition: Vec<Occurrence>,
    declarations: Vec<Declaration>,
    boundaries: Vec<Boundary>,
    errors: Vec<String>,
}

#[derive(Clone, Debug, Serialize)]
struct Boundary {
    path: String,
    kind: String,
    detail: String,
    line: usize,
    #[serde(skip_serializing)]
    column: usize,
}

#[derive(Clone, Debug, Serialize)]
struct Occurrence {
    path: String,
    kind: String,
    symbol: String,
    ordinal: usize,
    line: usize,
    #[serde(skip_serializing)]
    column: usize,
}

#[derive(Clone, Debug, Serialize)]
struct Declaration {
    path: String,
    name: String,
    line: usize,
}

#[derive(Default)]
struct Args {
    root: PathBuf,
    excludes: BTreeSet<String>,
}

fn main() {
    let args = parse_args().unwrap_or_else(|error| {
        eprintln!("{error}");
        std::process::exit(2);
    });
    let focused_fixture =
        env::var("NIMBUS_NETWORK_VERIFY_FOCUSED_BIND_CHILD").as_deref() == Ok("1");
    if focused_fixture
        && (env::var("NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD").as_deref() != Ok("1")
            || (env::var("NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE")
                .map(|fixture| fixture.is_empty())
                .unwrap_or(true)
                && env::var("NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED")
                    .map(|fixture| fixture.is_empty())
                    .unwrap_or(true)))
    {
        eprintln!("focused bind census requires a self-test child with an injected bind fixture");
        std::process::exit(2);
    }
    let mut files = Vec::new();
    if !focused_fixture {
        walk_rust_sources(&args.root, &mut files).unwrap_or_else(|error| {
            eprintln!("failed to walk {}: {error}", args.root.display());
            std::process::exit(1);
        });
    }
    files.sort();
    let exempt_paths = files
        .iter()
        .map(|file| normalized_path(file))
        .filter(|file| args.excludes.contains(file) || is_convention_exempt(file))
        .collect::<BTreeSet<_>>();

    let mut output = ScanOutput::default();
    for file in files {
        let display = normalized_path(&file);
        if exempt_paths.contains(&display) {
            continue;
        }
        let source = match fs::read_to_string(&file) {
            Ok(source) => source,
            Err(error) => {
                output
                    .errors
                    .push(format!("Rust source unreadable: {display}: {error}"));
                continue;
            }
        };
        scan_source(&display, &source, &mut output, &exempt_paths);
    }

    if env::var("NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD").as_deref() == Ok("1")
        && let Ok(fixture) = env::var("NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE")
    {
        scan_source(FIXTURE_PATH, &fixture, &mut output, &exempt_paths);
    }

    finish_ordinals(&mut output.authorities);
    finish_ordinals(&mut output.risks);
    finish_ordinals(&mut output.composition);
    output.declarations.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.name.cmp(&right.name))
    });
    output.boundaries.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.column.cmp(&right.column))
            .then(left.kind.cmp(&right.kind))
            .then(left.detail.cmp(&right.detail))
    });
    serde_json::to_writer(std::io::stdout(), &output).unwrap_or_else(|error| {
        eprintln!("failed to encode census output: {error}");
        std::process::exit(1);
    });
    println!();
}

fn parse_args() -> Result<Args, String> {
    let mut args = Args {
        root: PathBuf::from("crates"),
        ..Args::default()
    };
    let mut values = env::args().skip(1);
    while let Some(value) = values.next() {
        match value.as_str() {
            "--root" => {
                args.root = PathBuf::from(
                    values
                        .next()
                        .ok_or_else(|| "--root requires a path".to_owned())?,
                );
            }
            "--exclude" => {
                let excluded = values
                    .next()
                    .ok_or_else(|| "--exclude requires a repository path".to_owned())?;
                args.excludes.insert(normalized_path(Path::new(&excluded)));
            }
            _ => return Err(format!("unknown argument: {value}")),
        }
    }
    Ok(args)
}

fn walk_rust_sources(directory: &Path, files: &mut Vec<PathBuf>) -> std::io::Result<()> {
    if !directory.is_dir() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "source root is not a directory",
        ));
    }
    let mut entries = fs::read_dir(directory)?.collect::<Result<Vec<_>, _>>()?;
    entries.sort_by_key(|entry| entry.file_name());
    for entry in entries {
        let path = entry.path();
        let file_type = entry.file_type()?;
        if file_type.is_dir() {
            walk_rust_sources(&path, files)?;
        } else if file_type.is_file()
            && path.extension().and_then(|extension| extension.to_str()) == Some("rs")
        {
            files.push(path);
        }
    }
    Ok(())
}

fn is_convention_exempt(path: &str) -> bool {
    path == "crates/nimbus-testing"
        || path.starts_with("crates/nimbus-testing/")
        || path == "crates/nimbus-process-harness"
        || path.starts_with("crates/nimbus-process-harness/")
        || path.ends_with("/tests.rs")
        || path
            .split('/')
            .any(|component| matches!(component, "tests" | "benches"))
}

fn scan_source(
    path: &str,
    source: &str,
    output: &mut ScanOutput,
    exempt_paths: &BTreeSet<String>,
) {
    let file = match syn::parse_file(source) {
        Ok(file) => file,
        Err(error) => {
            output.errors.push(format!(
                "Rust source parse failed: {path}:{}:{}: {error}",
                error.span().start().line,
                error.span().start().column + 1
            ));
            return;
        }
    };
    verify_exempt_references(path, &file, exempt_paths, &mut output.errors);
    let source_path = PathBuf::from(path);
    let source_directory = source_path.parent().unwrap_or(Path::new(""));
    let module_directory = if source_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "lib.rs" | "main.rs" | "mod.rs"))
    {
        source_directory.to_path_buf()
    } else {
        source_directory.join(
            source_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default(),
        )
    };
    let mut scanner = Scanner {
        path,
        source_directory: source_directory.to_path_buf(),
        module_directories: vec![module_directory],
        symbols: Vec::new(),
        impl_types: Vec::new(),
        authorities: &mut output.authorities,
        risks: &mut output.risks,
        composition: &mut output.composition,
        declarations: &mut output.declarations,
        boundaries: &mut output.boundaries,
        errors: &mut output.errors,
    };
    scanner.visit_file(&file);
}

fn verify_exempt_references(
    path: &str,
    file: &syn::File,
    exempt_paths: &BTreeSet<String>,
    errors: &mut Vec<String>,
) {
    let source_path = PathBuf::from(path);
    let source_directory = source_path.parent().unwrap_or(Path::new(""));
    let module_directory = if source_path
        .file_name()
        .and_then(|name| name.to_str())
        .is_some_and(|name| matches!(name, "lib.rs" | "main.rs" | "mod.rs"))
    {
        source_directory.to_path_buf()
    } else {
        source_directory.join(
            source_path
                .file_stem()
                .and_then(|stem| stem.to_str())
                .unwrap_or_default(),
        )
    };
    let mut scanner = ExemptReferenceScanner {
        source_path: path,
        source_directory,
        module_directories: vec![module_directory],
        exempt_paths,
        errors,
    };
    scanner.visit_file(file);
}

struct ExemptReferenceScanner<'a> {
    source_path: &'a str,
    source_directory: &'a Path,
    module_directories: Vec<PathBuf>,
    exempt_paths: &'a BTreeSet<String>,
    errors: &'a mut Vec<String>,
}

impl ExemptReferenceScanner<'_> {
    fn module_directory(&self) -> &Path {
        self.module_directories
            .last()
            .map(PathBuf::as_path)
            .unwrap_or(self.source_directory)
    }

    fn check_candidates(
        &mut self,
        raw_path: &str,
        candidates: impl IntoIterator<Item = PathBuf>,
        span: Span,
    ) {
        let raw_is_exempt = is_convention_exempt(&normalized_path(Path::new(raw_path)));
        let matched = candidates
            .into_iter()
            .map(|candidate| normalized_path(&candidate))
            .find(|candidate| {
                self.exempt_paths.contains(candidate) || is_convention_exempt(candidate)
            });
        if raw_is_exempt || matched.is_some() {
            let start = span.start();
            self.errors.push(format!(
                "{}:{}:{}: production module/include references test-exempt source: {}",
                self.source_path,
                start.line,
                start.column + 1,
                matched.unwrap_or_else(|| raw_path.to_owned())
            ));
        }
    }

    fn visit_module(&mut self, item: &ItemMod) {
        if let Some((_, items)) = &item.content {
            self.module_directories
                .push(self.module_directory().join(item.ident.to_string()));
            for nested in items {
                self.visit_item(nested);
            }
            self.module_directories.pop();
            return;
        }

        if let Some(path_value) = path_attribute(&item.attrs) {
            self.check_candidates(
                &path_value,
                [
                    self.source_directory.join(&path_value),
                    self.module_directory().join(&path_value),
                ],
                item.ident.span(),
            );
            return;
        }

        let module_name = item.ident.to_string();
        self.check_candidates(
            &module_name,
            [
                self.module_directory().join(format!("{module_name}.rs")),
                self.module_directory().join(&module_name).join("mod.rs"),
            ],
            item.ident.span(),
        );
    }
}

impl<'ast> Visit<'ast> for ExemptReferenceScanner<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        self.visit_module(item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if impl_item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if trait_item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_trait_item(self, item);
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        if foreign_item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_foreign_item(self, item);
    }

    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if stmt_attributes(statement).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_stmt(self, statement);
    }

    fn visit_expr(&mut self, expression: &'ast Expr) {
        if expr_attributes(expression).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_expr(self, expression);
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        if is_cfg_test(&arm.attrs) {
            return;
        }
        visit::visit_arm(self, arm);
    }

    fn visit_field_value(&mut self, field: &'ast FieldValue) {
        if is_cfg_test(&field.attrs) {
            return;
        }
        visit::visit_field_value(self, field);
    }

    fn visit_macro(&mut self, macro_invocation: &'ast Macro) {
        if macro_invocation
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "include")
            && let Ok(path) = syn::parse2::<LitStr>(macro_invocation.tokens.clone())
        {
            let path = path.value();
            self.check_candidates(
                &path,
                [
                    self.source_directory.join(&path),
                    self.module_directory().join(&path),
                ],
                macro_invocation.span(),
            );
        }
        visit::visit_macro(self, macro_invocation);
    }
}

fn path_attribute(attributes: &[Attribute]) -> Option<String> {
    attributes.iter().find_map(|attribute| {
        if !attribute.path().is_ident("path") {
            return None;
        }
        let syn::Meta::NameValue(name_value) = &attribute.meta else {
            return None;
        };
        let Expr::Lit(literal) = &name_value.value else {
            return None;
        };
        let Lit::Str(value) = &literal.lit else {
            return None;
        };
        Some(value.value())
    })
}

fn module_path_boundaries(attributes: &[Attribute]) -> Vec<String> {
    fn collect(meta: &syn::Meta, paths: &mut Vec<String>) {
        if let syn::Meta::NameValue(name_value) = meta
            && name_value.path.is_ident("path")
            && let Expr::Lit(literal) = &name_value.value
            && let Lit::Str(value) = &literal.lit
        {
            paths.push(value.value());
            return;
        }
        let syn::Meta::List(list) = meta else {
            return;
        };
        if !list.path.is_ident("cfg_attr") {
            return;
        }
        let Ok(items) = list.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated)
        else {
            return;
        };
        for item in items.iter().skip(1) {
            collect(item, paths);
        }
    }

    let mut paths = Vec::new();
    for attribute in attributes {
        collect(&attribute.meta, &mut paths);
    }
    paths
}

fn conditional_module_predicates(attributes: &[Attribute]) -> Vec<String> {
    attributes
        .iter()
        .filter_map(|attribute| {
            let syn::Meta::List(list) = &attribute.meta else {
                return None;
            };
            if attribute.path().is_ident("cfg") {
                Some(format!("cfg({})", list.tokens))
            } else if attribute.path().is_ident("cfg_attr") {
                Some(format!("cfg_attr({})", list.tokens))
            } else {
                None
            }
        })
        .collect()
}

fn network_glob_roots(tree: &UseTree) -> Vec<String> {
    fn visit(tree: &UseTree, prefix: &mut Vec<String>, roots: &mut Vec<String>) {
        match tree {
            UseTree::Path(path) => {
                prefix.push(path.ident.to_string());
                visit(&path.tree, prefix, roots);
                prefix.pop();
            }
            UseTree::Group(group) => {
                for item in &group.items {
                    visit(item, prefix, roots);
                }
            }
            UseTree::Glob(_) => {
                let is_network_root = matches!(prefix.first().map(String::as_str), Some("socket2"))
                    || matches!(prefix.first().map(String::as_str), Some("std" | "tokio"))
                        && prefix.iter().any(|segment| segment == "net");
                if is_network_root {
                    roots.push(prefix.join("::"));
                }
            }
            UseTree::Name(_) | UseTree::Rename(_) => {}
        }
    }

    let mut roots = Vec::new();
    visit(tree, &mut Vec::new(), &mut roots);
    roots
}

fn finish_ordinals(occurrences: &mut Vec<Occurrence>) {
    occurrences.sort_by(|left, right| {
        left.path
            .cmp(&right.path)
            .then(left.line.cmp(&right.line))
            .then(left.column.cmp(&right.column))
            .then(left.kind.cmp(&right.kind))
    });
    let mut ordinals = BTreeMap::<(String, String, String), usize>::new();
    for occurrence in occurrences {
        let key = (
            occurrence.path.clone(),
            occurrence.kind.clone(),
            occurrence.symbol.clone(),
        );
        let ordinal = ordinals.entry(key).or_default();
        *ordinal += 1;
        occurrence.ordinal = *ordinal;
    }
}

struct Scanner<'a> {
    path: &'a str,
    source_directory: PathBuf,
    module_directories: Vec<PathBuf>,
    symbols: Vec<String>,
    impl_types: Vec<String>,
    authorities: &'a mut Vec<Occurrence>,
    risks: &'a mut Vec<Occurrence>,
    composition: &'a mut Vec<Occurrence>,
    declarations: &'a mut Vec<Declaration>,
    boundaries: &'a mut Vec<Boundary>,
    errors: &'a mut Vec<String>,
}

impl Scanner<'_> {
    fn module_directory(&self) -> &Path {
        self.module_directories
            .last()
            .map(PathBuf::as_path)
            .unwrap_or(&self.source_directory)
    }

    fn conditional_module_sources(&self, item: &ItemMod) -> Vec<String> {
        let explicit = module_path_boundaries(&item.attrs);
        let mut sources = BTreeSet::new();
        if explicit.is_empty() {
            let name = item.ident.to_string();
            sources.insert(normalized_path(
                &self.module_directory().join(format!("{name}.rs")),
            ));
            sources.insert(normalized_path(
                &self.module_directory().join(name).join("mod.rs"),
            ));
        } else {
            for raw in explicit {
                sources.insert(normalized_path(&self.source_directory.join(&raw)));
                sources.insert(normalized_path(&self.module_directory().join(raw)));
            }
        }
        sources.into_iter().collect()
    }

    fn symbol(&self) -> String {
        self.symbols
            .last()
            .cloned()
            .unwrap_or_else(|| "<module>".to_owned())
    }

    fn authority(&mut self, kind: &str, span: Span) {
        self.authorities
            .push(occurrence(self.path, kind, self.symbol(), span));
    }

    fn risk(&mut self, kind: &str, span: Span) {
        self.risks
            .push(occurrence(self.path, kind, self.symbol(), span));
    }

    fn composition(&mut self, kind: &str, span: Span) {
        self.composition
            .push(occurrence(self.path, kind, self.symbol(), span));
    }

    fn error(&mut self, message: impl Into<String>, span: Span) {
        let start = span.start();
        self.errors.push(format!(
            "{}:{}:{}: {}",
            self.path,
            start.line,
            start.column + 1,
            message.into()
        ));
    }

    fn boundary(&mut self, kind: &str, detail: impl Into<String>, span: Span) {
        let start = span.start();
        self.boundaries.push(Boundary {
            path: self.path.to_owned(),
            kind: kind.to_owned(),
            detail: detail.into(),
            line: start.line,
            column: start.column,
        });
    }

    fn visit_signature_body(
        &mut self,
        name: &str,
        signature: &syn::Signature,
        block: Option<&syn::Block>,
    ) {
        self.declarations.push(Declaration {
            path: self.path.to_owned(),
            name: name.to_owned(),
            line: signature.ident.span().start().line,
        });
        self.symbols.push(name.to_owned());
        if let Some(kind) = self
            .impl_types
            .last()
            .and_then(|type_name| network_composition_declaration_kind(type_name, name))
        {
            self.composition(kind, signature.ident.span());
        }
        scan_port_function_name(name, signature.ident.span(), self);
        visit::visit_signature(self, signature);
        if let Some(block) = block {
            self.visit_block(block);
        }
        self.symbols.pop();
    }
}

fn occurrence(path: &str, kind: &str, symbol: String, span: Span) -> Occurrence {
    let start = span.start();
    Occurrence {
        path: path.to_owned(),
        kind: kind.to_owned(),
        symbol,
        ordinal: 0,
        line: start.line,
        column: start.column,
    }
}

impl<'ast> Visit<'ast> for Scanner<'_> {
    fn visit_item(&mut self, item: &'ast Item) {
        if item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_item(self, item);
    }

    fn visit_impl_item(&mut self, item: &'ast ImplItem) {
        if impl_item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_impl_item(self, item);
    }

    fn visit_item_impl(&mut self, item: &'ast ItemImpl) {
        if is_cfg_test(&item.attrs) {
            return;
        }
        let type_name = match item.self_ty.as_ref() {
            Type::Path(path) => path
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string()),
            _ => None,
        };
        if let Some(type_name) = type_name {
            self.impl_types.push(type_name);
            visit::visit_item_impl(self, item);
            self.impl_types.pop();
        } else {
            visit::visit_item_impl(self, item);
        }
    }

    fn visit_item_mod(&mut self, item: &'ast ItemMod) {
        for path in module_path_boundaries(&item.attrs) {
            self.boundary("module-path", path, item.ident.span());
        }
        let predicates = conditional_module_predicates(&item.attrs);
        if item.content.is_none() && !predicates.is_empty() {
            let sources = self.conditional_module_sources(item);
            self.boundary(
                "conditional-module",
                format!("{}|{}", predicates.join(" && "), sources.join(",")),
                item.ident.span(),
            );
        }
        if item.content.is_some() {
            self.module_directories
                .push(self.module_directory().join(item.ident.to_string()));
            visit::visit_item_mod(self, item);
            self.module_directories.pop();
        } else {
            visit::visit_item_mod(self, item);
        }
    }

    fn visit_item_use(&mut self, item: &'ast ItemUse) {
        for root in network_glob_roots(&item.tree) {
            self.boundary("network-glob-import", root, item.span());
        }
        visit::visit_item_use(self, item);
    }

    fn visit_trait_item(&mut self, item: &'ast TraitItem) {
        if trait_item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_trait_item(self, item);
    }

    fn visit_foreign_item(&mut self, item: &'ast ForeignItem) {
        if foreign_item_attributes(item).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_foreign_item(self, item);
    }

    fn visit_stmt(&mut self, statement: &'ast Stmt) {
        if stmt_attributes(statement).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_stmt(self, statement);
    }

    fn visit_arm(&mut self, arm: &'ast Arm) {
        if is_cfg_test(&arm.attrs) {
            return;
        }
        visit::visit_arm(self, arm);
    }

    fn visit_field_value(&mut self, field: &'ast FieldValue) {
        if is_cfg_test(&field.attrs) {
            return;
        }
        visit::visit_field_value(self, field);
    }

    fn visit_item_fn(&mut self, item: &'ast ItemFn) {
        self.visit_signature_body(&item.sig.ident.to_string(), &item.sig, Some(&item.block));
    }

    fn visit_impl_item_fn(&mut self, item: &'ast ImplItemFn) {
        if is_cfg_test(&item.attrs) {
            return;
        }
        self.visit_signature_body(&item.sig.ident.to_string(), &item.sig, Some(&item.block));
    }

    fn visit_trait_item_fn(&mut self, item: &'ast TraitItemFn) {
        if is_cfg_test(&item.attrs) {
            return;
        }
        self.visit_signature_body(
            &item.sig.ident.to_string(),
            &item.sig,
            item.default.as_ref(),
        );
    }

    fn visit_foreign_item_fn(&mut self, item: &'ast ForeignItemFn) {
        if is_cfg_test(&item.attrs) {
            return;
        }
        self.visit_signature_body(&item.sig.ident.to_string(), &item.sig, None);
    }

    fn visit_item_struct(&mut self, item: &'ast ItemStruct) {
        if item.ident == "PortManager" {
            self.authorities.push(occurrence(
                self.path,
                "legacy-port-manager-definition",
                item.ident.to_string(),
                item.ident.span(),
            ));
        }
        visit::visit_item_struct(self, item);
    }

    fn visit_item_type(&mut self, item: &'ast ItemType) {
        if network_composition_type_info(&item.ty) {
            self.composition("composition-type-alias", item.ident.span());
        }
        if socket_type_info(&item.ty).found {
            self.error(
                format!("socket authority type alias is forbidden: {}", item.ident),
                item.ident.span(),
            );
        }
        visit::visit_item_type(self, item);
    }

    fn visit_impl_item_type(&mut self, item: &'ast ImplItemType) {
        if socket_type_info(&item.ty).found {
            self.error(
                format!(
                    "associated socket authority type alias is forbidden: {}",
                    item.ident
                ),
                item.ident.span(),
            );
        }
        visit::visit_impl_item_type(self, item);
    }

    fn visit_trait_item_type(&mut self, item: &'ast TraitItemType) {
        if let Some((_, ty)) = &item.default
            && socket_type_info(ty).found
        {
            self.error(
                format!(
                    "associated socket authority type alias is forbidden: {}",
                    item.ident
                ),
                item.ident.span(),
            );
        }
        visit::visit_trait_item_type(self, item);
    }

    fn visit_use_tree(&mut self, tree: &'ast UseTree) {
        match tree {
            UseTree::Name(name) if is_bind_operation(&name.ident.to_string()) => {
                self.risk("ambiguous-bind-function-import", name.span());
            }
            UseTree::Rename(rename) => {
                let original = rename.ident.to_string();
                let alias = rename.rename.to_string();
                if is_network_composition_type(&original) {
                    self.composition("composition-type-import-alias", rename.span());
                }
                if is_socket_type_name(&original)
                    && !(original == "UnixListener" && alias == "StdUnixListener")
                {
                    self.error(
                        format!(
                            "ambiguous socket authority alias is forbidden: {original} as {alias}"
                        ),
                        rename.span(),
                    );
                }
                if is_bind_operation(&original) {
                    self.risk("ambiguous-bind-function-import", rename.span());
                }
            }
            _ => {}
        }
        visit::visit_use_tree(self, tree);
    }

    fn visit_field(&mut self, field: &'ast Field) {
        if is_cfg_test(&field.attrs) {
            return;
        }
        let info = socket_type_info(&field.ty);
        if info.owned {
            self.authority("listener-ownership-slot", field.ty.span());
        }
        visit::visit_field(self, field);
    }

    fn visit_variant(&mut self, variant: &'ast Variant) {
        if is_cfg_test(&variant.attrs) {
            return;
        }
        visit::visit_variant(self, variant);
    }

    fn visit_fn_arg(&mut self, argument: &'ast FnArg) {
        if let FnArg::Typed(typed) = argument {
            if is_cfg_test(&typed.attrs) {
                return;
            }
            let info = socket_type_info(&typed.ty);
            if info.owned {
                self.authority("listener-ownership-slot", typed.ty.span());
            }
        }
        visit::visit_fn_arg(self, argument);
    }

    fn visit_return_type(&mut self, return_type: &'ast ReturnType) {
        if let ReturnType::Type(_, ty) = return_type {
            let info = socket_type_info(ty);
            if info.owned {
                self.authority("listener-return-handoff", ty.span());
            }
        }
        visit::visit_return_type(self, return_type);
    }

    fn visit_expr(&mut self, expression: &'ast Expr) {
        if expr_attributes(expression).is_some_and(is_cfg_test) {
            return;
        }
        visit::visit_expr(self, expression);
    }

    fn visit_expr_call(&mut self, call: &'ast ExprCall) {
        if let Expr::Path(function) = &*call.func
            && function.path.segments.len() == 1
        {
            let operation = function
                .path
                .segments
                .last()
                .map(|segment| segment.ident.to_string())
                .unwrap_or_default();
            if is_bind_operation(&operation) {
                self.risk("ambiguous-bare-bind-call", call.func.span());
            }
            if operation == "send_machine_forwarder_request" {
                let method = call.args.iter().nth(1).and_then(expr_string_literal);
                let path = call.args.iter().nth(2).and_then(expr_string_literal);
                if method.as_deref() == Some("POST")
                    && matches!(path.as_deref(), Some("/expose" | "/unexpose"))
                {
                    self.authority("machine-forwarder-port-request", call.func.span());
                }
            }
        }
        visit::visit_expr_call(self, call);
    }

    fn visit_expr_path(&mut self, expression: &'ast ExprPath) {
        let segments = expression.path.segments.iter().collect::<Vec<_>>();
        if expression.qself.is_some()
            && let Some(operation) = segments.last().map(|segment| segment.ident.to_string())
            && is_bind_operation(&operation)
        {
            self.boundary("qself-bind-adoption", operation, expression.span());
        }
        if segments.len() >= 2 {
            let operation = segments[segments.len() - 1].ident.to_string();
            let receiver = segments[segments.len() - 2].ident.to_string();
            if let Some(kind) = network_composition_call_kind(&receiver, &operation) {
                self.composition(kind, expression.path.span());
            }
            if let Some(kind) = associated_authority_kind(&receiver, &operation) {
                self.authority(kind, expression.path.span());
            } else if operation == "bind" {
                self.risk("ambiguous-associated-bind", expression.path.span());
            } else if is_descriptor_adoption(&operation) {
                self.risk("ambiguous-descriptor-adoption", expression.path.span());
            }
        }
        visit::visit_expr_path(self, expression);
    }

    fn visit_expr_method_call(&mut self, call: &'ast ExprMethodCall) {
        if call.method == "bind" {
            self.risk("ambiguous-instance-bind", call.method.span());
        }
        visit::visit_expr_method_call(self, call);
    }

    fn visit_expr_struct(&mut self, expression: &'ast ExprStruct) {
        let Some(type_name) = expression
            .path
            .segments
            .last()
            .map(|segment| segment.ident.to_string())
        else {
            visit::visit_expr_struct(self, expression);
            return;
        };
        if type_name.ends_with("Request") {
            let fields = expression
                .fields
                .iter()
                .filter_map(|field| match &field.member {
                    syn::Member::Named(ident) => Some(ident.to_string()),
                    syn::Member::Unnamed(_) => None,
                })
                .collect::<BTreeSet<_>>();
            let kind = if type_name == "MachinePortForwardRequest" {
                Some("machine-forwarder-port-request")
            } else if type_name == "NetavarkRequest" {
                Some("netavark-port-mapping-request")
            } else if fields.iter().any(|field| {
                field == "host_port"
                    || field == "port_mappings"
                    || (field.starts_with("publish") && field.ends_with("_port"))
            }) {
                Some("provider-port-request")
            } else {
                None
            };
            if let Some(kind) = kind {
                self.authority(kind, expression.path.span());
            }
        }
        visit::visit_expr_struct(self, expression);
    }

    fn visit_lit_str(&mut self, literal: &'ast LitStr) {
        let value = literal.value();
        if matches!(value.as_str(), "-ssh-port" | "--ssh-port") {
            self.authority("gvproxy-ssh-port-request", literal.span());
        }
        if value.contains("ListenStream=") {
            self.authority("systemd-listen-stream-request", literal.span());
        }
        visit::visit_lit_str(self, literal);
    }

    fn visit_macro(&mut self, macro_invocation: &'ast Macro) {
        let tokens = macro_invocation.tokens.to_string();
        let macro_name = macro_invocation
            .path
            .segments
            .iter()
            .map(|segment| segment.ident.to_string())
            .collect::<Vec<_>>()
            .join("::");
        if macro_invocation
            .path
            .segments
            .last()
            .is_some_and(|segment| segment.ident == "include")
        {
            self.boundary("include-expansion", tokens.clone(), macro_invocation.span());
        }
        if tokens.contains("NetworkCapabilityBundle :: new") {
            self.composition("capability-bundle-construction", macro_invocation.span());
        }
        if tokens.contains("ListenStream") {
            self.authority("systemd-listen-stream-request", macro_invocation.span());
        }
        if tokens.contains("ssh-port") {
            self.authority("gvproxy-ssh-port-request", macro_invocation.span());
        }
        let name_words = macro_name
            .split(|character: char| !character.is_ascii_alphanumeric())
            .filter(|word| !word.is_empty())
            .collect::<BTreeSet<_>>();
        let operation_shaped_name = ["bind", "adopt", "reserve", "allocate"]
            .iter()
            .any(|word| name_words.contains(word))
            && ["listener", "socket", "port", "fd", "stream"]
                .iter()
                .any(|word| name_words.contains(word));
        if operation_shaped_name
            || [
                "TcpListener",
                "UdpSocket",
                "TcpSocket",
                "UnixListener",
                "UnixDatagram",
                "host_port",
                "port_mappings",
            ]
            .iter()
            .any(|needle| tokens.contains(needle))
        {
            self.risk("authority-shaped-macro", macro_invocation.span());
            self.boundary(
                "authority-shaped-macro",
                format!("{macro_name}|{tokens}"),
                macro_invocation.span(),
            );
        }
        visit::visit_macro(self, macro_invocation);
    }
}

fn network_composition_call_kind(receiver: &str, operation: &str) -> Option<&'static str> {
    match (receiver, operation) {
        ("LocalNodeNetworkRoot", "resolve_for_current_platform") => {
            Some("local-node-root-resolver")
        }
        ("LocalNetworkManager", "bootstrap") => Some("manager-bootstrap"),
        ("LocalNetworkManager", "open") => Some("manager-direct-open"),
        ("LocalNetworkStateStore", "open") => Some("primitive-state-store-open"),
        ("LocalNetworkStateStore", "open_with_options") => {
            Some("primitive-state-store-open-with-options")
        }
        ("LocalPortLeaseAuthority", "open") => Some("primitive-port-authority-open"),
        ("StagedLocalNetworkComposition", "claim") => Some("cli-staged-manager-claim"),
        ("PreparedLocalNetworkComposition", "prepare") => Some("cli-complete-composition"),
        ("PreparedLocalNetworkComposition", "prepare_attachment_only") => {
            Some("cli-attachment-only-composition")
        }
        ("OciNetworkProcess", "new") => Some("oci-process-construction"),
        ("ConfiguredSegmentAllocator", "reconstruct_direct") => {
            Some("segment-direct-reconstruction")
        }
        ("ConfiguredSegmentAllocator", "reconstruct_for_runner") => {
            Some("segment-runner-reconstruction")
        }
        ("ConfiguredSegmentAllocator", "reconstruct_from_state_root") => {
            Some("segment-primitive-reconstruction")
        }
        ("SingleNodeSegmentAllocator", "reconstruct_for_cluster_lease") => {
            Some("segment-cluster-lease-reconstruction")
        }
        ("SingleNodeSegmentAllocator", "reconstruct_for_cluster_cleanup") => {
            Some("segment-cluster-cleanup-reconstruction")
        }
        ("DurableSegmentCleanupAuthority", "reconstruct_for_cluster_cleanup") => {
            Some("segment-cleanup-handle-reconstruction")
        }
        ("OciIpamAuthority", "reconstruct_direct") => Some("ipam-direct-reconstruction"),
        ("OciIpamAuthority", "reconstruct_for_runner") => Some("ipam-runner-reconstruction"),
        ("OciPortLeaseCoordinator", "reconstruct_direct") => {
            Some("port-coordinator-direct-reconstruction")
        }
        ("OciPortLeaseCoordinator", "reconstruct_for_runner") => {
            Some("port-coordinator-runner-reconstruction")
        }
        ("KrunSandboxBackend", "new") => Some("direct-krun-backend-construction"),
        ("KrunSandboxBackend", "with_network_process") => {
            Some("manager-derived-krun-backend-construction")
        }
        ("ContainerSandboxBackend", "new") => Some("direct-container-backend-construction"),
        ("ContainerSandboxBackend", "with_network_process") => {
            Some("manager-derived-container-backend-construction")
        }
        ("PreboundServerListeners", "new") => Some("manager-derived-prebound-listeners"),
        ("PreboundServerListeners", "reconstruct_direct") => {
            Some("direct-prebound-listener-reconstruction")
        }
        ("ServeOptions", "new") => Some("manager-derived-serve-options"),
        ("ServeOptions", "reconstruct_direct" | "reconstruct_direct_at") => {
            Some("direct-serve-options-reconstruction")
        }
        ("ServerListenerLeaseAuthority", "reconstruct_direct") => {
            Some("server-internal-direct-reconstruction")
        }
        ("ServerListenerLeaseAuthority", "new") => {
            Some("manager-derived-server-listener-authority")
        }
        ("RetainedServerNetworkAuthority", "manager_derived") => {
            Some("manager-derived-server-primitive-handle")
        }
        ("RetainedServerNetworkAuthority", "reconstruct_direct") => {
            Some("server-primitive-direct-reconstruction")
        }
        ("NimbusKvListenerConfig", "from_network_authority") => Some("manager-derived-kv-listener"),
        ("NimbusKvListenerConfig", "from_network_authority_for_incarnation") => {
            Some("manager-derived-kv-listener-incarnation")
        }
        ("NimbusKvListenerConfig", "reconstruct_direct") => {
            Some("kv-direct-listener-reconstruction")
        }
        ("NimbusKvListenerConfig", "reconstruct_direct_for_incarnation") => {
            Some("kv-direct-listener-incarnation-reconstruction")
        }
        ("HostMachineNetworkAuthority", "injected") => {
            Some("manager-derived-parent-machine-authority")
        }
        ("MachineForwarderAuthority", "new") => Some("machine-forwarder-authority-mint"),
        ("NetworkAttachmentProviderRegistration", "new") => {
            Some("attachment-registration-construction")
        }
        ("NetworkIngressProviderRegistration", "new") => Some("ingress-registration-construction"),
        ("NetworkCapabilityBundle", "new") => Some("capability-bundle-construction"),
        ("NetworkCapabilityRegistry", "new") => Some("capability-registry-construction"),
        _ => None,
    }
}

fn network_composition_declaration_kind(type_name: &str, operation: &str) -> Option<&'static str> {
    match (type_name, operation) {
        ("LocalNodeNetworkRoot", "resolve_for_current_platform") => {
            Some("local-node-root-resolver-declaration")
        }
        ("LocalNetworkManager", "bootstrap") => Some("manager-bootstrap-declaration"),
        ("LocalNetworkManager", "open") => Some("manager-open-declaration"),
        ("LocalNetworkStateStore", "open" | "open_with_options") => {
            Some("primitive-state-store-open-declaration")
        }
        ("LocalPortLeaseAuthority", "open") => Some("primitive-port-authority-open-declaration"),
        ("ConfiguredSegmentAllocator", "reconstruct_direct") => {
            Some("segment-direct-reconstruction-declaration")
        }
        ("ConfiguredSegmentAllocator", "reconstruct_for_runner") => {
            Some("segment-runner-reconstruction-declaration")
        }
        ("ConfiguredSegmentAllocator", "reconstruct_from_state_root") => {
            Some("segment-primitive-reconstruction-declaration")
        }
        ("SingleNodeSegmentAllocator", "reconstruct_for_cluster_lease") => {
            Some("segment-cluster-lease-reconstruction-declaration")
        }
        ("SingleNodeSegmentAllocator", "reconstruct_for_cluster_cleanup") => {
            Some("segment-cluster-cleanup-reconstruction-declaration")
        }
        ("DurableSegmentCleanupAuthority", "reconstruct_for_cluster_cleanup") => {
            Some("segment-cleanup-handle-reconstruction-declaration")
        }
        ("OciIpamAuthority", "reconstruct_direct") => {
            Some("ipam-direct-reconstruction-declaration")
        }
        ("OciIpamAuthority", "reconstruct_for_runner") => {
            Some("ipam-runner-reconstruction-declaration")
        }
        ("OciPortLeaseCoordinator", "reconstruct_direct") => {
            Some("port-coordinator-direct-reconstruction-declaration")
        }
        ("OciPortLeaseCoordinator", "reconstruct_for_runner") => {
            Some("port-coordinator-runner-reconstruction-declaration")
        }
        ("ContainerSandboxBackend", "new") => {
            Some("direct-container-backend-constructor-declaration")
        }
        ("ContainerSandboxBackend", "with_network_process") => {
            Some("manager-derived-container-backend-constructor-declaration")
        }
        ("ContainerSandboxBackend", "reconstruct_for_runner") => {
            Some("container-runner-reconstruction-declaration")
        }
        ("KrunSandboxBackend", "new") => Some("direct-krun-backend-constructor-declaration"),
        ("KrunSandboxBackend", "with_network_process") => {
            Some("manager-derived-krun-backend-constructor-declaration")
        }
        ("PreboundServerListeners", "new") => {
            Some("manager-derived-prebound-listeners-declaration")
        }
        ("PreboundServerListeners", "reconstruct_direct") => {
            Some("direct-prebound-listener-reconstruction-declaration")
        }
        ("ServeOptions", "new") => Some("manager-derived-serve-options-declaration"),
        ("ServeOptions", "reconstruct_direct" | "reconstruct_direct_at") => {
            Some("direct-serve-options-reconstruction-declaration")
        }
        ("ServerListenerLeaseAuthority", "new") => {
            Some("manager-derived-server-listener-authority-declaration")
        }
        ("ServerListenerLeaseAuthority", "reconstruct_direct") => {
            Some("server-internal-direct-reconstruction-declaration")
        }
        ("RetainedServerNetworkAuthority", "manager_derived") => {
            Some("manager-derived-server-primitive-handle-declaration")
        }
        ("RetainedServerNetworkAuthority", "reconstruct_direct") => {
            Some("server-primitive-direct-reconstruction-declaration")
        }
        ("NimbusKvListenerConfig", "from_network_authority") => {
            Some("manager-derived-kv-listener-declaration")
        }
        ("NimbusKvListenerConfig", "from_network_authority_for_incarnation") => {
            Some("manager-derived-kv-listener-incarnation-declaration")
        }
        ("NimbusKvListenerConfig", "reconstruct_direct") => {
            Some("kv-direct-listener-reconstruction-declaration")
        }
        ("NimbusKvListenerConfig", "reconstruct_direct_for_incarnation") => {
            Some("kv-direct-listener-incarnation-reconstruction-declaration")
        }
        ("HostMachineNetworkComposition", "claim_default" | "claim") => {
            Some("parent-machine-manager-constructor-declaration")
        }
        ("HostMachineNetworkAuthority", "injected") => {
            Some("manager-derived-parent-machine-authority-declaration")
        }
        ("GuestMachineNetworkComposition", "claim") => {
            Some("guest-machine-manager-constructor-declaration")
        }
        ("MachineForwarderAuthority", "new") => {
            Some("machine-forwarder-authority-mint-declaration")
        }
        _ => None,
    }
}

fn is_network_composition_type(name: &str) -> bool {
    matches!(
        name,
        "LocalNodeNetworkRoot"
            | "LocalNetworkManager"
            | "LocalNetworkStateStore"
            | "LocalPortLeaseAuthority"
            | "StagedLocalNetworkComposition"
            | "PreparedLocalNetworkComposition"
            | "OciNetworkProcess"
            | "ConfiguredSegmentAllocator"
            | "SingleNodeSegmentAllocator"
            | "DurableSegmentCleanupAuthority"
            | "OciIpamAuthority"
            | "OciPortLeaseCoordinator"
            | "KrunSandboxBackend"
            | "ContainerSandboxBackend"
            | "PreboundServerListeners"
            | "ServeOptions"
            | "ServerListenerLeaseAuthority"
            | "RetainedServerNetworkAuthority"
            | "NetworkAttachmentProviderRegistration"
            | "NetworkIngressProviderRegistration"
            | "NetworkCapabilityBundle"
            | "NetworkCapabilityRegistry"
            | "NimbusKvListenerConfig"
            | "HostMachineNetworkComposition"
            | "HostMachineNetworkAuthority"
            | "GuestMachineNetworkComposition"
            | "MachineForwarderAuthority"
    )
}

fn network_composition_type_info(ty: &Type) -> bool {
    struct Inspector {
        found: bool,
    }

    impl<'ast> Visit<'ast> for Inspector {
        fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
            if path
                .path
                .segments
                .last()
                .is_some_and(|segment| is_network_composition_type(&segment.ident.to_string()))
            {
                self.found = true;
            }
            visit::visit_type_path(self, path);
        }
    }

    let mut inspector = Inspector { found: false };
    inspector.visit_type(ty);
    inspector.found
}

fn scan_port_function_name(name: &str, span: Span, scanner: &mut Scanner<'_>) {
    if matches!(
        name,
        "resolve_listener_port"
            | "ephemeral_port"
            | "allocate_machine_ssh_port"
            | "machine_port_is_available"
    ) {
        scanner.authority("legacy-port-probe-definition", span);
        return;
    }
    let tokens = name.split('_').collect::<BTreeSet<_>>();
    let has_port = tokens.contains("port") || tokens.contains("ports");
    let has_action = [
        "allocate",
        "available",
        "choose",
        "ephemeral",
        "find",
        "free",
        "pick",
        "reserve",
        "select",
        "unused",
    ]
    .iter()
    .any(|action| tokens.contains(action));
    if has_port && has_action {
        scanner.authority("suspicious-port-allocation-definition", span);
    }
}

fn associated_authority_kind(receiver: &str, operation: &str) -> Option<&'static str> {
    match (receiver, operation) {
        ("TcpListener", "bind") => Some("tcp-bind"),
        ("TcpSocket", "bind") => Some("tcp-socket-bind"),
        ("UdpSocket", "bind") => Some("udp-bind"),
        ("Socket", "bind") => Some("generic-socket-bind"),
        ("TcpListener", "from_std") => Some("tcp-from-std"),
        ("TcpListener", "from_raw_fd") => Some("tcp-from-raw-fd"),
        ("TcpListener", "from_raw_socket") => Some("tcp-from-raw-socket"),
        ("UdpSocket", "from_std") => Some("udp-from-std"),
        ("UdpSocket", "from_raw_fd") => Some("udp-from-raw-fd"),
        ("UdpSocket", "from_raw_socket") => Some("udp-from-raw-socket"),
        ("UnixListener" | "StdUnixListener", "bind") => Some("unix-bind"),
        ("UnixListener" | "StdUnixListener", "from_std") => Some("unix-from-std"),
        ("UnixListener" | "StdUnixListener", "from_raw_fd") => Some("unix-from-raw-fd"),
        ("UnixDatagram" | "StdUnixDatagram", "bind") => Some("unix-datagram-bind"),
        ("UnixDatagram" | "StdUnixDatagram", "from_std") => Some("unix-datagram-from-std"),
        ("UnixDatagram" | "StdUnixDatagram", "from_raw_fd") => Some("unix-datagram-from-raw-fd"),
        _ => None,
    }
}

fn is_descriptor_adoption(operation: &str) -> bool {
    matches!(operation, "from_std" | "from_raw_fd" | "from_raw_socket")
}

fn is_bind_operation(operation: &str) -> bool {
    operation == "bind" || is_descriptor_adoption(operation)
}

fn expr_string_literal(expression: &Expr) -> Option<String> {
    let Expr::Lit(literal) = expression else {
        return None;
    };
    let Lit::Str(value) = &literal.lit else {
        return None;
    };
    Some(value.value())
}

fn is_socket_type_name(name: &str) -> bool {
    matches!(
        name,
        "TcpListener"
            | "TcpSocket"
            | "UdpSocket"
            | "Socket"
            | "UnixListener"
            | "StdUnixListener"
            | "UnixDatagram"
            | "StdUnixDatagram"
    )
}

#[derive(Default)]
struct SocketTypeInfo {
    found: bool,
    owned: bool,
}

fn socket_type_info(ty: &Type) -> SocketTypeInfo {
    struct Inspector {
        info: SocketTypeInfo,
        reference_depth: usize,
    }

    impl<'ast> Visit<'ast> for Inspector {
        fn visit_type_reference(&mut self, reference: &'ast syn::TypeReference) {
            self.reference_depth += 1;
            visit::visit_type_reference(self, reference);
            self.reference_depth -= 1;
        }

        fn visit_type_path(&mut self, path: &'ast syn::TypePath) {
            if path
                .path
                .segments
                .last()
                .is_some_and(|segment| is_socket_type_name(&segment.ident.to_string()))
            {
                self.info.found = true;
                if self.reference_depth == 0 {
                    self.info.owned = true;
                }
            }
            visit::visit_type_path(self, path);
        }
    }
    let mut inspector = Inspector {
        info: SocketTypeInfo::default(),
        reference_depth: 0,
    };
    inspector.visit_type(ty);
    inspector.info
}

fn is_cfg_test(attributes: &[Attribute]) -> bool {
    attributes.iter().any(|attribute| {
        if !attribute.path().is_ident("cfg") {
            return false;
        }
        let syn::Meta::List(list) = &attribute.meta else {
            return false;
        };
        syn::parse2::<syn::Meta>(list.tokens.clone())
            .is_ok_and(|meta| cfg_meta_requires_test(&meta))
    })
}

fn cfg_meta_requires_test(meta: &syn::Meta) -> bool {
    match meta {
        syn::Meta::Path(path) => path.is_ident("test"),
        syn::Meta::NameValue(_) => false,
        syn::Meta::List(list) if list.path.is_ident("all") => {
            let nested = list.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated);
            nested.is_ok_and(|items| items.iter().any(cfg_meta_requires_test))
        }
        syn::Meta::List(list) if list.path.is_ident("any") => {
            let nested = list.parse_args_with(Punctuated::<syn::Meta, Token![,]>::parse_terminated);
            nested.is_ok_and(|items| !items.is_empty() && items.iter().all(cfg_meta_requires_test))
        }
        syn::Meta::List(_) => false,
    }
}

fn item_attributes(item: &Item) -> Option<&[Attribute]> {
    match item {
        Item::Const(item) => Some(&item.attrs),
        Item::Enum(item) => Some(&item.attrs),
        Item::ExternCrate(item) => Some(&item.attrs),
        Item::Fn(item) => Some(&item.attrs),
        Item::ForeignMod(item) => Some(&item.attrs),
        Item::Impl(item) => Some(&item.attrs),
        Item::Macro(item) => Some(&item.attrs),
        Item::Mod(item) => Some(&item.attrs),
        Item::Static(item) => Some(&item.attrs),
        Item::Struct(item) => Some(&item.attrs),
        Item::Trait(item) => Some(&item.attrs),
        Item::TraitAlias(item) => Some(&item.attrs),
        Item::Type(item) => Some(&item.attrs),
        Item::Union(item) => Some(&item.attrs),
        Item::Use(item) => Some(&item.attrs),
        Item::Verbatim(_) | _ => None,
    }
}

fn impl_item_attributes(item: &ImplItem) -> Option<&[Attribute]> {
    match item {
        ImplItem::Const(item) => Some(&item.attrs),
        ImplItem::Fn(item) => Some(&item.attrs),
        ImplItem::Type(item) => Some(&item.attrs),
        ImplItem::Macro(item) => Some(&item.attrs),
        ImplItem::Verbatim(_) | _ => None,
    }
}

fn trait_item_attributes(item: &TraitItem) -> Option<&[Attribute]> {
    match item {
        TraitItem::Const(item) => Some(&item.attrs),
        TraitItem::Fn(item) => Some(&item.attrs),
        TraitItem::Type(item) => Some(&item.attrs),
        TraitItem::Macro(item) => Some(&item.attrs),
        TraitItem::Verbatim(_) | _ => None,
    }
}

fn foreign_item_attributes(item: &ForeignItem) -> Option<&[Attribute]> {
    match item {
        ForeignItem::Fn(item) => Some(&item.attrs),
        ForeignItem::Static(item) => Some(&item.attrs),
        ForeignItem::Type(item) => Some(&item.attrs),
        ForeignItem::Macro(item) => Some(&item.attrs),
        ForeignItem::Verbatim(_) | _ => None,
    }
}

fn stmt_attributes(statement: &Stmt) -> Option<&[Attribute]> {
    match statement {
        Stmt::Local(statement) => Some(&statement.attrs),
        Stmt::Item(item) => item_attributes(item),
        Stmt::Expr(expression, _) => expr_attributes(expression),
        Stmt::Macro(statement) => Some(&statement.attrs),
    }
}

fn expr_attributes(expression: &Expr) -> Option<&[Attribute]> {
    match expression {
        Expr::Array(expression) => Some(&expression.attrs),
        Expr::Assign(expression) => Some(&expression.attrs),
        Expr::Async(expression) => Some(&expression.attrs),
        Expr::Await(expression) => Some(&expression.attrs),
        Expr::Binary(expression) => Some(&expression.attrs),
        Expr::Block(expression) => Some(&expression.attrs),
        Expr::Break(expression) => Some(&expression.attrs),
        Expr::Call(expression) => Some(&expression.attrs),
        Expr::Cast(expression) => Some(&expression.attrs),
        Expr::Closure(expression) => Some(&expression.attrs),
        Expr::Const(expression) => Some(&expression.attrs),
        Expr::Continue(expression) => Some(&expression.attrs),
        Expr::Field(expression) => Some(&expression.attrs),
        Expr::ForLoop(expression) => Some(&expression.attrs),
        Expr::Group(expression) => Some(&expression.attrs),
        Expr::If(expression) => Some(&expression.attrs),
        Expr::Index(expression) => Some(&expression.attrs),
        Expr::Infer(expression) => Some(&expression.attrs),
        Expr::Let(expression) => Some(&expression.attrs),
        Expr::Lit(expression) => Some(&expression.attrs),
        Expr::Loop(expression) => Some(&expression.attrs),
        Expr::Macro(expression) => Some(&expression.attrs),
        Expr::Match(expression) => Some(&expression.attrs),
        Expr::MethodCall(expression) => Some(&expression.attrs),
        Expr::Paren(expression) => Some(&expression.attrs),
        Expr::Path(expression) => Some(&expression.attrs),
        Expr::Range(expression) => Some(&expression.attrs),
        Expr::RawAddr(expression) => Some(&expression.attrs),
        Expr::Reference(expression) => Some(&expression.attrs),
        Expr::Repeat(expression) => Some(&expression.attrs),
        Expr::Return(expression) => Some(&expression.attrs),
        Expr::Struct(expression) => Some(&expression.attrs),
        Expr::Try(expression) => Some(&expression.attrs),
        Expr::TryBlock(expression) => Some(&expression.attrs),
        Expr::Tuple(expression) => Some(&expression.attrs),
        Expr::Unary(expression) => Some(&expression.attrs),
        Expr::Unsafe(expression) => Some(&expression.attrs),
        Expr::While(expression) => Some(&expression.attrs),
        Expr::Yield(expression) => Some(&expression.attrs),
        Expr::Verbatim(_) | _ => None,
    }
}

fn normalized_path(path: &Path) -> String {
    let mut normalized = PathBuf::new();
    for component in path.components() {
        match component {
            std::path::Component::CurDir => {}
            std::path::Component::ParentDir => {
                if !normalized.pop() {
                    normalized.push("..");
                }
            }
            std::path::Component::Prefix(prefix) => normalized.push(prefix.as_os_str()),
            std::path::Component::RootDir => normalized.push(Path::new("/")),
            std::path::Component::Normal(component) => normalized.push(component),
        }
    }
    normalized.to_string_lossy().replace('\\', "/")
}

#[cfg(test)]
mod tests;
