use std::io;
use std::path::{Path, PathBuf};

use crate::node;

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) enum DevAdapter {
    Convex { source_root: PathBuf },
    CloudFunctions { source_roots: Vec<PathBuf> },
}

impl DevAdapter {
    pub(super) fn adapter(&self) -> node::Adapter {
        match self {
            Self::Convex { .. } => node::Adapter::Convex,
            Self::CloudFunctions { .. } => node::Adapter::CloudFunctions,
        }
    }

    pub(super) fn name(&self) -> &'static str {
        self.adapter().name()
    }

    pub(super) fn source_roots(&self) -> &[PathBuf] {
        match self {
            Self::Convex { source_root } => std::slice::from_ref(source_root),
            Self::CloudFunctions { source_roots } => source_roots,
        }
    }

    pub(super) fn needs_node_dependencies(&self) -> bool {
        self.adapter().needs_node_dependencies()
    }

    pub(super) fn npm_install_dirs(&self, app_dir: &Path) -> Vec<PathBuf> {
        match self {
            Self::Convex { .. } => vec![app_dir.to_path_buf()],
            Self::CloudFunctions { source_roots } => source_roots.clone(),
        }
    }
}

pub(super) fn detect_dev_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    let nimbus_root = app_dir.join("nimbus");
    if nimbus_root.is_dir() {
        return Ok(Some(DevAdapter::Convex {
            source_root: nimbus_root,
        }));
    }

    let convex_root = app_dir.join("convex");
    if convex_root.is_dir() {
        return Ok(Some(DevAdapter::Convex {
            source_root: convex_root,
        }));
    }

    if let Some(adapter) = detect_cloud_functions_adapter(app_dir)? {
        return Ok(Some(adapter));
    }

    Ok(None)
}

fn detect_cloud_functions_adapter(app_dir: &Path) -> io::Result<Option<DevAdapter>> {
    if let Some(project) = node::firebase_functions_project(app_dir)? {
        return Ok(Some(DevAdapter::CloudFunctions {
            source_roots: project.source_dirs(),
        }));
    }

    if has_functions_framework_dependency(app_dir) {
        return Ok(Some(DevAdapter::CloudFunctions {
            source_roots: vec![app_dir.to_path_buf()],
        }));
    }

    Ok(None)
}

fn has_functions_framework_dependency(app_dir: &Path) -> bool {
    let package_json_path = app_dir.join("package.json");
    let Ok(content) = std::fs::read_to_string(&package_json_path) else {
        return false;
    };
    let Ok(parsed) = serde_json::from_str::<serde_json::Value>(&content) else {
        return false;
    };
    let package_name = "@google-cloud/functions-framework";
    for key in [
        "dependencies",
        "devDependencies",
        "optionalDependencies",
        "peerDependencies",
    ] {
        if parsed[key].get(package_name).is_some() {
            return true;
        }
    }
    false
}
