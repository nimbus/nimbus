use std::collections::BTreeSet;
use std::io;
use std::path::Path;

use crate::cli_ux;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Adapter {
    Convex,
    CloudFunctions,
}

impl Adapter {
    pub(crate) fn from_cli_arg(s: &str) -> Option<Adapter> {
        match s {
            "convex" => Some(Self::Convex),
            "cloud-functions" => Some(Self::CloudFunctions),
            _ => None,
        }
    }

    pub(crate) fn name(self) -> &'static str {
        match self {
            Self::Convex => "convex",
            Self::CloudFunctions => "cloud-functions",
        }
    }

    pub(crate) fn needs_node_dependencies(self) -> bool {
        match self {
            Self::Convex | Self::CloudFunctions => true,
        }
    }
}

/// Install Node.js dependencies when declared authoring packages are missing.
pub(crate) async fn auto_install_node_dependencies(
    app_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let missing_packages = missing_required_node_packages(app_dir)?;
    if missing_packages.is_empty() {
        return Ok(());
    }

    cli_ux::write_stderr_prefixed_line("info:", "running npm install")?;
    let status = tokio::process::Command::new("npm")
        .arg("install")
        .current_dir(app_dir)
        .status()
        .await
        .map_err(|e| {
            io::Error::other(format!(
                "failed to run npm install in {}: {e}. Install Node.js with npm to use Neovex authoring flows.",
                app_dir.display()
            ))
        })?;

    if !status.success() {
        return Err(io::Error::other(format!(
            "npm install failed in {}. Resolve the npm error above, then rerun the Neovex command or run `npm install` manually.",
            app_dir.display()
        ))
        .into());
    }
    Ok(())
}

fn missing_required_node_packages(app_dir: &Path) -> io::Result<Vec<String>> {
    let package_json_path = app_dir.join("package.json");
    if !package_json_path.is_file() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&package_json_path).map_err(|error| {
        io::Error::new(
            error.kind(),
            format!(
                "failed to read package.json at {}: {error}",
                package_json_path.display()
            ),
        )
    })?;
    let parsed: serde_json::Value = serde_json::from_str(&content).map_err(|error| {
        io::Error::other(format!(
            "package.json at {} is not valid JSON: {error}",
            package_json_path.display()
        ))
    })?;

    let mut packages = BTreeSet::new();
    collect_dependency_names(&parsed, "dependencies", &package_json_path, &mut packages)?;
    collect_dependency_names(
        &parsed,
        "devDependencies",
        &package_json_path,
        &mut packages,
    )?;

    Ok(packages
        .into_iter()
        .filter(|package_name| !node_package_manifest_path(app_dir, package_name).is_file())
        .collect())
}

fn collect_dependency_names(
    parsed: &serde_json::Value,
    field_name: &str,
    package_json_path: &Path,
    packages: &mut BTreeSet<String>,
) -> io::Result<()> {
    let Some(value) = parsed.get(field_name) else {
        return Ok(());
    };
    if value.is_null() {
        return Ok(());
    }
    let Some(object) = value.as_object() else {
        return Err(io::Error::other(format!(
            "package.json field `{field_name}` at {} must be an object",
            package_json_path.display()
        )));
    };
    packages.extend(object.keys().cloned());
    Ok(())
}

fn node_package_manifest_path(app_dir: &Path, package_name: &str) -> std::path::PathBuf {
    app_dir
        .join("node_modules")
        .join(package_name)
        .join("package.json")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn adapter_from_cli_arg_convex() {
        assert_eq!(Adapter::from_cli_arg("convex"), Some(Adapter::Convex));
    }

    #[test]
    fn adapter_from_cli_arg_cloud_functions() {
        assert_eq!(
            Adapter::from_cli_arg("cloud-functions"),
            Some(Adapter::CloudFunctions)
        );
    }

    #[test]
    fn adapter_from_cli_arg_unknown() {
        assert_eq!(Adapter::from_cli_arg("unknown"), None);
    }

    #[test]
    fn adapter_round_trips_through_name() {
        for adapter in [Adapter::Convex, Adapter::CloudFunctions] {
            assert_eq!(Adapter::from_cli_arg(adapter.name()), Some(adapter));
        }
    }

    #[test]
    fn all_adapters_need_node_dependencies() {
        assert!(Adapter::Convex.needs_node_dependencies());
        assert!(Adapter::CloudFunctions.needs_node_dependencies());
    }

    #[tokio::test]
    async fn auto_install_skips_when_no_package_json() {
        let temp = tempfile::tempdir().unwrap();
        auto_install_node_dependencies(temp.path())
            .await
            .expect("should be a no-op without package.json");
        assert!(
            !temp.path().join("node_modules").exists(),
            "should not create node_modules"
        );
    }

    #[tokio::test]
    async fn auto_install_skips_when_no_packages_are_declared() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        auto_install_node_dependencies(temp.path())
            .await
            .expect("should be a no-op when no packages are declared");
    }

    #[test]
    fn missing_required_node_packages_reports_declared_packages_without_install() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  },
  "devDependencies": {
    "@neovex/codegen": "^1.0.0"
  }
}"#,
        )
        .unwrap();

        let missing = missing_required_node_packages(temp.path()).unwrap();

        assert_eq!(
            missing,
            vec!["@neovex/codegen".to_string(), "convex".to_string()]
        );
    }

    #[test]
    fn missing_required_node_packages_ignores_installed_packages() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  },
  "devDependencies": {
    "@neovex/codegen": "^1.0.0"
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules/convex")).unwrap();
        std::fs::write(temp.path().join("node_modules/convex/package.json"), "{}").unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules/@neovex/codegen")).unwrap();
        std::fs::write(
            temp.path()
                .join("node_modules/@neovex/codegen/package.json"),
            "{}",
        )
        .unwrap();

        let missing = missing_required_node_packages(temp.path()).unwrap();

        assert!(
            missing.is_empty(),
            "all declared packages should be present"
        );
    }

    #[test]
    fn missing_required_node_packages_does_not_trust_node_modules_directory_alone() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(
            temp.path().join("package.json"),
            r#"{
  "dependencies": {
    "convex": "^1.0.0"
  }
}"#,
        )
        .unwrap();
        std::fs::create_dir_all(temp.path().join("node_modules")).unwrap();

        let missing = missing_required_node_packages(temp.path()).unwrap();

        assert_eq!(missing, vec!["convex".to_string()]);
    }

    #[test]
    fn missing_required_node_packages_errors_on_invalid_package_json() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("package.json"), "{not-json").unwrap();

        let error = missing_required_node_packages(temp.path()).unwrap_err();

        assert!(
            error.to_string().contains("not valid JSON"),
            "error should mention invalid JSON, got: {error}"
        );
    }
}
