# Plan: Service Backend And Sandbox Spec Refactor

## Status

- **Status:** `proposed`
- **Primary goal:** make the pre-launch service/sandbox type model match the
  architecture vocabulary by replacing service "implementation" wrappers with
  declarative service backends, moving sandbox root materialization into
  `SandboxSpec`, keeping Dockerfile/context build input as an explicit
  policy-gated OCI image materialization source, and keeping `SandboxBackend`
  as the only sandbox lifecycle executor interface.
- **Owning docs:**
  - [`docs/architecture/sandbox/service-sandbox-session-model.md`](../architecture/sandbox/service-sandbox-session-model.md)
  - [`docs/plans/nimbus-sdk-resource-model-plan.md`](nimbus-sdk-resource-model-plan.md)
  - [`docs/plans/nimbus-capability-segregation-plan.md`](nimbus-capability-segregation-plan.md)
- **Pre-launch posture:** breaking changes are preferred. Do not add aliases,
  compatibility shims, or deprecated old names.

This plan is a naming and resource-model cleanup before the SDK/service model
hardens. It does not create adapter-visible Nimbus shortcuts, does not change the
public adapter compatibility APIs, and does not make runtime invocation isolates
SDK sandboxes.

Complete this plan before implementing the backend/spec phases of
[`docs/plans/nimbus-sdk-resource-model-plan.md`](nimbus-sdk-resource-model-plan.md).
The SDK plan may bootstrap registration/verifier work and verify the already
landed service lifecycle/status SDK surface, but it must not implement dynamic
service backend specs, sandbox resource APIs, or session resource APIs against
the old `ServiceImplementation` / launch-spec vocabulary.

This plan primarily owns service/backend/sandbox naming. The already-completed
workload identity naming cleanup remains in the capability-segregation and
service-identity-provider plans: use `WorkloadIdentity`, `WorkloadKind`,
`WorkloadAttributes`, and `WorkloadIdentity.subject()` there; keep
`TenantWorkloadSpec` for the server-owned desired-state object in
`nimbus-node`/local enforcement because the tenant prefix marks that boundary.
The only workload-identity cleanup carried here is test-name polish for stale
`tenant_workload_identity_*` names.

## Plan Value

This plan removes type/model churn before the SDK resource model expands. Its
value is architectural leverage: one canonical Rust vocabulary for service
definitions, sandbox specs, root materialization, and sandbox backend lifecycle.
That lets the SDK plan add services, sandboxes, and sessions on top of stable
resource names instead of encoding transitional `Implementation` and `Launch`
types into public APIs, docs, or verifiers.

## Final Decision

The canonical chain is:

```text
Service
  -> ServiceBackend
  -> ServiceBackend::Sandbox(SandboxSpec)
  -> SandboxSpec.backend: SandboxBackendKind
  -> selected SandboxBackend implementation
  -> SandboxBackend::start(spec)
```

Service definitions declare a backend. Sandbox specs declare the desired sandbox
resource, including ownership metadata, the sandbox backend kind, and the root
materialization. A sandbox backend implementation owns how the sandbox starts.

## Build Input Decision

Current repo evidence shows that Nimbus does have build-backed execution paths:
Compose `build:` lowers to `SandboxBuildLaunchSpec`, the macOS machine API
offers `service-sandboxes.build-start`, and container/krun backends prepare a
root filesystem from Dockerfile/context input. The current runtime path does
not require the Docker daemon or Podman as the product contract; the sandbox
code has an internal Dockerfile-subset materializer that pulls/materializes OCI
base images and applies supported Dockerfile instructions to a sandbox rootfs.
Buildah CLI helpers remain part of the OCI/rootfs tooling surface, but `crun`
and `krun` are execution backends, not image builders.

