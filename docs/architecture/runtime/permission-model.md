# Runtime Permission Model

Status: active baseline

Nimbus models runtime execution with separate axes:

| Axis | Current values | Notes |
| --- | --- | --- |
| Permission mode | `Restricted`, `Standard`, `Privileged` | The permission ceiling. |
| Grants | read/write roots, net, env, secret, identity, service, run, sys, ffi, worker, tool | The exact resource surface. |
| Runtime language | `JavaScript` | Other languages are future work. |
| Compatibility target | `WebStandardIsolate`, `Node20`, `Node22`, `Node24`, `BunJsc` | JavaScript/API compatibility, not permission. |
| Runtime preset | `Application`, `Tooling`, `Oracle`, `Operator`, `Code` | Internal workload bundles that lower to mode plus grants. |

## Execution Trust Tiers

Execution trust tiers are the cross-backend policy vocabulary. They are not
engine names, compatibility targets, runtime presets, or public API modes.

The tier answers "what isolation boundary makes this workload acceptable?"
before runtime mode and grants answer "which exact resources may it use?"

| Tier | Boundary | Intended workloads | Current status |
| --- | --- | --- | --- |
| `in_process_untrusted` | Same Nimbus process, engine-enforced permissions, runtime watchdog, and host-call ABI. | Tenant/application functions whose engine has proven permission enforcement, cancellation, memory policy, and teardown behavior. | Deno/V8 JavaScript lanes may run here when grants stay within the accepted subset below. Bun/JSC's fresh/discard profile is admitted here only with empty host-sensitive grants and an outer quota requirement; current builds still fail closed before guest execution unless a Bun embedder adapter is linked. |
| `in_process_trusted_only` | Same Nimbus process, but the workload is trusted because the engine or grant set cannot prove the untrusted contract yet. | Operator-owned tooling, Nimbus-owned code, proof harnesses, or future engines with incomplete containment. | Bun/JSC retained-VM reuse and proof-only profiles stay here until a later plan promotes them. Privileged in-process code also belongs here. |
| `wasm_capability_sandbox` | Same Nimbus process, WASM Component Model boundary, typed WIT imports, Store resource limits, and fuel/epoch interruption. | Future WASM components and tightly scoped agent extensions that receive only imported capabilities. | Deferred until the wasmtime backend proves WIT imports, Store lifecycle, interruption, and resource limits. |
| `microvm_service` | Separate sandbox lifecycle, usually a krun-backed microVM on Linux, with host exposure controlled through service bindings and endpoint policy. | Services or agents that need broader OS behavior, subprocesses, native dependencies, or crash/security isolation beyond an in-process engine. | Current service-control and sandbox seam. Security audit routing is owned by the execution-isolation plan. |

Tier selection must be explicit in architecture and validation decisions. The
server admission path has an internal `RuntimeIsolationTier` vocabulary for
the current `in_process_untrusted` gate and its canonical routing fallbacks:
`in_process_trusted_only`, `microvm_service`, and future
`wasm_capability_sandbox`.

### Tier Admission Rules

- Engine selection does not grant permission. `V8`, `Bun/JSC`, and future
  `wasmtime` are implementation choices below the tier.
- Compatibility target does not grant permission. A Node target means API
  shape, not filesystem, network, subprocess, secret, identity, native addon,
  or tool authority.
- Runtime preset does not grant trust. `Application`, `Tooling`, and
  `Operator` lower to mode plus grants, but the trust tier decides whether
  those grants are acceptable for untrusted in-process code.
- Unsupported tier/backend/content/target combinations must be rejected during
  policy construction or registry loading, or be marked deferred in the owning
  plan before implementation starts.
- A workload that needs host subprocesses, native addons, unrestricted package
  loading, broad outbound networking, or unproven engine policy hooks starts as
  `in_process_trusted_only` or moves out to `microvm_service`.

### Capability Matrix

