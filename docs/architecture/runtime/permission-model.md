# Runtime Permission Model

Status: active baseline

Nimbus models runtime execution with separate axes:

| Axis | Current values | Notes |
| --- | --- | --- |
| Permission mode | `Restricted`, `Standard`, `Privileged` | The permission ceiling. |
| Grants | read/write roots, net, env, secret, identity, service, run, sys, ffi, worker, tool | The exact resource surface. |
| Runtime language | `JavaScript` | Other languages are future work. |
| Compatibility target | `WebStandardIsolate`, `Node20`, `Node22`, `Node24` | JavaScript/API compatibility, not permission. |
| Runtime preset | `Application`, `Tooling`, `Oracle`, `Operator`, `Code` | Internal workload bundles that lower to mode plus grants. |

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

Secret and identity grants are declaration and audit inputs until a future
secret-store or service-identity API exists. Declaring a `secret` grant does not
place secret material in `process.env` or globals, and declaring an `identity`
grant does not synthesize `ctx.auth` identity. Request auth remains
request-owned; secret materialization must be introduced as an explicit,
separately tested surface.

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

## Compatibility Targets

Compatibility targets describe JavaScript/runtime API shape. They do not grant
ambient host access.

For example, `Node22` exposes the measured Node-compatible API surface, but
filesystem, env, network, subprocess, secret, service, identity, FFI, worker,
and tool access still depend on the active mode and grants.
