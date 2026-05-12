# Runtimes

Neovex executes user code inside explicit runtime targets. Runtime docs explain
what a developer can select, what the runtime is allowed to expose, and what
evidence backs each support claim.

## Available Runtime Families

| Runtime family | Status | Docs |
| --- | --- | --- |
| Web-standard JavaScript isolate | Supported baseline | See adapter-specific docs and `docs/architecture/runtime/adapter-boundary.md` |
| Node.js-compatible JavaScript | Supported for measured Node20, Node22, and Node24 lanes | [Node.js runtime](nodejs/) |

## Runtime Permission Model

Runtime compatibility and runtime permissions are separate axes. A Node target
does not imply ambient host access, and a broader permission mode does not
change the JavaScript compatibility target.

Neovex uses three permission modes:

| Mode | Meaning |
| --- | --- |
| `Restricted` | Least-privilege execution for explicitly sandboxed, tenant-supplied, or generated code surfaces. |
| `Standard` | Normal bounded backend/runtime execution with explicit grants. This is the current platform baseline. |
| `Privileged` | Highest Neovex-approved permission ceiling for explicitly trusted operator or enterprise workloads. |

Fine-grained grants define the actual resource surface: filesystem roots,
network hosts, environment names, secret handles, service bindings, identities,
subprocess commands, system metadata, FFI, workers, and external tools. Internal
presets such as `Application` and `Tooling` are convenience bundles that lower
to `RuntimeMode + RuntimeGrants`; they are not permission modes.

## Evidence Posture

Neovex does not use runtime names as blanket compatibility claims. A runtime
surface is documented as supported only when it has fixture, canary, oracle, or
classification evidence.

Internal engineering references live under `docs/architecture/runtime/`.
Developer-facing runtime guidance lives here.