| Capability family | `in_process_untrusted` | `in_process_trusted_only` | `wasm_capability_sandbox` | `microvm_service` |
| --- | --- | --- | --- | --- |
| `HostBridge` database, scheduler, nested runtime, and runtime extension calls | Allowed through the versioned host-call ABI and adapter-owned dispatch. Request principal and session checks still apply. | Allowed for trusted/operator workloads with the same ABI checks. | Allowed only through typed `nimbus:host` WIT imports that project to the same host operations. | Not direct. Services communicate through declared endpoints or future scoped service APIs. |
| Filesystem read/write | Allowed only through explicit symbolic roots such as `$generated_root`, `$app_root`, `$temp_root`, or `$cache_root` and engine-enforced path policy. | Allowed by explicit grants for trusted workloads. | Allowed only if the component imports a filesystem capability and admission binds it to a scoped provider. | Guest filesystem is inside the sandbox image/rootfs. Host mounts and read-only policy are sandbox-owned. |
| Network connect/listen | Allowed only by exact `RuntimeGrants` entries and server/operator exposure policy. Broad production exposure should prefer service sandboxes. | Allowed by explicit grants for trusted workloads. | Allowed only through imported HTTP/socket capabilities and admission policy. | Guest networking and published endpoints are sandbox-owned; host reachability is controlled by `SandboxPortBinding` and operator firewall policy. |
| Environment | `env_read` by exact name only. `env_write` is not allowed for untrusted in-process code. | Explicit `env_read`/`env_write` grants. | Imported environment/config capability only. | Sandbox process environment is declared in `SandboxSpec` or image metadata. |
| Secrets | No ambient materialization. A future secret API must require explicit grant and audit. | Explicit grant plus secret API audit when that surface exists. | Imported secret capability only, with typed handles rather than ambient globals. | Delivered through sandbox/service secret policy, not runtime globals. |
| Identity and token minting | Request auth is request-owned. No synthetic identity or token minting from a runtime grant alone. | Explicit identity grant plus server-owned mint/audit path. | Imported identity capability only. | Scoped service or agent sessions only; local admin tokens must not be passed into guests. |
| Services | Managed service lookup through exact service grants and server registry policy. | Explicit service grants. | Imported service capability only. | The service is the sandboxed endpoint. It does not gain Nimbus host authority by existing. |
| Subprocess/run | Not allowed for untrusted in-process code. | Explicit `run` grants for trusted tooling/operator paths. | Agent process capability only, if the component imports it and admission allows it. | Natural inside the guest; host impact controlled by sandbox isolation, mounts, and resource limits. |
| Native FFI/addons | Not allowed. | `Privileged` plus explicit `ffi` grants only. | Not allowed unless represented as a typed imported host capability. | Native code runs inside the guest isolation boundary. |
| Workers/background execution | Exact `worker` grants only when cancellation, resource accounting, and policy inheritance are proven for the engine. | Explicit grants for trusted workloads. | Component concurrency follows wasmtime/linker policy. | Guest owns its process tree; sandbox lifecycle and resource limits bound it. |
| Tools/connectors | Not ambient. Future tool access needs explicit grants and audit. | Explicit `tool` grants for trusted workloads. | Imported tool capability only. | Exposed as a declared service or scoped agent capability. |

### Current Assignments

