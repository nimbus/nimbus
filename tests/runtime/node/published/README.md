# Runtimes

Nimbus executes user code inside explicit runtime targets. Runtime docs explain
what a developer can select, what the runtime is allowed to expose, and what
evidence backs each support claim.

## Available Runtime Families

| Runtime family | Status | Docs |
| --- | --- | --- |
| Web-standard JavaScript isolate | Supported baseline | See adapter-specific docs and `docs/architecture/runtime/adapter-boundary.md` |
| Node.js-compatible JavaScript | Node24 default, Node22 supported LTS, Node20 legacy-grace, and Node26 Current/non-LTS lanes with measured evidence | [Node.js runtime](nodejs/) |
| Bun/JSC JavaScript | Optional in-process backend candidate. Default builds stay fail-closed as `not_linked`; linked builds verify adapter metadata for diagnostics and load the shared adapter lazily on invocation. | Native runtime diagnostics and distribution docs are internal while this backend remains an optional candidate. |

## Runtime Permission Model

Runtime compatibility and runtime permissions are separate axes. A Node target
does not imply ambient host access, and a broader permission mode does not
change the JavaScript compatibility target.

Runtime compatibility is also separate from the internal engine that executes
the code. The default JavaScript runtime family is implemented through
Deno/V8. Bun/JSC is being productized as a separate optional in-process backend
with a dedicated Bun pool, not as a replacement for the Deno/V8/Node lanes. It
only becomes invocable when `/debug/runtime/metrics` reports
`execution_adapter_state: "linked"` and an `execution_adapter_artifact.status`
of `linked`; otherwise Bun/JSC selection fails closed.

Nimbus uses three permission modes:

| Mode | Meaning |
| --- | --- |
| `Restricted` | Least-privilege execution for explicitly sandboxed, tenant-supplied, or generated code surfaces. |
| `Standard` | Normal bounded backend/runtime execution with explicit grants. This is the current platform baseline. |
| `Privileged` | Highest Nimbus-approved permission ceiling for explicitly trusted operator or enterprise workloads. |

Fine-grained grants define the actual resource surface: filesystem roots,
network hosts, environment names, secret handles, service bindings, identities,
subprocess commands, system metadata, FFI, workers, and external tools. Internal
presets such as `Application` and `Tooling` are convenience bundles that lower
to `RuntimeMode + RuntimeGrants`; they are not permission modes.

## Evidence Posture

Nimbus does not use runtime names as blanket compatibility claims. A runtime
surface is documented as supported only when it has fixture, canary, oracle, or
classification evidence.

Internal engineering references live under `docs/architecture/runtime/`.
Developer-facing runtime guidance lives here.

## Bun/JSC Operator States

The Bun/JSC lane always reports `memory_enforcement:
"outer_quota_required"` until Bun/JSC exposes a proven hard per-VM heap
boundary. Its artifact diagnostics intentionally expose only sanitized install
metadata: source kind, status, reason code, expected source ref/revision, ABI
version, platform/target, and verified manifest metadata when available. Nimbus
does not expose absolute manifest or library paths through the operator API.

For installs, the Linux direct installer accepts `--with-bun-jsc` and installs
the optional `nimbus-bun-jsc-adapter` artifact after checksum, attestation,
manifest, SBOM/SLSA, export-set, native-symbol, and late-`dlopen` safety checks.
Linux package/repository lanes keep the adapter as a separate opt-in package.
macOS currently reserves the packaged Homebrew layout and uses the separate
release artifact lane until the tap has a payload for the adapter.
