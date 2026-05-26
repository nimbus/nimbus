use std::collections::BTreeMap;
use std::sync::{LazyLock, Mutex};

use crate::backends::v8::embedder::{JsErrorBox, OpState, op2};
use crate::runtime::bootstrap::state::InstalledRuntimeCapabilityPolicy;
use crate::runtime_capabilities::RuntimeEnvLookupDescriptor;

static NIMBUS_SHARED_WORKER_ENV: LazyLock<Mutex<BTreeMap<String, String>>> =
    LazyLock::new(|| Mutex::new(BTreeMap::new()));

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_env_get(
    state: &mut OpState,
    #[string] name: String,
) -> RuntimeEnvLookupDescriptor {
    let policy = state.borrow::<InstalledRuntimeCapabilityPolicy>();
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
    let policy = state.borrow::<InstalledRuntimeCapabilityPolicy>();
    policy.env.snapshot()
}

fn is_valid_shared_env_name(name: &str) -> bool {
    !name.is_empty()
        && name
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_shared_env_seed(
    #[serde] snapshot: BTreeMap<String, String>,
) -> std::result::Result<(), JsErrorBox> {
    for name in snapshot.keys() {
        if !is_valid_shared_env_name(name) {
            return Err(JsErrorBox::generic(format!(
                "invalid shared worker env variable name `{name}`"
            )));
        }
    }
    *NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned") = snapshot;
    Ok(())
}

#[op2]
#[string]
pub(in super::super) fn op_nimbus_runtime_shared_env_get(
    #[string] name: String,
) -> std::result::Result<Option<String>, JsErrorBox> {
    if !is_valid_shared_env_name(&name) {
        return Err(JsErrorBox::generic(format!(
            "invalid shared worker env variable name `{name}`"
        )));
    }
    Ok(NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .get(&name)
        .cloned())
}

#[op2]
#[serde]
pub(in super::super) fn op_nimbus_runtime_shared_env_snapshot() -> BTreeMap<String, String> {
    NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .clone()
}

#[op2(fast)]
pub(in super::super) fn op_nimbus_runtime_shared_env_set(
    #[string] name: String,
    #[string] value: String,
) -> std::result::Result<(), JsErrorBox> {
    if !is_valid_shared_env_name(&name) {
        return Err(JsErrorBox::generic(format!(
            "invalid shared worker env variable name `{name}`"
        )));
    }
    NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .insert(name, value);
    Ok(())
}

#[op2(fast)]
pub(in super::super) fn op_nimbus_runtime_shared_env_delete(
    #[string] name: String,
) -> std::result::Result<(), JsErrorBox> {
    if !is_valid_shared_env_name(&name) {
        return Err(JsErrorBox::generic(format!(
            "invalid shared worker env variable name `{name}`"
        )));
    }
    NIMBUS_SHARED_WORKER_ENV
        .lock()
        .expect("shared worker env lock should not be poisoned")
        .remove(&name);
    Ok(())
}
