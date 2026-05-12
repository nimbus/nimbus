use std::collections::BTreeMap;
use std::fs;
use std::net::IpAddr;
use std::path::{Path, PathBuf};
use std::time::Duration;

use nimbus::{
    Error, PublishedEndpointProtocol, SandboxBackendKind, SandboxBuildLaunchSpec,
    SandboxFilesystemSpec, SandboxImageLaunchSpec, SandboxImageProcessOverrides,
    SandboxLifecycleSpec, SandboxPortBinding, SandboxProcessSpec, SandboxRestartPolicy,
    SandboxServiceCatalog, SandboxServiceLaunch, SandboxSpec, TenantId,
};
use serde::{Deserialize, Serialize};
use serde_yaml::Value;

mod lower;
mod parse;
mod raw;
mod render;
#[cfg(test)]
mod tests;
mod warnings;

#[cfg(test)]
pub(crate) use self::render::render_compose_project;
pub(crate) use self::render::render_compose_project_selection;

pub(crate) const DEFAULT_COMPOSE_FILE: &str = "compose.yaml";
const CONFIG_VALIDATION_TENANT_ID: &str = "compose-config";

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct RenderedComposeProject {
    pub(crate) stdout: String,
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeProjectPlan {
    pub(crate) source_file: PathBuf,
    pub(crate) project_name: String,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) volumes: Vec<String>,
    pub(crate) services: BTreeMap<String, ComposeServicePlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct ComposeServiceCatalog {
    project: ComposeProjectPlan,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeServicePlan {
    pub(crate) backend: SandboxBackendKind,
    pub(crate) source: ComposeLaunchPlan,
    pub(crate) process: ComposeProcessPlan,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) ports: Vec<ComposePortBindingPlan>,
    pub(crate) resources: ComposeResourcePlan,
    pub(crate) restart: ComposeRestartPlan,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) depends_on: BTreeMap<String, ComposeDependencyCondition>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) healthcheck: Option<ComposeHealthcheckPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) stop_grace_period: Option<String>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) labels: BTreeMap<String, String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) volumes: Vec<ComposeVolumeMountPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) x_nimbus: Option<ComposeNimbusPlan>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub(crate) warnings: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case", tag = "kind")]
pub(crate) enum ComposeLaunchPlan {
    Image {
        image_reference: String,
    },
    Build {
        image_name: String,
        dockerfile_path: PathBuf,
        context_path: PathBuf,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeProcessPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) entrypoint: Option<ComposeCommandPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) command: Option<ComposeCommandPlan>,
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub(crate) environment: BTreeMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) working_dir: Option<PathBuf>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) user: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(untagged)]
pub(crate) enum ComposeCommandPlan {
    String(String),
    List(Vec<String>),
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposePortBindingPlan {
    pub(crate) name: String,
    pub(crate) protocol: PublishedEndpointProtocol,
    pub(crate) host_address: IpAddr,
    pub(crate) host_port: u16,
    pub(crate) guest_port: u16,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeResourcePlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) requested_cpus: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) cpu_count: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) requested_memory: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) memory_limit_bytes: Option<u64>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeRestartPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) requested: Option<String>,
    pub(crate) policy: SandboxRestartPolicy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub(crate) enum ComposeDependencyCondition {
    ServiceStarted,
    ServiceHealthy,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeHealthcheckPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) test: Option<ComposeCommandPlan>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) interval: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) retries: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) start_period: Option<String>,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) disable: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub(crate) struct ComposeVolumeMountPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) source: Option<String>,
    pub(crate) target: String,
    pub(crate) kind: String,
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub(crate) read_only: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub(crate) struct ComposeNimbusPlan {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) backend: Option<SandboxBackendKind>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) idle_timeout: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub(crate) snapshot: Option<bool>,
}
