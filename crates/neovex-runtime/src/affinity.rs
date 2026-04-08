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

pub(crate) fn runtime_affinity_key(
    routing_affinity: RuntimeRoutingAffinity,
    context: Option<&RuntimeInvocationContext>,
    bundle: &RuntimeBundle,
) -> Option<RuntimeAffinityKey> {
    match routing_affinity {
        RuntimeRoutingAffinity::None => None,
        RuntimeRoutingAffinity::Tenant => context
            .and_then(|context| context.tenant_label.clone())
            .map(RuntimeAffinityKey::Tenant),
        RuntimeRoutingAffinity::Function => context.and_then(|context| {
            context
                .tenant_label
                .clone()
                .map(|tenant_label| RuntimeAffinityKey::Function {
                    tenant_label,
                    function_name: context.function_name.clone(),
                })
        }),
        RuntimeRoutingAffinity::Script => Some(RuntimeAffinityKey::Script {
            tenant_label: bundle.identity().tenant_label().map(str::to_owned),
            entrypoint: bundle.identity().entrypoint().to_path_buf(),
            expected_sha256: bundle.identity().expected_sha256().map(str::to_owned),
        }),
    }
}
