use std::path::PathBuf;

use crate::context::RuntimeInvocationContext;
use crate::limits::RuntimeRoutingAffinity;
use crate::runtime::RuntimeBundle;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeRouteKey {
    Tenant(String),
    Function {
        tenant_label: String,
        function_name: String,
    },
    Script {
        tenant_label: Option<String>,
        entrypoint: PathBuf,
        expected_sha256: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) enum RuntimeLocalityError {
    MissingTenant {
        routing_affinity: RuntimeRoutingAffinity,
    },
}

impl std::fmt::Display for RuntimeLocalityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTenant { routing_affinity } => write!(
                f,
                "runtime routing affinity {routing_affinity:?} requires a tenant label"
            ),
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeReuseLocalityKey {
    Tenant(String),
    Function {
        tenant_label: String,
        function_name: String,
    },
    Script {
        tenant_label: Option<String>,
        entrypoint: PathBuf,
        expected_sha256: Option<String>,
    },
}

pub(crate) fn runtime_route_key(
    routing_affinity: RuntimeRoutingAffinity,
    context: Option<&RuntimeInvocationContext>,
    bundle: &RuntimeBundle,
) -> Result<Option<RuntimeRouteKey>, RuntimeLocalityError> {
    match routing_affinity {
        RuntimeRoutingAffinity::None => Ok(None),
        RuntimeRoutingAffinity::Tenant => {
            let Some(tenant_label) = context.and_then(|context| context.tenant_label.clone())
            else {
                return Err(RuntimeLocalityError::MissingTenant { routing_affinity });
            };
            Ok(Some(RuntimeRouteKey::Tenant(tenant_label)))
        }
        RuntimeRoutingAffinity::Function => {
            let Some(context) = context else {
                return Err(RuntimeLocalityError::MissingTenant { routing_affinity });
            };
            let Some(tenant_label) = context.tenant_label.clone() else {
                return Err(RuntimeLocalityError::MissingTenant { routing_affinity });
            };
            Ok(Some(RuntimeRouteKey::Function {
                tenant_label,
                function_name: context.function_name.clone(),
            }))
        }
        RuntimeRoutingAffinity::Script => Ok(Some(RuntimeRouteKey::Script {
            tenant_label: bundle.identity().tenant_label().map(str::to_owned),
            entrypoint: bundle.identity().entrypoint().to_path_buf(),
            expected_sha256: bundle.identity().expected_sha256().map(str::to_owned),
        })),
    }
}

pub(crate) fn runtime_reuse_locality_key(
    routing_affinity: RuntimeRoutingAffinity,
    context: Option<&RuntimeInvocationContext>,
    bundle: &RuntimeBundle,
) -> Result<Option<RuntimeReuseLocalityKey>, RuntimeLocalityError> {
    runtime_route_key(routing_affinity, context, bundle).map(|key| {
        key.map(|key| match key {
            RuntimeRouteKey::Tenant(tenant_label) => RuntimeReuseLocalityKey::Tenant(tenant_label),
            RuntimeRouteKey::Function {
                tenant_label,
                function_name,
            } => RuntimeReuseLocalityKey::Function {
                tenant_label,
                function_name,
            },
            RuntimeRouteKey::Script {
                tenant_label,
                entrypoint,
                expected_sha256,
            } => RuntimeReuseLocalityKey::Script {
                tenant_label,
                entrypoint,
                expected_sha256,
            },
        })
    })
}
