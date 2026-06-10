# Bun/JSC Gate 15: Artifact Metadata And Server Rejection

Date: 2026-05-23

Nimbus prior proof revision: `3686f790` (`Record Bun lifecycle reuse proof`)

Bun worktree: `/Users/jack/src/github.com/oven-sh/bun`

Bun proof commit: `65cdc97796` (`Add Bun embed lifecycle reuse proof`)

Bun upstream base in local worktree: `f161e0311d`
(`shell: wrap only component-leading ! when neutralizing glob metachars (#31272)`)

Bun patch status: committed locally on Bun `main`, not upstreamed.

## Question

Does Nimbus name the runtime engine, bundle content kind, JavaScript evaluation
format, compatibility target, and package-resolution policy explicitly, and do
server/codegen paths reject unsupported Bun/JSC combinations before invocation?

## Scope

This gate is a Nimbus-side metadata and admission gate. It does not add a Bun
runtime route, executor lane, codegen target, or product selector.

## Implementation Shape

The runtime metadata seam now includes:

| Axis | Current production value | Bun proof value |
| --- | --- | --- |
| runtime engine | `v8` | `bun_jsc` is recognized but rejected |
| bundle content kind | `javascript` | `javascript` |
| JavaScript evaluation format | `es_module` | `program_wrapper` |
| compatibility target | `web_standard_isolate`, `node20`, `node22`, `node24` | not selectable |
| package resolution | `bundled`, `node_external_packages` | no Bun resolver yet |

Code and metadata changes:

- `RuntimeBackendKind` now has a named `BunJsc` variant, but
  `RuntimePolicy::new` rejects it as proof-only.
- `RuntimeJavaScriptEvaluationFormat` now names `es_module` and
  `program_wrapper`.
- `RuntimeLimits`, runtime diagnostics, tenant isolation decisions, and bundle
  engine cache keys carry the evaluation format.
- Convex generated metadata emits `runtime_javascript_evaluation_format:
  "es_module"` for all current V8 lanes.
- Convex registry loading rejects `runtime_engine: "bun_jsc"` with a
  proof-only error before invocation.
- Convex registry loading rejects `runtime_javascript_evaluation_format:
  "program_wrapper"` when paired with `runtime_engine: "v8"`.

## Rejection Examples

Unsupported V8/program-wrapper manifest:

```json
{
  "runtime_engine": "v8",
  "runtime_bundle_content_kind": "javascript",
  "runtime_javascript_evaluation_format": "program_wrapper",
  "runtime_compatibility_target": "web_standard_isolate",
  "runtime_package_resolution": "bundled"
}
```

Result: registry loading rejects it before invocation because V8 supports only
ES module evaluation.

Unsupported Bun/JSC manifest:

```json
{
  "runtime_engine": "bun_jsc",
  "runtime_bundle_content_kind": "javascript",
  "runtime_javascript_evaluation_format": "program_wrapper",
  "runtime_compatibility_target": "node22",
  "runtime_package_resolution": "node_external_packages"
}
```

Result: registry loading rejects it before invocation because Bun/JSC remains
proof-only and is not selectable.

## Verification

Runtime policy tests:

```sh
cargo test -p nimbus-runtime limits::tests --lib
```

Result: passed, 9 tests.

Server registry tests:

```sh
cargo test -p nimbus-server registry_and_license::registry --lib
```

Result: passed, 10 tests. The new cases are:

- `convex_registry_rejects_v8_program_wrapper_before_invocation`
- `convex_registry_rejects_bun_jsc_before_invocation`

Codegen selftest:

```sh
npm run test --workspace @nimbus/codegen
```

Result: passed.

Compilation check:

```sh
cargo check -p nimbus-runtime -p nimbus-server
```

Result: passed.

Formatting and whitespace:

```sh
cargo fmt --all --check
git diff --check
```

Result: passed.

## Decision

Status: artifact metadata and pre-invocation rejection are implemented.

Nimbus now has an explicit evaluation-format axis instead of overloading
`javascript` to mean both V8 ES modules and Bun/JSC program evaluation.
Bun/JSC is a named runtime engine for validation and audit clarity, but it is
not selectable. This keeps DX honest: generated artifacts describe what they
are, and unsupported Bun combinations fail at registry or policy construction
instead of reaching invocation execution.

The next gate should record the fork/upstream/hold decision against the current
Bun delta and the measured blockers.
