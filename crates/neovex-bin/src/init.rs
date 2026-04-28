use std::path::Path;

pub(crate) const CONVEX_VERSION: &str = env!("NEOVEX_CONVEX_VERSION");
pub(crate) const CODEGEN_VERSION: &str = env!("NEOVEX_CODEGEN_VERSION");

const SCHEMA_TS: &str = include_str!("../templates/backend/convex/schema.ts");
const MESSAGES_TS: &str = include_str!("../templates/backend/convex/messages.ts");
const GITIGNORE: &str = include_str!("../templates/backend/gitignore");
const TSCONFIG_JSON: &str = include_str!("../templates/backend/tsconfig.json");
const PACKAGE_JSON_TEMPLATE: &str = include_str!("../templates/backend/package.json.tmpl");

pub(crate) fn render_package_json(project_name: &str) -> String {
    PACKAGE_JSON_TEMPLATE
        .replace("{{PROJECT_NAME}}", project_name)
        .replace("{{CONVEX_VERSION}}", CONVEX_VERSION)
        .replace("{{CODEGEN_VERSION}}", CODEGEN_VERSION)
}

struct TemplateFile {
    relative_path: &'static str,
    content: TemplateContent,
}

enum TemplateContent {
    Static(&'static str),
    PackageJson,
}

const BACKEND_TEMPLATE: &[TemplateFile] = &[
    TemplateFile {
        relative_path: "convex/schema.ts",
        content: TemplateContent::Static(SCHEMA_TS),
    },
    TemplateFile {
        relative_path: "convex/messages.ts",
        content: TemplateContent::Static(MESSAGES_TS),
    },
    TemplateFile {
        relative_path: ".gitignore",
        content: TemplateContent::Static(GITIGNORE),
    },
    TemplateFile {
        relative_path: "tsconfig.json",
        content: TemplateContent::Static(TSCONFIG_JSON),
    },
    TemplateFile {
        relative_path: "package.json",
        content: TemplateContent::PackageJson,
    },
];

#[derive(Debug)]
pub(crate) enum ScaffoldAction {
    Created(String),
    Skipped(String),
}

#[derive(Debug)]
pub(crate) struct ScaffoldResult {
    pub(crate) actions: Vec<ScaffoldAction>,
    pub(crate) wrote_package_json: bool,
}

fn is_unsafe_directory(dir: &Path) -> Option<&'static str> {
    let canonical = dir.canonicalize().unwrap_or_else(|_| dir.to_path_buf());

    if let Ok(home) = std::env::var("HOME")
        && canonical
            == Path::new(&home)
                .canonicalize()
                .unwrap_or_else(|_| home.into())
    {
        return Some(
            "Refusing to scaffold into your home directory. \
             Create a project directory first: `mkdir my-app && cd my-app`",
        );
    }

    let path_str = canonical.to_string_lossy();
    if path_str == "/" {
        return Some(
            "Refusing to scaffold into the root directory. \
             Create a project directory first: `mkdir my-app && cd my-app`",
        );
    }
    if path_str == "/tmp" || path_str == "/private/tmp" {
        return Some(
            "Refusing to scaffold into /tmp. \
             Create a project directory first: `mkdir my-app && cd my-app`",
        );
    }
    None
}

pub(crate) fn scaffold_project(target_dir: &Path) -> Result<ScaffoldResult, String> {
    if let Some(msg) = is_unsafe_directory(target_dir) {
        return Err(msg.to_string());
    }

    let project_name = target_dir
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("my-app");

    let mut actions = Vec::new();
    let mut wrote_package_json = false;

    for template in BACKEND_TEMPLATE {
        let dest = target_dir.join(template.relative_path);

        if dest.exists() {
            actions.push(ScaffoldAction::Skipped(template.relative_path.to_string()));
            continue;
        }

        if let Some(parent) = dest.parent()
            && !parent.exists()
        {
            std::fs::create_dir_all(parent)
                .map_err(|e| format!("failed to create {}: {e}", parent.display()))?;
        }

        let content = match &template.content {
            TemplateContent::Static(s) => (*s).to_string(),
            TemplateContent::PackageJson => {
                wrote_package_json = true;
                render_package_json(project_name)
            }
        };

        std::fs::write(&dest, content)
            .map_err(|e| format!("failed to write {}: {e}", dest.display()))?;

        actions.push(ScaffoldAction::Created(template.relative_path.to_string()));
    }

    Ok(ScaffoldResult {
        actions,
        wrote_package_json,
    })
}