| Workload or backend | Tier assignment | Notes |
| --- | --- | --- |
| Deno/V8 `WebStandardIsolate` application functions | `in_process_untrusted` | Production default when policy normalization keeps grants within the untrusted subset. |
| Deno/V8 `Node20`, `Node22`, and `Node24` production application functions | `in_process_untrusted` | Node compatibility target is API shape only. The production in-process profile has no generic loopback, listen, worker, inspector, subprocess, FFI, or ambient TLS-disable env grants. |
| Deno/V8 `Node20`, `Node22`, and `Node24` local-development application functions | `in_process_untrusted` in local-development mode only | Uses explicit local-dev grants for loopback/listen, inspector, worker threads, and `NODE_TLS_REJECT_UNAUTHORIZED` compatibility. Production admission routes the same grant set away from in-process untrusted execution. |
| Deno/V8 `Node20`, `Node22`, and `Node24` service profiles | `microvm_service` | Uses the broad service/microVM grant constructor for host-heavy Node behavior; the microVM tier owns OS isolation outside the in-process gate. |
| Deno/V8 tooling or operator workloads with `run`, `tool`, `identity`, or `Privileged` grants | `in_process_trusted_only` | These are trusted workload classes even when the engine is V8. |
| Bun/JSC fresh/discard application profile | `in_process_untrusted` | Admitted only as `RuntimeBackendKind::BunJsc`, `RuntimeCompatibilityTarget::BunJsc`, `BackendOwnedEventLoop`, `bun_jsc_in_process_untrusted`, `bun_jsc_fresh_discard_pool_outer_quota_required`, and `bun_jsc_fresh_discard`. Default builds remain adapter-not-linked; linked builds must load the verified Bun/JSC shared adapter before execution. |
| Bun/JSC proof or retained-VM profiles | `in_process_trusted_only` | Retained reuse is trusted-only until a hard isolation boundary and deliberate promotion exist. |
| Future wasmtime components | `wasm_capability_sandbox` | Deferred. Must prove typed imports, interruption, Store resource limits, and bundle metadata before selection. |
| Future WASI agent capabilities | `wasm_capability_sandbox` with additional imported agent capabilities | Deferred until wasmtime host interfaces are stable. Filesystem/process/HTTP are not inherited by ordinary WASM functions. |
| krun/container-backed services | `microvm_service` or local-dev container equivalent | Runtime code reaches them through service bindings; sandbox lifecycle and endpoint policy are separate from runtime engine policy. |

## Modes

`Restricted` is the least-privilege ceiling for explicitly sandboxed,
tenant-supplied, or generated-code surfaces.

`Standard` is the normal bounded backend/runtime ceiling. It is the current
platform baseline and still requires explicit grants for host-sensitive
resources.

`Privileged` is the highest Nimbus-approved ceiling for explicitly trusted
operator or enterprise workloads. It is not host root and still runs inside the
outer Nimbus sandbox.

## Grants

Modes do not directly imply resource access. Runtime enforcement consumes
`RuntimeGrants`:

| Grant family | Enforcement intent |
| --- | --- |
| `read` / `write` | Filesystem roots, including symbolic roots such as `$generated_root`, `$app_root`, `$temp_root`, and `$cache_root`. |
| `net_connect` / `net_listen` | Allowed network hosts. Node loopback support is a grant, not an automatic property of selecting Node. |
| `env_read` / `env_write` | Environment variables by explicit name. Sensitive values should use `secret`, not plain env grants. |
| `secret` | Secret handles and compatibility materialization rules. |
| `identity` | Service identity, token minting, or delegated-principal authority. |
| `service` | Managed service/binding handles. |
| `run` | Subprocess command or executable grants. |
| `sys` | System metadata, such as hostname or inspector metadata. |
| `ffi` | Native library access. |
| `worker` | Worker/background concurrency surface. |
| `tool` | Explicit connector or tool access. |

Current runtime enforcement builds filesystem, environment, network, system
metadata, subprocess, service, worker, and FFI admission from `RuntimeGrants`.
Subprocess grants use either exact executable names/paths or symbolic runtime
grants such as `$discovered_tooling`, `$runtime_self_exec`, and
`$runtime_host_exec`.

## Mode Ceilings

Mode ceilings are enforced during runtime-limit normalization before the runtime
policy is installed:

| Mode | Ceiling |
| --- | --- |
| `Restricted` | Rejects `env_write`, `identity`, `run`, `ffi`, `worker`, and `tool` grants. |
| `Standard` | Accepts bounded application/tooling grants but rejects `ffi`. |
| `Privileged` | Allows explicit grants, while still running inside the outer Nimbus sandbox. |

