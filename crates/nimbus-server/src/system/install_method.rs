use std::path::{Path, PathBuf};

use serde::Serialize;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "kebab-case")]
pub(crate) enum InstallMethod {
    Brew,
    Apt,
    Dnf,
    InstallScript,
    Source,
    Unknown,
}

impl InstallMethod {
    pub(crate) fn as_str(self) -> &'static str {
        match self {
            Self::Brew => "brew",
            Self::Apt => "apt",
            Self::Dnf => "dnf",
            Self::InstallScript => "install-script",
            Self::Source => "source",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub(crate) struct UpgradeAction {
    pub method: InstallMethod,
    pub command: Option<&'static str>,
    pub needs_sudo: bool,
    pub interactive: bool,
    pub fallback_url: &'static str,
}

const FALLBACK_INSTALL: &str = "https://github.com/nimbus/nimbus#install";
const FALLBACK_SOURCE: &str = "https://github.com/nimbus/nimbus#build-from-source";

const BREW_PREFIXES: &[&str] = &[
    "/opt/homebrew/",
    "/usr/local/Homebrew/",
    "/usr/local/Cellar/",
    "/home/linuxbrew/",
];

const INSTALL_SCRIPT_SUFFIXES: &[&str] = &[".local/bin/nimbus", ".nimbus/bin/nimbus"];

pub(crate) fn detect_install_method(current_exe: &Path) -> UpgradeAction {
    let path_str = canonicalize_for_match(current_exe);
    classify(&path_str)
}

fn canonicalize_for_match(current_exe: &Path) -> String {
    current_exe
        .canonicalize()
        .unwrap_or_else(|_| current_exe.to_path_buf())
        .to_string_lossy()
        .into_owned()
}

fn classify(path: &str) -> UpgradeAction {
    if BREW_PREFIXES.iter().any(|prefix| path.starts_with(prefix)) {
        return UpgradeAction {
            method: InstallMethod::Brew,
            command: Some("brew upgrade --cask nimbus/tap/nimbus"),
            needs_sudo: false,
            interactive: true,
            fallback_url: FALLBACK_INSTALL,
        };
    }

    if is_cargo_target_build(path) || is_cargo_install_path(path) {
        return UpgradeAction {
            method: InstallMethod::Source,
            command: None,
            needs_sudo: false,
            interactive: false,
            fallback_url: FALLBACK_SOURCE,
        };
    }

    if matches_install_script_path(path) {
        return UpgradeAction {
            method: InstallMethod::InstallScript,
            command: Some("curl -fsSL https://nimbus.dev/install.sh | sh"),
            needs_sudo: false,
            interactive: true,
            fallback_url: FALLBACK_INSTALL,
        };
    }

    if is_system_bin_path(path) {
        if path_managed_by_dpkg(path) {
            return UpgradeAction {
                method: InstallMethod::Apt,
                command: Some("sudo apt update && sudo apt upgrade nimbus"),
                needs_sudo: true,
                interactive: true,
                fallback_url: FALLBACK_INSTALL,
            };
        }
        if path_managed_by_rpm(path) {
            return UpgradeAction {
                method: InstallMethod::Dnf,
                command: Some("sudo dnf upgrade nimbus"),
                needs_sudo: true,
                interactive: true,
                fallback_url: FALLBACK_INSTALL,
            };
        }
    }

    UpgradeAction {
        method: InstallMethod::Unknown,
        command: None,
        needs_sudo: false,
        interactive: false,
        fallback_url: FALLBACK_INSTALL,
    }
}

fn is_cargo_target_build(path: &str) -> bool {
    path.contains("/target/debug/") || path.contains("/target/release/")
}

fn is_cargo_install_path(path: &str) -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        let home = PathBuf::from(home).join(".cargo").join("bin");
        if path.starts_with(home.to_string_lossy().as_ref()) {
            return true;
        }
    }
    path.contains("/.cargo/bin/")
}

fn matches_install_script_path(path: &str) -> bool {
    if let Some(home) = std::env::var_os("HOME") {
        let home_str = PathBuf::from(home).to_string_lossy().into_owned();
        for suffix in INSTALL_SCRIPT_SUFFIXES {
            let candidate = format!("{home_str}/{suffix}");
            if path == candidate {
                return true;
            }
        }
    }
    INSTALL_SCRIPT_SUFFIXES
        .iter()
        .any(|suffix| path.ends_with(suffix))
}

fn is_system_bin_path(path: &str) -> bool {
    path.starts_with("/usr/bin/") || path.starts_with("/usr/local/bin/")
}

