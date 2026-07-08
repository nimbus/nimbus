//! Wire-facing sandbox spec conversion shared by the sandbox and service
//! (sandbox-backed) resource orchestration. Reference shape for CP3: the
//! request/response DTOs live beside their conversion logic in
//! `nimbus-compute` rather than the transport crate, since neither carries
//! any HTTP-framework dependency.

use std::path::PathBuf;

use nimbus_core::{Error, TenantId};
use nimbus_sandbox::{
    SandboxBackendKind, SandboxOciImageReferenceSpec, SandboxOciImageSource, SandboxOwnerSpec,
    SandboxProcessSpec, SandboxRootSpec, SandboxSpec,
};
use nimbus_system::user_tenant_id;
use serde::{Deserialize, Serialize};

use crate::state::ComputeError;

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
pub struct SandboxSpecInput {
    tenant_id: Option<String>,
    owner: SandboxOwnerInput,
    backend: SandboxBackendInput,
    root: SandboxRootInput,
    process: SandboxProcessInput,
}

impl SandboxSpecInput {
    pub fn into_spec(
        self,
        default_tenant_id: &TenantId,
        default_service_name: Option<&str>,
    ) -> Result<SandboxSpec, ComputeError> {
        let tenant_id = match self.tenant_id {
            Some(tenant_id) => {
                let tenant_id = user_tenant_id(tenant_id).map_err(ComputeError::from)?;
                if &tenant_id != default_tenant_id {
                    return Err(ComputeError::from(Error::InvalidInput(format!(
                        "sandbox spec tenantId `{tenant_id}` must match route tenant `{default_tenant_id}`"
                    ))));
                }
                tenant_id
            }
            None => default_tenant_id.clone(),
        };
        Ok(SandboxSpec::new(
            tenant_id,
            self.owner.into_owner(default_service_name)?,
            self.backend.into_backend(),
            self.root.into_root()?,
            self.process.into_process(),
        ))
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SandboxSpecResponse {
    tenant_id: String,
    owner: SandboxOwnerResponse,
    backend: &'static str,
    root: SandboxRootResponse,
    process: SandboxProcessResponse,
}

impl SandboxSpecResponse {
    pub fn from_spec(spec: SandboxSpec) -> Self {
        Self {
            tenant_id: spec.tenant_id.as_str().to_owned(),
            owner: SandboxOwnerResponse::from_owner(spec.owner),
            backend: backend_kind_response(spec.backend),
            root: SandboxRootResponse::from_root(spec.root),
            process: SandboxProcessResponse::from_process(spec.process),
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "camelCase", deny_unknown_fields)]
enum SandboxOwnerInput {
    #[serde(rename = "service")]
    Service {
        #[serde(rename = "serviceName")]
        service_name: Option<String>,
    },
    #[serde(rename = "standalone")]
    Standalone {
        #[serde(rename = "displayName")]
        display_name: Option<String>,
    },
}

impl SandboxOwnerInput {
    fn into_owner(
        self,
        default_service_name: Option<&str>,
    ) -> Result<SandboxOwnerSpec, ComputeError> {
        match self {
            Self::Service { service_name } => {
                let service_name = service_name
                    .or_else(|| default_service_name.map(str::to_owned))
                    .ok_or_else(|| {
                        ComputeError::from(Error::InvalidInput(
                            "service-owned sandbox specs require owner.serviceName".to_owned(),
                        ))
                    })?;
                Ok(SandboxOwnerSpec::service(service_name))
            }
            Self::Standalone { display_name } => Ok(display_name
                .map(SandboxOwnerSpec::standalone_named)
                .unwrap_or_else(SandboxOwnerSpec::standalone)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "camelCase")]
enum SandboxOwnerResponse {
    #[serde(rename = "service")]
    Service {
        #[serde(rename = "serviceName")]
        service_name: String,
    },
    #[serde(rename = "standalone")]
    Standalone {
        #[serde(rename = "displayName")]
        #[serde(skip_serializing_if = "Option::is_none")]
        display_name: Option<String>,
    },
}

impl SandboxOwnerResponse {
    fn from_owner(owner: SandboxOwnerSpec) -> Self {
        match owner {
            SandboxOwnerSpec::Service { name } => Self::Service { service_name: name },
            SandboxOwnerSpec::Standalone { display_name } => Self::Standalone { display_name },
        }
    }
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "snake_case")]
enum SandboxBackendInput {
    Container,
    Krun,
}

impl SandboxBackendInput {
    fn into_backend(self) -> SandboxBackendKind {
        match self {
            Self::Container => SandboxBackendKind::Container,
            Self::Krun => SandboxBackendKind::Krun,
        }
    }
}

fn backend_kind_response(kind: SandboxBackendKind) -> &'static str {
    match kind {
        SandboxBackendKind::Container => "container",
        SandboxBackendKind::Krun => "krun",
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SandboxRootInput {
    Rootfs {
        rootfs: PathBuf,
        #[serde(default)]
        readonly: bool,
    },
    OciImage {
        source: SandboxOciImageSourceInput,
    },
}

impl SandboxRootInput {
    fn into_root(self) -> Result<SandboxRootSpec, ComputeError> {
        match self {
            Self::Rootfs { rootfs, readonly } => {
                Err(ComputeError::from(Error::InvalidInput(format!(
                    "public sandbox specs must use root.kind `oci_image`; host rootfs path `{}` readonly={} is an operator-only internal input",
                    rootfs.display(),
                    readonly
                ))))
            }
            Self::OciImage { source } => Ok(SandboxRootSpec::oci_image(source.into_source()?)),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SandboxRootResponse {
    OciImage {
        source: SandboxOciImageSourceResponse,
    },
    Redacted {
        redacted: bool,
        reason: &'static str,
    },
}

impl SandboxRootResponse {
    fn from_root(root: SandboxRootSpec) -> Self {
        match root {
            SandboxRootSpec::Rootfs(_) => redacted_root_response(),
            SandboxRootSpec::OciImage(image) => match image.source {
                SandboxOciImageSource::Reference(reference) => Self::OciImage {
                    source: SandboxOciImageSourceResponse::from_reference(reference),
                },
                SandboxOciImageSource::Build(_) => redacted_root_response(),
            },
        }
    }
}

fn redacted_root_response() -> SandboxRootResponse {
    SandboxRootResponse::Redacted {
        redacted: true,
        reason: "operatorOnlyLaunchInput",
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
enum SandboxOciImageSourceInput {
    Reference {
        reference: String,
    },
    Build {
        #[serde(rename = "imageName")]
        image_name: String,
        #[serde(rename = "dockerfilePath")]
        dockerfile_path: PathBuf,
        #[serde(rename = "contextPath")]
        context_path: PathBuf,
    },
}

impl SandboxOciImageSourceInput {
    fn into_source(self) -> Result<SandboxOciImageSource, ComputeError> {
        match self {
            Self::Reference { reference } => Ok(SandboxOciImageSource::Reference(
                SandboxOciImageReferenceSpec::new(reference),
            )),
            Self::Build {
                image_name,
                dockerfile_path,
                context_path,
            } => Err(ComputeError::from(Error::InvalidInput(format!(
                "public sandbox specs must use an admitted OCI image reference; local build context paths for image `{image_name}` at dockerfile `{}` and context `{}` are operator-only internal inputs",
                dockerfile_path.display(),
                context_path.display()
            )))),
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
enum SandboxOciImageSourceResponse {
    Reference { reference: String },
}

impl SandboxOciImageSourceResponse {
    fn from_reference(reference: SandboxOciImageReferenceSpec) -> Self {
        Self::Reference {
            reference: reference.reference,
        }
    }
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase", deny_unknown_fields)]
struct SandboxProcessInput {
    argv: Option<Vec<String>>,
    args: Option<Vec<String>>,
    entrypoint: Option<Vec<String>>,
    command: Option<Vec<String>>,
    env: Option<Vec<String>>,
    cwd: Option<PathBuf>,
    user: Option<String>,
    terminal: Option<bool>,
}

impl SandboxProcessInput {
    fn into_process(self) -> SandboxProcessSpec {
        let mut process = SandboxProcessSpec::new(self.argv.or(self.args).unwrap_or_default());
        if let Some(entrypoint) = self.entrypoint {
            process.entrypoint = Some(entrypoint);
        }
        if let Some(command) = self.command {
            process.command = Some(command);
        }
        if let Some(env) = self.env {
            process.env = env;
        }
        if let Some(cwd) = self.cwd {
            process.cwd = cwd;
        }
        if let Some(user) = self.user {
            process.user = Some(user);
        }
        if let Some(terminal) = self.terminal {
            process.terminal = terminal;
        }
        process
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SandboxProcessResponse {
    argv: RedactedValuesResponse,
    #[serde(skip_serializing_if = "Option::is_none")]
    entrypoint: Option<RedactedValuesResponse>,
    #[serde(skip_serializing_if = "Option::is_none")]
    command: Option<RedactedValuesResponse>,
    environment: RedactedValuesResponse,
    cwd: PathBuf,
    #[serde(skip_serializing_if = "Option::is_none")]
    user: Option<String>,
    terminal: bool,
}

impl SandboxProcessResponse {
    fn from_process(process: SandboxProcessSpec) -> Self {
        Self {
            argv: redacted_values(process.args.len()),
            entrypoint: process
                .entrypoint
                .map(|values| redacted_values(values.len())),
            command: process.command.map(|values| redacted_values(values.len())),
            environment: redacted_values(process.env.len()),
            cwd: process.cwd,
            user: process.user,
            terminal: process.terminal,
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct RedactedValuesResponse {
    redacted: bool,
    value_count: usize,
}

fn redacted_values(value_count: usize) -> RedactedValuesResponse {
    RedactedValuesResponse {
        redacted: true,
        value_count,
    }
}