That means build input is real, so the target model must not erase it. Build is
not a peer root kind, though. It is a way to obtain OCI image material. The
target model therefore keeps build under
`SandboxRootSpec::OciImage(SandboxOciImageSpec { source: ... })`, with
`SandboxOciImageSource::Reference(...)` and
`SandboxOciImageSource::Build(...)` as the source variants. `Rootfs` versus
`OciImage` answers what kind of root material the sandbox uses; `Reference`
versus `Build` answers how OCI image material is obtained. Build remains
policy-gated: production Compose admission continues to reject local builds by
default, tenant image admission treats local builds as an explicit exception
without registry-image verification evidence, and Quadlet/Kubernetes export
continues to tell operators to build and tag images first unless a future
artifact/provenance pipeline changes that policy.

If Nimbus later makes Dockerfile/context builds a production product feature,
that work must add cache keys, provenance, SBOM/signature policy, failure
semantics, and admission evidence. That hardening can extend
`SandboxOciBuildSpec` or route through a separate artifact/build pipeline that
returns admitted OCI image material, but it must not reintroduce a second
sandbox lifecycle API.

## Target Type Shape

Use service-scoped `Spec` names for declarative payloads. Reserve `Backend` for
actual executor/provider seams or the top-level service backend category.

```rust
pub enum ServiceBackend {
    Sandbox(SandboxSpec),
    BuiltIn(BuiltInServiceSpec),
    External(ExternalServiceSpec),
}

pub struct BuiltInServiceSpec {
    pub provider: BuiltInServiceProvider,
}

pub struct ExternalServiceSpec {
    pub endpoint: ExternalServiceEndpoint,
}
```

Move root materialization into `SandboxSpec` and remove service-owned sandbox
launch wrappers.

```rust
pub struct SandboxSpec {
    pub tenant_id: TenantId,
    pub owner: SandboxOwnerSpec,
    pub backend: SandboxBackendKind,
    pub root: SandboxRootSpec,
    pub process: SandboxProcessSpec,
    pub resources: SandboxResourceLimits,
    pub lifecycle: SandboxLifecycleSpec,
    pub port_bindings: Vec<SandboxPortBinding>,
    pub mounts: Vec<SandboxMountSpec>,
    pub egress: SandboxEgressPolicy,
}

pub enum SandboxOwnerSpec {
    Service { name: String },
    Standalone { display_name: Option<String> },
}

pub enum SandboxRootSpec {
    Rootfs(SandboxRootfsSpec),
    OciImage(SandboxOciImageSpec),
}

pub struct SandboxOciImageSpec {
    pub source: SandboxOciImageSource,
    // platform, admission evidence, cache policy...
}

pub enum SandboxOciImageSource {
    Reference(SandboxOciImageReferenceSpec),
    Build(SandboxOciBuildSpec),
}
```

Keep the sandbox backend interface lifecycle-shaped:

```rust
pub trait SandboxBackend: Send + Sync + 'static {
    fn kind(&self) -> SandboxBackendKind;
    fn start(&self, spec: SandboxSpec) -> SandboxFuture<SandboxHandle>;
    fn inspect(&self, id: &SandboxId) -> SandboxFuture<Option<SandboxHandle>>;
    fn stop(&self, id: &SandboxId) -> SandboxFuture<()>;
}
```

`SandboxOwnerSpec` is metadata for scoping artifacts, audit records, and service
readiness bindings. It is not a sandbox address. Service-backed sandboxes use
`SandboxOwnerSpec::Service { name }`; standalone/user-created sandboxes are still
addressed only by the sandbox id or returned handle.

`SandboxSpec.process` is the public sandbox process contract. Do not preserve a
public image-launch-only `SandboxImageProcessOverrides` type. The final process
model must be able to express OCI image default resolution semantics
(`ENTRYPOINT`, `CMD`, environment, working directory, user, and terminal
settings) without making image/build startup a separate launch API. If a backend
needs a helper for resolved OCI defaults, keep that helper private to the
backend or OCI materialization module.

## Naming Rules

- Use `Backend` when the type is an executor/provider seam or a top-level
  backend category.
