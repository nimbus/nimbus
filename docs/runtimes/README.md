# Runtimes

Neovex executes user code inside explicit runtime targets. Runtime docs explain
what a developer can select, what the runtime is allowed to expose, and what
evidence backs each support claim.

## Available Runtime Families

| Runtime family | Status | Docs |
| --- | --- | --- |
| Web-standard JavaScript isolate | Supported baseline | See adapter-specific docs and `docs/architecture/runtime/adapter-boundary.md` |
| Node.js-compatible JavaScript | Supported for measured Node20, Node22, and Node24 lanes | [Node.js runtime](nodejs/) |

## Evidence Posture

Neovex does not use runtime names as blanket compatibility claims. A runtime
surface is documented as supported only when it has fixture, canary, oracle, or
classification evidence.

Internal engineering references live under `docs/architecture/runtime/`.
Developer-facing runtime guidance lives here.
