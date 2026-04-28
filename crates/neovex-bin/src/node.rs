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

/// Install Node.js dependencies when `package.json` exists but `node_modules/`
/// does not.
pub(crate) async fn auto_install_node_dependencies(
    app_dir: &Path,
) -> Result<(), Box<dyn std::error::Error>> {
    if !app_dir.join("package.json").is_file() || app_dir.join("node_modules").is_dir() {
        return Ok(());
    }

    cli_ux::write_stderr_prefixed_line("info:", "running npm install")?;
    let status = tokio::process::Command::new("npm")
        .arg("install")
        .current_dir(app_dir)
        .status()
        .await
        .map_err(|e| io::Error::other(format!("failed to run npm install: {e}")))?;

    if !status.success() {
        return Err(io::Error::other(
            "npm install failed. Install dependencies manually and try again.",
        )
        .into());
    }
    Ok(())
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
    async fn auto_install_skips_when_node_modules_exists() {
        let temp = tempfile::tempdir().unwrap();
        std::fs::write(temp.path().join("package.json"), "{}").unwrap();
        std::fs::create_dir(temp.path().join("node_modules")).unwrap();
        auto_install_node_dependencies(temp.path())
            .await
            .expect("should be a no-op when node_modules exists");
    }
}