- Use `Spec` when the type is a declarative resource payload.
- Use `Kind` for small discriminants such as `SandboxBackendKind::Container` and
  `SandboxBackendKind::Krun`.
- Use `OciImage` for OCI image material. This is not stutter: OCI means Open
  Container Initiative, and "OCI image" is the standard artifact name.
- Do not use `Type` for these enums; it is less precise than `Kind` or `Spec`.
- Do not use `Launch` in durable service or sandbox catalog types. Launch/start
  is a lifecycle operation owned by backend implementations.
- Do not use `Backing`; Nimbus already uses `Backend` consistently for provider
  seams.

## Rename And Refactor Table

| Current name or shape | Target name or shape | Reason |
| --- | --- | --- |
| `ServiceImplementation` | `ServiceBackend` | Service catalog chooses the service backend category. |
| `ServiceImplementation::SandboxBacked(...)` | `ServiceBackend::Sandbox(SandboxSpec)` | The service should carry the desired sandbox resource, not a service-specific launch wrapper. |
| `SandboxBackedServiceImplementation` | Remove | Its variants move into `SandboxSpec.root`. |
| `BuiltInServiceImplementation` | `BuiltInServiceSpec` | Built-in service payload is declarative; the executor/provider is separate. |
| `ExternalServiceImplementation` | `ExternalServiceSpec` | External service payload is declarative endpoint policy, not an executor. |
| `BuiltInServiceImplementation.capability` | `BuiltInServiceSpec.provider` or `provider_id` | Avoid conflict with security capabilities. Use `provider_id` only if it is a stable registry key. |
| `service_implementation_for_tenant(...)` | `service_backend_for_tenant(...)` | Aligns catalog API with `ServiceBackend`. |
| `implementation_kind()` | `kind()` returning `ServiceBackendKind` or stable label | Removes stutter from `implementation_kind`. |
| `SandboxSpec.name` | `SandboxSpec.owner: SandboxOwnerSpec` | Keeps service-backed provenance without implying sandboxes are name-addressed resources. |
| `SandboxImageLaunchSpec` | `SandboxOciImageSource::Reference(SandboxOciImageReferenceSpec)` inside `SandboxRootSpec::OciImage(SandboxOciImageSpec { source, ... })` | Existing-image startup is an OCI image materialization input, not a separate launch API. |
| `SandboxBuildLaunchSpec` | `SandboxOciImageSource::Build(SandboxOciBuildSpec)` inside `SandboxRootSpec::OciImage(SandboxOciImageSpec { source, ... })` | Dockerfile/context builds are an OCI image materialization input, not a separate root kind or lifecycle API. |
| `SandboxImageProcessOverrides` | Fold into `SandboxProcessSpec` or private OCI-default resolution helpers | Process configuration belongs to the sandbox spec; image default merging is an implementation detail, not a public launch wrapper. |
| `SandboxFilesystemSpec` | `SandboxRootfsSpec` | The current field represents a root filesystem. Reserve broader filesystem naming for mounts/volumes. |
| `SandboxBackend::start_from_image(...)` | Remove | `SandboxBackend::start(SandboxSpec)` interprets `spec.root`. |
| `SandboxBackend::start_from_build(...)` | Remove | Build handling stays inside `SandboxBackend::start(SandboxSpec)` dispatch on `SandboxRootSpec::OciImage(SandboxOciImageSpec { source: SandboxOciImageSource::Build(...), ... })`. |
| `ServiceInstanceRuntimeRegistry` | `ServiceInstanceBindingRegistry` | The concrete registry projects service instances into runtime invocation bindings. |
| `start_launch_async(...)` and launch-local variables | `start_sandbox_service_async(...)`, `service_backend`, `sandbox_spec`, or inline `start(...)` | Avoid naming the lifecycle operation as a durable model object. |
| `service_process_snapshot(...)` | `service_sandbox_process_snapshot(...)` | The machine API method takes a sandbox id and calls a `service-sandboxes/{id}/ps` backing-plane route. |
| `tenant_workload_identity_*` test names | `workload_identity_*` test names | The tests exercise `WorkloadIdentity`; the tenant prefix already appears in the setup and expected subject fields. |

