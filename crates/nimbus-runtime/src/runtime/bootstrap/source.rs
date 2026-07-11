use crate::backends::v8::embedder::JsRealm;
use crate::backends::v8::embedder::JsRuntime;
use crate::error::{NimbusRuntimeError, Result};

const DENO_HOST_CALL_TRANSPORT_SOURCE: &str = include_str!("js/deno_host_call_transport.js");

const NIMBUS_CONTEXT_CONTRACT_SOURCE: &str = include_str!("js/nimbus_context_contract.js");

const CLOUDFLARE_WORKERS_RUNTIME_SOURCE: &str = include_str!("js/cloudflare_workers_runtime.js");

const DENO_RUNTIME_GLOBALS_SOURCE: &str = include_str!("js/deno_runtime_globals.js");

const NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE: &str =
    include_str!("js/nimbus_side_channel_hardening.js");

const NIMBUS_GUEST_SEMANTICS_SOURCE: &str = include_str!("js/nimbus_guest_semantics.js");

// Keep Deno cleanup out of the bootstrap sources. Those sources are executed
// during startup-snapshot creation, and moving `delete globalThis.Deno` into
// them has already regressed snapshot-backed Locker runtime startup in the
// repaired deno_core fork. The cleanup must remain a separate post-bootstrap
// step until the fork exposes an explicit snapshot-safe alternative. Node22
// now binds its internal substrate against `__bootstrap.ext_node_denoGlobals`,
// so ordinary bundles should not observe the public `globalThis.Deno` contract
// after finalize_bootstrap() completes.
const POST_BOOTSTRAP_SOURCE: &str = include_str!("js/post_bootstrap.js");

const RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE: &str =
    include_str!("js/reset_bootstrap_invocation_state.js");

#[derive(Clone, Copy)]
struct BootstrapScript {
    name: &'static str,
    source: &'static str,
}

const BOOTSTRAP_SCRIPTS: &[BootstrapScript] = &[
    BootstrapScript {
        name: "<nimbus-runtime:bootstrap:deno-host-call-transport>",
        source: DENO_HOST_CALL_TRANSPORT_SOURCE,
    },
    BootstrapScript {
        name: "<nimbus-runtime:bootstrap:context-contract>",
        source: NIMBUS_CONTEXT_CONTRACT_SOURCE,
    },
    BootstrapScript {
        name: "<nimbus-runtime:bootstrap:cloudflare-workers-runtime>",
        source: CLOUDFLARE_WORKERS_RUNTIME_SOURCE,
    },
    BootstrapScript {
        name: "<nimbus-runtime:bootstrap:deno-runtime-globals>",
        source: DENO_RUNTIME_GLOBALS_SOURCE,
    },
    BootstrapScript {
        name: "<nimbus-runtime:bootstrap:side-channel-hardening>",
        source: NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE,
    },
    BootstrapScript {
        name: "<nimbus-runtime:bootstrap:guest-semantics>",
        source: NIMBUS_GUEST_SEMANTICS_SOURCE,
    },
];

const FINALIZE_BOOTSTRAP_SCRIPT: BootstrapScript = BootstrapScript {
    name: "<nimbus-runtime:bootstrap:finalize>",
    source: POST_BOOTSTRAP_SOURCE,
};

const RESET_BOOTSTRAP_SCRIPT: BootstrapScript = BootstrapScript {
    name: "<nimbus-runtime:bootstrap:reset>",
    source: RESET_BOOTSTRAP_INVOCATION_STATE_SOURCE,
};

pub(crate) fn install_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    execute_runtime_scripts(runtime, BOOTSTRAP_SCRIPTS)
}

pub(crate) fn install_bootstrap_in_realm(runtime: &mut JsRuntime, realm: &JsRealm) -> Result<()> {
    execute_realm_scripts(runtime, realm, BOOTSTRAP_SCRIPTS)
}

pub(crate) fn finalize_bootstrap(runtime: &mut JsRuntime) -> Result<()> {
    // This stays as an intentional second step instead of being folded into
    // install_bootstrap(), because the snapshot path also executes
    // the bootstrap sources during snapshot creation.
    execute_runtime_script(runtime, FINALIZE_BOOTSTRAP_SCRIPT)
}

pub(crate) fn finalize_bootstrap_in_realm(runtime: &mut JsRuntime, realm: &JsRealm) -> Result<()> {
    // Keep the realm path aligned with the main-context bootstrap contract: the
    // finalize step stays separate so callers can snapshot or install sources
    // before hiding bootstrap-only globals.
    execute_realm_script(runtime, realm, FINALIZE_BOOTSTRAP_SCRIPT)
}

pub(crate) fn reset_bootstrap_invocation_state(runtime: &mut JsRuntime) -> Result<()> {
    execute_runtime_script(runtime, RESET_BOOTSTRAP_SCRIPT)
}

pub(crate) fn reset_bootstrap_invocation_state_in_realm(
    runtime: &mut JsRuntime,
    realm: &JsRealm,
) -> Result<()> {
    execute_realm_script(runtime, realm, RESET_BOOTSTRAP_SCRIPT)
}

fn execute_runtime_scripts(runtime: &mut JsRuntime, scripts: &[BootstrapScript]) -> Result<()> {
    for script in scripts {
        execute_runtime_script(runtime, *script)?;
    }
    Ok(())
}