#[cfg(target_os = "linux")]
fn path_managed_by_dpkg(path: &str) -> bool {
    package_query(path, "dpkg", &["-S"])
}

#[cfg(not(target_os = "linux"))]
fn path_managed_by_dpkg(_path: &str) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn path_managed_by_rpm(path: &str) -> bool {
    package_query(path, "rpm", &["-qf"])
}

#[cfg(not(target_os = "linux"))]
fn path_managed_by_rpm(_path: &str) -> bool {
    false
}

#[cfg(target_os = "linux")]
fn package_query(path: &str, program: &str, args: &[&str]) -> bool {
    use std::process::Command;
    let Ok(output) = Command::new(program).args(args).arg(path).output() else {
        return false;
    };
    output.status.success()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::Path;

    fn classify_path(path: &str) -> UpgradeAction {
        classify(path)
    }

    #[test]
    fn brew_prefix_apple_silicon() {
        let action = classify_path("/opt/homebrew/Cellar/nimbus/0.1.31/bin/nimbus");
        assert_eq!(action.method, InstallMethod::Brew);
        assert_eq!(
            action.command,
            Some("brew upgrade --cask nimbus/tap/nimbus")
        );
        assert!(!action.needs_sudo);
        assert!(action.interactive);
        assert_eq!(action.fallback_url, FALLBACK_INSTALL);
    }

    #[test]
    fn brew_prefix_intel() {
        let action = classify_path("/usr/local/Homebrew/Cellar/nimbus/0.1.31/bin/nimbus");
        assert_eq!(action.method, InstallMethod::Brew);
    }

    #[test]
    fn brew_prefix_linuxbrew() {
        let action = classify_path("/home/linuxbrew/.linuxbrew/Cellar/nimbus/0.1.31/bin/nimbus");
        assert_eq!(action.method, InstallMethod::Brew);
    }

    #[test]
    fn cargo_target_debug_is_source() {
        let action = classify_path("/Users/jack/src/nimbus/target/debug/nimbus");
        assert_eq!(action.method, InstallMethod::Source);
        assert_eq!(action.command, None);
        assert_eq!(action.fallback_url, FALLBACK_SOURCE);
    }

    #[test]
    fn cargo_target_release_is_source() {
        let action = classify_path("/Users/jack/src/nimbus/target/release/nimbus");
        assert_eq!(action.method, InstallMethod::Source);
    }

    #[test]
    fn cargo_install_path_is_source() {
        let action = classify_path("/Users/jack/.cargo/bin/nimbus");
        assert_eq!(action.method, InstallMethod::Source);
    }

    #[test]
    fn install_script_local_bin() {
        let action = classify_path("/home/dev/.local/bin/nimbus");
        assert_eq!(action.method, InstallMethod::InstallScript);
        assert_eq!(
            action.command,
            Some("curl -fsSL https://nimbus.dev/install.sh | sh")
        );
        assert!(!action.needs_sudo);
    }

    #[test]
    fn install_script_nimbus_bin() {
        let action = classify_path("/home/dev/.nimbus/bin/nimbus");
        assert_eq!(action.method, InstallMethod::InstallScript);
    }

    #[test]
    fn unknown_when_outside_known_prefixes() {
        let action = classify_path("/opt/custom/bin/nimbus");
        assert_eq!(action.method, InstallMethod::Unknown);
        assert_eq!(action.command, None);
        assert_eq!(action.fallback_url, FALLBACK_INSTALL);
    }

    #[test]
    #[cfg(not(target_os = "linux"))]
    fn usr_bin_off_linux_is_unknown() {
        let action = classify_path("/usr/bin/nimbus");
        assert_eq!(action.method, InstallMethod::Unknown);
    }

    #[test]
    fn install_method_as_str_strings_match_decision_doc() {
        assert_eq!(InstallMethod::Brew.as_str(), "brew");
        assert_eq!(InstallMethod::Apt.as_str(), "apt");
        assert_eq!(InstallMethod::Dnf.as_str(), "dnf");
        assert_eq!(InstallMethod::InstallScript.as_str(), "install-script");
        assert_eq!(InstallMethod::Source.as_str(), "source");
        assert_eq!(InstallMethod::Unknown.as_str(), "unknown");
    }

    #[test]
    fn detect_install_method_does_not_panic_on_unknown_path() {
        let action = detect_install_method(Path::new("/nonexistent/path/nimbus"));
        assert_eq!(action.method, InstallMethod::Unknown);
    }
}