## Adjacent Cleanup Items

These are small but intentional enterprise-polish cleanups that prevent stale
vocabulary from re-teaching the wrong model:

- Replace private-doc `ctx.services` examples in
  `docs/private/agentic-ai-tco.md` with explicit `Nimbus` SDK service examples.
- Rename `crates/nimbus-tenant/src/tests.rs` workload identity tests from
  `tenant_workload_identity_*` to `workload_identity_*`.
- Rename the machine API client helper `service_process_snapshot(...)` to
  `service_sandbox_process_snapshot(...)`.

## Non-Escape Rules

- Do not introduce `ServiceBackend::Sandbox(SandboxLaunchSpec)`.
- Do not introduce a generic `SandboxSource` layer unless it removes real
  duplication; `SandboxRootSpec` is the canonical materialization enum.
- Do not introduce `SandboxRootSpec::Build` or `SandboxRootSpec::OciBuild`.
  Build is not a peer root kind. Use
  `SandboxRootSpec::OciImage(SandboxOciImageSpec { source:
  SandboxOciImageSource::Build(...), ... })` so the type says the build input
  belongs to OCI image materialization.
- Do not allow build input to bypass image/build admission policy. Local/dev
  builds are explicit exceptions; production builds require an operator-owned
  provenance and admission story before they become allowed.
- Do not reuse `SandboxBackend` for declarative catalog payloads. It remains the
  executor/provider trait.
- Do not keep a public image-launch-only process override type such as
  `SandboxImageProcessOverrides`. OCI image default resolution is part of
  `SandboxSpec.process` handling or private backend preparation.
- Do not make sandbox owner metadata a lookup surface. Sandboxes remain
  id/handle-addressed, even when a service-backed sandbox records
  `SandboxOwnerSpec::Service { name }`.
- Do not add a second service implementation vocabulary beside
  `ServiceBackend`.
- Do not add aliases for old names. This repo has not launched.
- Do not expose `ctx.services`, `ctx.sandboxes`, `ctx.sessions`, `ctx.browser`,
  or equivalent adapter shortcuts while doing this refactor.
- Do not convert runtime invocation isolates into SDK sandbox resources. A future
  user-created isolate sandbox, if added, must use the explicit sandbox resource
  spelling from the SDK resource model plan.

## Phase Status Ledger

| Phase | Status | Hard dependencies | Verifiable success signal |
| --- | --- | --- | --- |
| SBR0 | `done` | none | Plan is registered and docs validation passes. |
| SBR1 | `todo` | SBR0 | `nimbus-services` exposes `ServiceBackend`, `BuiltInServiceSpec`, `ExternalServiceSpec`, and `service_backend_for_tenant(...)`; old implementation names are gone. |
| SBR2 | `todo` | SBR1 | `SandboxSpec` owns `owner: SandboxOwnerSpec` and `root: SandboxRootSpec`; rootfs/OCI-image materialization no longer uses `Launch` names. |
| SBR3 | `todo` | SBR2 | `SandboxBackend` has one start path, `start(SandboxSpec)`, and backend implementations dispatch internally on `SandboxRootSpec`. |
| SBR4 | `todo` | SBR3 | Compose lowering, service manager activation, verification, machine API, and runtime binding code use `ServiceBackend::Sandbox(SandboxSpec)`. |
| SBR5 | `todo` | SBR4 | Architecture and SDK docs use the final vocabulary and examples contain no old names. |
| SBR6 | `todo` | SBR5 | Focused Rust/docs gates and stale-name guards pass with evidence. |

## Phases

### SBR0 - Plan Registration