fn execute_runtime_script(runtime: &mut JsRuntime, script: BootstrapScript) -> Result<()> {
    runtime
        .execute_script(script.name, script.source)
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

fn execute_realm_scripts(
    runtime: &mut JsRuntime,
    realm: &JsRealm,
    scripts: &[BootstrapScript],
) -> Result<()> {
    for script in scripts {
        execute_realm_script(runtime, realm, *script)?;
    }
    Ok(())
}

fn execute_realm_script(
    runtime: &mut JsRuntime,
    realm: &JsRealm,
    script: BootstrapScript,
) -> Result<()> {
    realm
        .execute_script(runtime.v8_isolate(), script.name, script.source)
        .map_err(|error| NimbusRuntimeError::JavaScript(error.to_string()))?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{
        BOOTSTRAP_SCRIPTS, CLOUDFLARE_WORKERS_RUNTIME_SOURCE, DENO_HOST_CALL_TRANSPORT_SOURCE,
        DENO_RUNTIME_GLOBALS_SOURCE, NIMBUS_CONTEXT_CONTRACT_SOURCE, NIMBUS_GUEST_SEMANTICS_SOURCE,
        NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE,
    };

    #[test]
    fn bootstrap_registry_preserves_install_order() {
        let names = BOOTSTRAP_SCRIPTS
            .iter()
            .map(|script| script.name)
            .collect::<Vec<_>>();

        assert_eq!(
            names,
            vec![
                "<nimbus-runtime:bootstrap:deno-host-call-transport>",
                "<nimbus-runtime:bootstrap:context-contract>",
                "<nimbus-runtime:bootstrap:cloudflare-workers-runtime>",
                "<nimbus-runtime:bootstrap:deno-runtime-globals>",
                "<nimbus-runtime:bootstrap:side-channel-hardening>",
                "<nimbus-runtime:bootstrap:guest-semantics>",
            ],
        );
        assert!(
            BOOTSTRAP_SCRIPTS
                .iter()
                .all(|script| !script.source.trim().is_empty())
        );
    }

    #[test]
    fn context_contract_source_does_not_bind_deno_ops() {
        assert!(NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusCreateContext"));
        assert!(NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusSyncHostValue"));
        assert!(NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusAsyncHostValue"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("Deno.core.ops"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusCoreOps"));
    }

    #[test]
    fn deno_transport_source_injects_host_call_primitives_only() {
        assert!(DENO_HOST_CALL_TRANSPORT_SOURCE.contains("Deno.core.ops"));
        assert!(DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusSyncHostValue"));
        assert!(DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusAsyncHostValue"));
        assert!(DENO_HOST_CALL_TRANSPORT_SOURCE.contains("op_nimbus_cf_kv_get"));
        assert!(!DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusCreateContext"));
        assert!(!DENO_HOST_CALL_TRANSPORT_SOURCE.contains("__nimbusInstallRuntimeContractGlobals"));
    }

    #[test]
    fn cloudflare_workers_runtime_source_exposes_fetch_entrypoint_without_overriding_invoke() {
        assert!(CLOUDFLARE_WORKERS_RUNTIME_SOURCE.contains("CloudflareWorker"));
        assert!(CLOUDFLARE_WORKERS_RUNTIME_SOURCE.contains("__nimbusInvokeCloudflareWorkerFetch"));
        assert!(CLOUDFLARE_WORKERS_RUNTIME_SOURCE.contains("op_nimbus_cf_kv_get"));
        assert!(!CLOUDFLARE_WORKERS_RUNTIME_SOURCE.contains("__nimbusInvoke ="));
    }

    #[test]
    fn deno_runtime_globals_source_stays_outside_context_contract() {
        assert!(DENO_RUNTIME_GLOBALS_SOURCE.contains("__nimbusInstallRuntimeContractGlobals"));
        assert!(DENO_RUNTIME_GLOBALS_SOURCE.contains("op_nimbus_runtime_env_snapshot"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("__nimbusInstallRuntimeContractGlobals"));
        assert!(!NIMBUS_CONTEXT_CONTRACT_SOURCE.contains("op_nimbus_runtime_env_snapshot"));
    }

    #[test]
    fn guest_semantics_source_defines_controller_and_stays_inert_until_installed() {
        assert!(NIMBUS_GUEST_SEMANTICS_SOURCE.contains("__nimbusInstallGuestSemantics"));
        assert!(NIMBUS_GUEST_SEMANTICS_SOURCE.contains("__nimbusEnterGuestImportPhase"));
        assert!(NIMBUS_GUEST_SEMANTICS_SOURCE.contains("__nimbusBeginGuestInvocation"));
        // Only activates on the Convex default-runtime dialect.
        assert!(NIMBUS_GUEST_SEMANTICS_SOURCE.contains("convex_default"));
        // Definitions only at script-eval time (the source runs during
        // startup-snapshot creation): the sole op call is inside the
        // begin-invocation entry point, never at top level.
        assert!(NIMBUS_GUEST_SEMANTICS_SOURCE.contains("op_nimbus_runtime_invocation_determinism"));
    }

    #[test]
    fn pir3_side_channel_hardening_source_coarsens_timers_and_removes_shared_memory() {
        assert!(NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("__nimbusCoarsenTimerValue"));
        assert!(NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("Date.now"));
        assert!(NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("performance"));
        assert!(NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("Atomics.wait"));
        assert!(NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("Atomics.waitAsync"));
        assert!(NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("SharedArrayBuffer"));
        assert!(NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("WebAssembly.Memory"));
        assert!(
            NIMBUS_SIDE_CHANNEL_HARDENING_SOURCE.contains("__nimbusDisableSharedWebAssemblyMemory")
        );
    }
}
