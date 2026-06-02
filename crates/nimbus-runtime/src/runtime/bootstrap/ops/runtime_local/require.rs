use std::path::Path;

use deno_permissions::OpenAccessKind;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::node_compat::{
    ResolvedNodeModuleKind, ResolvedNodeTarget, resolve_node_target_with_user_conditions,
};
use crate::runtime::bootstrap::payloads::RuntimeHostCallEnvelope;
use crate::runtime::bootstrap::state::InstalledRuntimeCapabilityPolicy;

use super::support::capability_denied_error;
use super::types::{
    RuntimeRequireReadFilePayload, RuntimeRequireResolvePayload, RuntimeRequireResolveResponse,
};

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_require_resolve(
    state: &mut OpState,
    #[serde] payload: RuntimeRequireResolvePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let capability_policy = state.borrow::<InstalledRuntimeCapabilityPolicy>();
    let path_policy = capability_policy.paths.clone();
    let node_conditions = capability_policy.node_conditions.clone();
    let referrer = payload
        .referrer
        .unwrap_or_else(|| path_policy.cwd().display().to_string());
    let resolved = resolve_node_target_with_user_conditions(
        &path_policy,
        &payload.specifier,
        &referrer,
        node_resolver::ResolutionMode::Require,
        &node_conditions,
    )?;
    let response = match resolved {
        ResolvedNodeTarget::BuiltIn { module_name } => {
            RuntimeRequireResolveResponse::Builtin { module_name }
        }
        ResolvedNodeTarget::Module { path, kind } => {
            let path = path_policy
                .ensure_module_read_path(&path)
                .map_err(|error| JsErrorBox::generic(error.to_string()))?;
            match kind {
                ResolvedNodeModuleKind::CommonJs => RuntimeRequireResolveResponse::CommonJs {
                    path: path.display().to_string(),
                },
                ResolvedNodeModuleKind::EsModule => RuntimeRequireResolveResponse::EsModule {
                    path: path.display().to_string(),
                },
                ResolvedNodeModuleKind::Json => RuntimeRequireResolveResponse::Json {
                    path: path.display().to_string(),
                },
            }
        }
    };
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::to_value(response)
            .map_err(|error| JsErrorBox::generic(error.to_string()))?,
    })
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_require_read_file(
    state: &mut OpState,
    #[serde] payload: RuntimeRequireReadFilePayload,
) -> std::result::Result<RuntimeHostCallEnvelope, JsErrorBox> {
    let permissions = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .permissions
        .clone();
    let path = permissions
        .check_open(
            std::borrow::Cow::Borrowed(Path::new(&payload.path)),
            OpenAccessKind::Read,
            Some("node:module.require"),
        )
        .map_err(capability_denied_error)?
        .into_owned_path();
    let value = std::fs::read_to_string(&path).map_err(|error| {
        JsErrorBox::generic(format!(
            "failed to read CommonJS runtime module {}: {error}",
            path.display()
        ))
    })?;
    Ok(RuntimeHostCallEnvelope::Ok {
        value: serde_json::Value::String(value),
    })
}
