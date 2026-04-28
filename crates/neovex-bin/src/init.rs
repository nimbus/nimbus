pub(crate) const CONVEX_VERSION: &str = env!("NEOVEX_CONVEX_VERSION");
pub(crate) const CODEGEN_VERSION: &str = env!("NEOVEX_CODEGEN_VERSION");

pub(crate) const PACKAGE_JSON_TEMPLATE: &str =
    include_str!("../templates/backend/package.json.tmpl");

pub(crate) fn render_package_json(project_name: &str) -> String {
    PACKAGE_JSON_TEMPLATE
        .replace("{{PROJECT_NAME}}", project_name)
        .replace("{{CONVEX_VERSION}}", CONVEX_VERSION)
        .replace("{{CODEGEN_VERSION}}", CODEGEN_VERSION)
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
}
