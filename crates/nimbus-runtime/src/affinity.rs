use std::path::PathBuf;

use crate::context::RuntimeInvocationContext;
use crate::limits::RuntimeRoutingAffinity;
use crate::runtime::RuntimeBundle;

#[derive(Clone, Debug, PartialEq, Eq, Hash)]
pub(crate) enum RuntimeAffinityKey {
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
pub(crate) enum RuntimeAffinityError {
    MissingTenant {
        routing_affinity: RuntimeRoutingAffinity,
    },
}

impl std::fmt::Display for RuntimeAffinityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::MissingTenant { routing_affinity } => write!(
                f,
                "runtime routing affinity {routing_affinity:?} requires a tenant label"
            ),
        }
    }
}

pub(crate) fn runtime_affinity_key(
    routing_affinity: RuntimeRoutingAffinity,
    context: Option<&RuntimeInvocationContext>,
    bundle: &RuntimeBundle,
) -> Result<Option<RuntimeAffinityKey>, RuntimeAffinityError> {
    match routing_affinity {
        RuntimeRoutingAffinity::None => Ok(None),
        RuntimeRoutingAffinity::Tenant => {
            let Some(tenant_label) = context.and_then(|context| context.tenant_label.clone())
            else {
                return Err(RuntimeAffinityError::MissingTenant { routing_affinity });
            };
            Ok(Some(RuntimeAffinityKey::Tenant(tenant_label)))
        }
        RuntimeRoutingAffinity::Function => {
            let Some(context) = context else {
                return Err(RuntimeAffinityError::MissingTenant { routing_affinity });
            };
            let Some(tenant_label) = context.tenant_label.clone() else {
                return Err(RuntimeAffinityError::MissingTenant { routing_affinity });
            };
            Ok(Some(RuntimeAffinityKey::Function {
                tenant_label,
                function_name: context.function_name.clone(),
            }))
        }
        RuntimeRoutingAffinity::Script => Ok(Some(RuntimeAffinityKey::Script {
            tenant_label: bundle.identity().tenant_label().map(str::to_owned),
            entrypoint: bundle.identity().entrypoint().to_path_buf(),
            expected_sha256: bundle.identity().expected_sha256().map(str::to_owned),
        })),
    }
}