- Goal: make the decision durable and discoverable.
- Files: this plan, `docs/plans/README.md`.
- Steps:
  - Register this plan in the active plans index.
  - Link it from follow-on SDK/resource-model work when those docs are touched.
  - Add stale-name verifier conditions in SBR6 rather than during plan-only
    creation.
- Gate: `npm run docs:validate-refs:strict` and `git diff --check` pass for the
  touched plan docs.

### SBR1 - Service Backend Vocabulary

- Goal: replace the service implementation vocabulary with service backend
  vocabulary.
- Files: `crates/nimbus-services/src/catalog.rs`, crate exports, service manager
  tests, compose tests, server service-manager tests.
- Steps:
  - Rename `ServiceImplementation` to `ServiceBackend`.
  - Rename `BuiltInServiceImplementation` to `BuiltInServiceSpec`.
  - Rename `ExternalServiceImplementation` to `ExternalServiceSpec`.
  - Replace `service_implementation_for_tenant(...)` with
    `service_backend_for_tenant(...)`.
  - Replace `implementation_kind()` with `kind()` and use either a typed
    `ServiceBackendKind` or a stable label only at serialization/logging edges.
  - Rename built-in `capability` fields to `provider` or `provider_id`.
- Gate: no production code references `ServiceImplementation`,
  `BuiltInServiceImplementation`, `ExternalServiceImplementation`, or
  `service_implementation_for_tenant`.

### SBR2 - Sandbox Root Spec

- Goal: make `SandboxSpec` the complete declarative sandbox resource.
- Files: `crates/nimbus-sandbox/src/spec.rs`, sandbox tests, compose lowering,
  machine API request types.
- Steps:
  - Add `SandboxRootSpec`.
  - Replace `SandboxSpec.name` with `SandboxOwnerSpec` so service-backed specs
    preserve the owning service name without creating sandbox name lookup.
  - Rename `SandboxFilesystemSpec` to `SandboxRootfsSpec` if its only durable
    meaning remains the root filesystem.
  - Replace `SandboxImageLaunchSpec` with
    `SandboxOciImageSource::Reference(SandboxOciImageReferenceSpec)`.
  - Replace `SandboxBuildLaunchSpec` with
    `SandboxOciImageSource::Build(SandboxOciBuildSpec)`.
  - Move Dockerfile/context build input into the OCI image branch:
    `SandboxRootSpec::OciImage(SandboxOciImageSpec { source:
    SandboxOciImageSource::Build(...), ... })`.
  - Preserve Compose `build:` for local/dev and explicitly admitted build
    scenarios; production admission must keep rejecting local builds until the
    operator-owned provenance/signature/SBOM/cache policy exists.
  - Fold `SandboxImageProcessOverrides` into the final process model or private
    OCI-default resolution helpers. The public model should expose
    `SandboxSpec.process`, not image-launch-specific process overrides.
- Gate: `SandboxSpec` has a `root` field and no durable sandbox catalog type has
  `Launch` or `SandboxImageProcessOverrides` in its name.

### SBR3 - Sandbox Backend Start Interface

- Goal: keep sandbox lifecycle behind the backend trait.
- Files: `crates/nimbus-sandbox/src/backend.rs`, container backend, krun backend,
  forwarded machine API backend, sandbox smoke tests.
- Steps:
  - Remove `SandboxBackend::start_from_image(...)`.
  - Remove `SandboxBackend::start_from_build(...)`.
  - Teach `SandboxBackend::start(SandboxSpec)` implementations to dispatch on
    `spec.root`.
  - Keep backend-specific planning helpers private and name them after backend
    planning details, not public launch specs.
- Gate: `rg "start_from_image|start_from_build|SandboxImageLaunchSpec|SandboxBuildLaunchSpec|SandboxImageProcessOverrides"`
  returns no production references; `SandboxOciImageSource::Build` and
  `SandboxOciBuildSpec` are the only production build-input spellings.

### SBR4 - Service Manager And Compose Lowering