Sensitive host entrypoints also enforce their own grant families at use time:
managed service lookups require an exact `service` grant, worker-thread
creation requires `worker = ["thread"]`, subprocess execution requires a
matching `run` grant, and FFI descriptors require `ffi` grants.

For production tenant isolation, runtime network grants and service grants must
also compose safely. A tenant runtime that lacks a service grant must not be
able to discover or use that service by scanning localhost ports through a
generic network grant. Granting loopback network access is therefore a
cross-service authority decision: it must be constrained to admitted
tenant-owned endpoints or treated as incompatible with production
multi-tenant isolation. The current server-side gate is
`TenantIsolationMode::Production`, which rejects generic loopback or wildcard
runtime network grants before Convex or Cloud Functions can invoke
`in_process_untrusted` JavaScript. `TenantIsolationMode::default()` is
production; local-development entrypoints such as `nimbus dev` must opt out
explicitly when they need Node-compatible localhost grants. The completed
cross-layer baseline is
`docs/plans/archive/tenant-isolation-control-plane-plan.md`.

Secret and identity grants are declaration and audit inputs until the dedicated
secret-management and service-identity plans promote concrete APIs. Declaring a
`secret` grant does not place secret material in `process.env` or globals, and
declaring an `identity` grant does not synthesize `ctx.auth` identity. Request
auth remains request-owned; secret materialization must be introduced as an
explicit, separately tested surface, and provider-auth credentials must be
minted from admitted workload identity through
`docs/plans/service-identity-provider-auth-plan.md`.

## Policy Engine Boundary

Runtime grants are compiled by Nimbus-owned typed policy code. They are not
currently delegated to an external policy language or runtime. Operator policy
now has a typed artifact, local `validate|explain|diff|prove` UX, optional
external backend seam, sandbox egress policy, OCSF/OpenTelemetry export
mappings, denied-event draft workflow, and advisory/prover lane through
`docs/plans/archive/enterprise-policy-and-sandbox-egress-plan.md`.

OPA/Rego and Cedar are credible future policy backends when customers need
user-authored policy-as-code or analyzable authorization policy. They must
remain optional adapters behind Nimbus's built-in fail-closed evaluator, and
must not override hard built-in denies for tenant mismatch, forged identity,
unsafe secrets, unsafe host binds, broad loopback/wildcard grants, or SSRF
hardening.

## Presets

Presets are an internal ergonomics layer. They must not be used as permission
mode names in public or operator-facing contracts.

| Preset | Default lowering |
| --- | --- |
| `Application` | `Standard + application grants` |
| `Tooling` | `Standard + tooling grants` |
| `Oracle` | Evidence workflow grants, selected by the workflow owner |
| `Operator` | `Privileged + operator grants` |
| `Code` | `Restricted + narrow code-execution grants` |

Node application presets have deployment-specific constructors:

| Constructor family | Intended deployment | Grant shape |
| --- | --- | --- |
| `RuntimeLimits::application_node*()` / `application_node_production_in_process(...)` | Production in-process | Generated bundle filesystem roots plus narrow system metadata only; no loopback/listen, worker, inspector, run, FFI, or `NODE_TLS_REJECT_UNAUTHORIZED`. |
| `RuntimeLimits::application_node*_local_development()` / `application_node_local_development(...)` | Local development and compatibility harnesses | Adds loopback/listen, inspector, worker, and TLS-disable env compatibility grants. |
| `RuntimeLimits::application_node*_service_microvm()` / `application_node_service_microvm(...)` | Production service/microVM routing | Uses the broad Node service grants for behavior that must run behind the service isolation boundary. |

## Compatibility Targets

Compatibility targets describe JavaScript/runtime API shape. They do not grant
ambient host access.

For example, `Node22` and `Node24` expose measured Node-compatible API
surfaces, but filesystem, env, network, subprocess, secret, service, identity,
FFI, worker, and tool access still depend on the active mode and grants.
