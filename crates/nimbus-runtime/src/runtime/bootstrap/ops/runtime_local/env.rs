use std::collections::BTreeMap;

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::runtime::bootstrap::state::{InstalledRuntimeCapabilityPolicy, RuntimeSharedWorkerEnv};
use crate::runtime_capabilities::RuntimeEnvLookupDescriptor;

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_env_get(
    state: &mut OpState,
    #[string] name: String,
) -> RuntimeEnvLookupDescriptor {
    let Some(policy) = state.try_borrow::<InstalledRuntimeCapabilityPolicy>() else {
        return RuntimeEnvLookupDescriptor::Missing;
    };
    let permissions = policy.permissions.clone();
    match permissions.check_env(&name) {
        Ok(()) => policy.env.lookup(&name),
        Err(error) => RuntimeEnvLookupDescriptor::Denied {
            message: format!("runtime env capability denied for `{name}`: {error}"),
        },
    }
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_env_snapshot(
    state: &mut OpState,
) -> BTreeMap<String, String> {
    state
        .try_borrow::<InstalledRuntimeCapabilityPolicy>()
        .map(|policy| policy.env.snapshot())
        .unwrap_or_default()
}

fn shared_env(state: &OpState) -> RuntimeSharedWorkerEnv {
    state.borrow::<RuntimeSharedWorkerEnv>().clone()
}

fn capability_denied_error(error: impl std::fmt::Display) -> JsErrorBox {
    JsErrorBox::generic(error.to_string())
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_shared_env_seed(
    state: &mut OpState,
    #[serde] snapshot: BTreeMap<String, String>,
) -> std::result::Result<(), JsErrorBox> {
    shared_env(state)
        .seed(snapshot)
        .map_err(capability_denied_error)
}

#[op2]
#[string]
pub(in super::super) fn op_nimbus_runtime_shared_env_get(
    state: &mut OpState,
    #[string] name: String,
) -> std::result::Result<Option<String>, JsErrorBox> {
    shared_env(state)
        .get(&name)
        .map_err(capability_denied_error)
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_shared_env_snapshot(
    state: &mut OpState,
) -> std::result::Result<BTreeMap<String, String>, JsErrorBox> {
    shared_env(state)
        .snapshot()
        .map_err(capability_denied_error)
}

#[op2(fast)]
pub(in super::super) fn op_nimbus_runtime_shared_env_set(
    state: &mut OpState,
    #[string] name: String,
    #[string] value: String,
) -> std::result::Result<(), JsErrorBox> {
    shared_env(state)
        .set(name, value)
        .map_err(capability_denied_error)
}

#[op2(fast)]
pub(in super::super) fn op_nimbus_runtime_shared_env_delete(
    state: &mut OpState,
    #[string] name: String,
) -> std::result::Result<(), JsErrorBox> {
    shared_env(state)
        .delete(&name)
        .map_err(capability_denied_error)
}