- Goal: make service activation consume the final model.
- Files: `crates/nimbus-services/**`, `crates/nimbus-bin/src/compose/**`,
  `crates/nimbus-bin/src/machine/**`, server service-control tests.
- Steps:
  - Make compose service lowering produce `ServiceBackend::Sandbox(SandboxSpec)`.
  - Keep Compose `build:` lowering to
    `SandboxRootSpec::OciImage(SandboxOciImageSpec { source:
    SandboxOciImageSource::Build(...), ... })` only when build admission allows
    it. Production tenant isolation keeps failing closed by default with an
    actionable "publish a digest-pinned image or configure operator-owned build
    provenance" error.
  - Make service activation fetch `ServiceBackend` and call
    `sandbox_backend.start(sandbox_spec)` for sandbox-backed services.
  - Keep service lifecycle/status routes service-shaped:
    `start`, `stop`, `restart`, and `get`.
  - Ensure built-in and external service definitions stay declarative and fail
    closed until their executor/route support exists.
  - Rename machine API client process inspection helper
    `service_process_snapshot(...)` to `service_sandbox_process_snapshot(...)`
    while preserving the existing `service-sandboxes/{id}/ps` backing-plane
    route.
- Gate: service manager tests prove sandbox-backed services still start, stop,
  restart, enforce exact grants, and report status through the existing
  service-control path.

### SBR5 - Docs And SDK Resource Model Alignment

- Goal: make docs and examples teach one vocabulary.
- Files:
  - `docs/architecture/sandbox/service-sandbox-session-model.md`
  - `docs/architecture/sandbox/microvm-service-baseline.md`
  - `docs/plans/nimbus-sdk-resource-model-plan.md`
  - `docs/plans/nimbus-capability-segregation-plan.md`
  - `docs/private/agentic-ai-tco.md`
  - relevant compose, sandbox, and service-control docs
- Steps:
  - Replace "service implementation" where it means catalog payload with
    "service backend".
  - Replace sandbox-backed launch-spec examples with `SandboxSpec` plus
    `SandboxRootSpec`.
  - Replace private-doc `ctx.services` examples with explicit SDK usage:
    `import { Nimbus } from "@nimbus/nimbus"; const nimbus = new Nimbus();`
    followed by `nimbus.services.start({ name, waitUntil: "ready" })`.
  - Keep user-facing SDK examples at the service/sandbox/session resource level;
    do not expose low-level backend or root-spec details unless the example is
    about operator/compose configuration.
- Gate: docs validation passes and stale-name search returns only archived
  history or deliberate migration notes in this plan.

### SBR6 - Verification And Closeout

- Goal: make the refactor mechanically durable.
- Files: focused tests and verifier scripts as needed.
- Required focused gates:
  - `cargo test -p nimbus-sandbox`
  - `cargo test -p nimbus-services`
  - `cargo test -p nimbus-bin compose`
  - `cargo test -p nimbus-server service_manager -- --nocapture`
  - `npm run docs:validate-refs:strict`
  - `git diff --check`
- Stale-name guards:
  - production code has no `ServiceImplementation`
  - production code has no `SandboxBackedServiceImplementation`
  - production code has no `SandboxImageLaunchSpec`
  - production code has no `SandboxBuildLaunchSpec`
  - production code has no `SandboxImageProcessOverrides`
  - production build inputs use `SandboxOciImageSource::Build` and
    `SandboxOciBuildSpec`, not generic root-level `Build` variants
  - production code has no `start_from_image` or `start_from_build`
  - tests use `workload_identity_*` names, not
    `tenant_workload_identity_*`
  - machine API client code uses `service_sandbox_process_snapshot(...)`, not
    `service_process_snapshot(...)`
  - private docs have no `ctx.services` examples
- Gate: all required focused gates pass, stale-name guards pass, and the plan
  execution log records exact commands and outcomes.

## Verifiable Success Criteria