pub(crate) fn check_source_root_flag(source_root: &str) -> Result<(), String> {
    if source_root == "neovex" {
        return Err(
            "The neovex/ source root is experimental and not yet supported \
             by the scaffold templates."
                .to_string(),
        );
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn compile_time_versions_are_populated() {
        assert!(
            !CONVEX_VERSION.is_empty(),
            "NEOVEX_CONVEX_VERSION should be set by build.rs"
        );
        assert!(
            !CODEGEN_VERSION.is_empty(),
            "NEOVEX_CODEGEN_VERSION should be set by build.rs"
        );
        assert!(
            CONVEX_VERSION.contains('.'),
            "NEOVEX_CONVEX_VERSION should be a semver string, got: {CONVEX_VERSION}"
        );
        assert!(
            CODEGEN_VERSION.contains('.'),
            "NEOVEX_CODEGEN_VERSION should be a semver string, got: {CODEGEN_VERSION}"
        );
    }

    #[test]
    fn package_json_template_substitution() {
        let rendered = render_package_json("my-app");
        assert!(
            rendered.contains(&format!("\"convex\": \"^{CONVEX_VERSION}\"")),
            "rendered package.json should contain convex version"
        );
        assert!(
            rendered.contains(&format!("\"@neovex/codegen\": \"^{CODEGEN_VERSION}\"")),
            "rendered package.json should contain codegen version"
        );
        assert!(
            rendered.contains("\"name\": \"my-app\""),
            "rendered package.json should contain the project name"
        );
        assert!(
            !rendered.contains("{{"),
            "rendered package.json should not contain unresolved placeholders"
        );
    }

    #[test]
    fn scaffold_writes_all_files_to_empty_dir() {
        let tmp = tempfile::tempdir().unwrap();
        let result = scaffold_project(tmp.path()).unwrap();

        assert_eq!(result.actions.len(), 5);
        for action in &result.actions {
            assert!(
                matches!(action, ScaffoldAction::Created(_)),
                "all files should be created in empty dir"
            );
        }
        assert!(result.wrote_package_json);

        assert!(tmp.path().join("convex/schema.ts").exists());
        assert!(tmp.path().join("convex/messages.ts").exists());
        assert!(tmp.path().join(".gitignore").exists());
        assert!(tmp.path().join("tsconfig.json").exists());
        assert!(tmp.path().join("package.json").exists());

        let pkg = std::fs::read_to_string(tmp.path().join("package.json")).unwrap();
        assert!(
            pkg.contains(&format!("\"convex\": \"^{CONVEX_VERSION}\"")),
            "package.json should have substituted convex version"
        );
        assert!(
            !pkg.contains("{{"),
            "package.json should not have unresolved placeholders"
        );
    }

    #[test]
    fn scaffold_skips_existing_files() {
        let tmp = tempfile::tempdir().unwrap();

        std::fs::write(tmp.path().join("package.json"), "{}").unwrap();
        std::fs::write(tmp.path().join("tsconfig.json"), "{}").unwrap();
        std::fs::write(tmp.path().join(".gitignore"), "node_modules/\n").unwrap();

        let result = scaffold_project(tmp.path()).unwrap();

        let created: Vec<_> = result
            .actions
            .iter()
            .filter_map(|a| match a {
                ScaffoldAction::Created(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();
        let skipped: Vec<_> = result
            .actions
            .iter()
            .filter_map(|a| match a {
                ScaffoldAction::Skipped(p) => Some(p.as_str()),
                _ => None,
            })
            .collect();

        assert!(created.contains(&"convex/schema.ts"));
        assert!(created.contains(&"convex/messages.ts"));
        assert!(skipped.contains(&"package.json"));
        assert!(skipped.contains(&"tsconfig.json"));
        assert!(skipped.contains(&".gitignore"));
        assert!(!result.wrote_package_json);

        assert_eq!(
            std::fs::read_to_string(tmp.path().join("package.json")).unwrap(),
            "{}",
            "existing package.json should not be overwritten"
        );
    }

    #[test]
    fn scaffold_refuses_home_directory() {
        let tmp = tempfile::tempdir().unwrap();
        let original_home = std::env::var("HOME").ok();
        // SAFETY: this test runs single-threaded for env mutation; restored below.
        unsafe { std::env::set_var("HOME", tmp.path()) };
        let result = scaffold_project(tmp.path());
        match original_home {
            Some(h) => unsafe { std::env::set_var("HOME", h) },
            None => unsafe { std::env::remove_var("HOME") },
        }
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("home directory"),
            "should mention home directory"
        );
    }

    #[test]
    fn scaffold_refuses_root_directory() {
        let result = scaffold_project(Path::new("/"));
        assert!(result.is_err());
        assert!(
            result.unwrap_err().contains("root directory"),
            "should mention root directory"
        );
    }

    #[test]
    fn scaffold_refuses_tmp_directory() {
        let result = scaffold_project(Path::new("/tmp"));
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("/tmp"), "should mention /tmp");
    }

    #[test]
    fn source_root_neovex_returns_advisory() {
        let result = check_source_root_flag("neovex");
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("experimental"));
    }

    #[test]
    fn source_root_convex_is_accepted() {
        assert!(check_source_root_flag("convex").is_ok());
    }
}