1. Service catalog APIs use `ServiceBackend` and declarative `Spec` payloads.
2. `ServiceBackend::Sandbox` carries a complete `SandboxSpec`.
3. `SandboxSpec` carries owner metadata through `SandboxOwnerSpec`, backend
   selection through `SandboxBackendKind`, and root materialization through
   `SandboxRootSpec`.
4. Dockerfile/context build input is represented as
   `SandboxRootSpec::OciImage(SandboxOciImageSpec { source:
   SandboxOciImageSource::Build(SandboxOciBuildSpec), ... })` and is
   policy-gated. Local/dev builds remain explicit exceptions; production builds
   require operator-owned provenance/admission before they are allowed.
5. `SandboxBackend` exposes one start path: `start(SandboxSpec)`.
6. Existing container, krun, and forwarded machine API paths preserve behavior
   through the new spec shape.
7. Compose services still lower to named services, but the hidden sandbox detail
   is represented by `ServiceBackend::Sandbox(SandboxSpec)`.
8. Built-in and external services remain declarative specs and do not imply
   process ownership.
9. Adapter-created contexts remain adapter-shaped.
10. Runtime invocation isolates remain runtime internals, not SDK sandbox
    resources.
11. Old names are absent from production code.
12. Focused Rust/docs gates pass with evidence.

## Execution Log

| Date | Phase | Outcome | Verification | Next step |
| --- | --- | --- | --- | --- |
| 2026-06-08 | Plan creation | plan-only | `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `git diff --check -- docs/plans/service-backend-and-sandbox-spec-refactor-plan.md docs/plans/README.md` pass | Start SBR0/SBR1 when ready to implement the breaking refactor |
| 2026-06-08 | Build input model correction | plan-only | Repo inspection found real current build-backed paths (`SandboxBuildLaunchSpec`, Compose `build:`, machine API `service-sandboxes.build-start`, and container/krun Dockerfile/context preparation), but production admission rejects local builds by default and export paths tell operators to build/tag first. This row originally removed build from the target model; it is superseded by the build-root corrections below. | Start SBR0/SBR1 after this plan reflects nested OCI image build input. |
| 2026-06-08 | Build root correction | plan-only | Reconciled the plan with the current build path: Compose `build:` and macOS machine `service-sandboxes.build-start` are real local/dev service-sandbox inputs. This row originally kept build as a root-level `SandboxRootSpec::OciBuild`; it is superseded by the nested OCI image correction below. | Start SBR0/SBR1 after this plan reflects `SandboxOciImageSource::Build`. |
| 2026-06-08 | Nested OCI image build correction | plan-only | Clarified the final hierarchy: `SandboxRootSpec` has `Rootfs` and `OciImage`; `SandboxOciImageSpec` has `source: SandboxOciImageSource`; `SandboxOciImageSource` has `Reference` and `Build`. Build is now an OCI image materialization input, not a peer root kind. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); tracked `git diff --check -- docs/plans/service-backend-and-sandbox-spec-refactor-plan.md docs/plans/nimbus-sdk-resource-model-plan.md docs/plans/README.md` pass; untracked no-index whitespace checks for this plan and `docs/plans/nimbus-sdk-resource-model-plan.md` produced no diagnostics. | Start SBR0/SBR1 when ready to implement the breaking refactor. |
| 2026-06-08 | OCI image naming sweep | plan-only | Repo sweep confirmed `OciImage` is not acronym stutter because OCI means Open Container Initiative, not Open Container Image. Kept existing `OciImage*` code names, but refined the new plan shape so `Build` and `Reference` live under `SandboxOciImageSource` instead of making `SandboxOciImageSpec` itself an enum. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `git diff --check -- docs/plans/README.md` pass; untracked no-index whitespace checks for this plan and `docs/plans/nimbus-sdk-resource-model-plan.md` produced no diagnostics. | Start SBR0/SBR1 when ready to implement the breaking refactor. |
| 2026-06-08 | Architecture propagation | plan-only | Propagated the nested root/source decision into `docs/architecture/sandbox/service-sandbox-session-model.md`, `ARCHITECTURE.md`, and `docs/operating/cli.md`. The docs now state that `OciImage` is not acronym stutter, `image:` lowers to an OCI image reference source, `build:` lowers to an OCI image build source only when admitted, and production local builds remain fail-closed by default. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); `git diff --check -- ARCHITECTURE.md docs/architecture/sandbox/service-sandbox-session-model.md docs/operating/cli.md docs/plans/README.md` pass; untracked no-index whitespace checks for this plan and `docs/plans/nimbus-sdk-resource-model-plan.md` produced no diagnostics. | Start SBR0/SBR1 when ready to implement the breaking refactor. |
| 2026-06-08 | SBR0 registration closeout | done | Marked SBR0 done because the plan is registered in `docs/plans/README.md`, linked to owning docs, and the docs validation/whitespace checks above pass. | Start SBR1 when ready to implement the breaking refactor. |
| 2026-06-08 | Final audit cleanup | plan-only | Review tightened the remaining backend/implementation wording and replaced ambiguous `SandboxSpec.name` in the target model with `SandboxOwnerSpec`, so service-backed sandboxes can record owning service metadata without becoming name-addressed resources. Verification: `npm run docs:validate-refs:strict` pass (246 working-tree Markdown files); targeted `git diff --check` pass for touched tracked docs; no-index whitespace checks for new docs produced no diagnostics. | Start SBR1 when ready to implement the breaking refactor. |

## /goal Prompt

```text
/goal Complete docs/plans/service-backend-and-sandbox-spec-refactor-plan.md autonomously.

Use the plan as the control plane. First inspect git status --short and preserve
unrelated user or agent changes. This repo has not launched, so make direct
breaking renames instead of compatibility aliases.

Execute SBR0-SBR6 in order. Preserve the final model:
Service -> ServiceBackend -> ServiceBackend::Sandbox(SandboxSpec) ->
SandboxSpec.backend: SandboxBackendKind -> SandboxBackend::start(SandboxSpec).
Use BuiltInServiceSpec and ExternalServiceSpec for declarative payloads. Use
SandboxOwnerSpec for service ownership metadata without adding sandbox name
lookup. Move root materialization into SandboxSpec.root via SandboxRootSpec.
Model existing image references as
SandboxRootSpec::OciImage(SandboxOciImageSpec { source:
SandboxOciImageSource::Reference(...), ... }) and
Dockerfile/context build input as
SandboxRootSpec::OciImage(SandboxOciImageSpec { source:
SandboxOciImageSource::Build(SandboxOciBuildSpec), ... }). Build is
policy-gated and fails closed in production unless an operator-owned build
provenance/admission story is configured. Do not keep SandboxImageLaunchSpec,
SandboxBuildLaunchSpec, SandboxImageProcessOverrides,
SandboxBackedServiceImplementation, start_from_image, or start_from_build in
production code. Do not add adapter ctx shortcuts. Do not make runtime
invocation isolates SDK sandboxes.

This is the pre-SRM foundation. Finish SBR1-SBR5 before implementing
nimbus-sdk-resource-model SRM2-SRM5. SRM0/SRM1 in the SDK plan may only
bootstrap and verify existing service lifecycle/status SDK behavior while this
refactor is still incomplete.

Also complete the cleanup items: replace private-doc `ctx.services` examples
with explicit `Nimbus` SDK usage, rename `tenant_workload_identity_*` tests to
`workload_identity_*`, and rename `service_process_snapshot(...)` to
`service_sandbox_process_snapshot(...)`.

After each phase, update the phase status ledger and execution log with exact
commands and outcomes. Final closeout requires the focused Rust gates listed in
SBR6, npm run docs:validate-refs:strict, git diff --check, and stale-name guard
evidence.
```
