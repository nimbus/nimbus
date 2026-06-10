# Plan: Tenant Secret Management

Canonical deferred design and execution plan for a tenant-scoped secret
management surface in Nimbus, with a multi-backend provider model so
Nimbus can run locally, on AWS, on GCP, on Azure, on Kubernetes, or
delegate to HashiCorp Vault.

This document owns the durable forward-looking context for the
`SecretProvider` trait, the `ctx.secret.*` host bridge, the `secret`
admission grant, the Nimbus-native secret store on `_nimbus.secrets`,
and the concrete provider adapters (AWS Secrets Manager, GCP Secret
Manager, Azure Key Vault, HashiCorp Vault, Kubernetes Secrets,
filesystem, Nimbus-native).

It supersedes the gap-identification note at
`docs/plans/research/secret-management-shape.md`. Prior-art research
that informs every decision below lives at
`docs/plans/research/secret-management-prior-art.md`.

---

## Status

- **Status:** `deferred`
- **Primary owner:** this plan
- **Activation gate:** promote only when at least one of the following
  becomes a concrete product requirement:
  1. `docs/plans/wasi-agent-capabilities-plan.md` Phase A2
     (`AgentOsProvider` + `nimbus:agent/http-client`) reaches `todo`
     ready-to-start status and a real consumer needs external API keys, or
  2. `docs/plans/agent-browser-service-plan.md` Phase B5
     (per-session policy credentials: proxy auth, client certs, CAPTCHA
     keys) reaches `todo` ready-to-start status, or
  3. The first paying tenant requests stronger-than-env-var handling
     for sensitive values (Stripe keys, OAuth client secrets, database
     URLs, LLM API keys) in tenant functions.
- This plan does **not** gate on any single sibling plan. It is the
  shared dependency the browser plan and the wasi-agent plan both
  call out as a hard prerequisite for their own credential-handling
  phases. Either consumer plan may pull this plan forward.

## How To Use This Plan

- Read this before starting any secret store, credential injection, or
  multi-backend integration work.
- Treat it as the canonical control plane for the secret-management
  workstream once promoted.
- Do not start implementation until the activation gate is met.
- When promoted, implement exactly one phase at a time and record
  verification in the Execution Log before marking a phase `done`.
- Cross-link the prior-art document
  (`docs/plans/research/secret-management-prior-art.md`) for any
  decision that requires "why" context; do not duplicate its content.

## Control Plan Rules

This document is the durable control plane for the secret-management
workstream. The source of truth is:

1. the current git worktree
2. this plan's `Phase Status Ledger`, `Implementation Checkpoints`, and
   `Execution Log`
3. `ARCHITECTURE.md` for the landed runtime architecture
4. `docs/architecture/runtime/permission-model.md` for the `secret`
   admission grant this plan makes load-bearing
5. `docs/architecture/storage/encryption.md` for the KMS DEK envelope
   this plan reuses to wrap stored secret values
6. `docs/architecture/horizontal-scaling.md` for the iroh + openraft
   substrate the multi-node phases ride on

Do not rely on prior chat transcripts as progress state.

### Status model

- `todo`: not started; eligible when hard dependencies and gate notes
  are satisfied
- `in_progress`: actively being implemented; keep exactly one phase in
  this state per autonomous execution run
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification has been recorded
- `deferred`: intentionally parked behind a product or benchmarking
  gate

### Recovery loop for every new session

1. Reread this `Control Plan Rules` section, `Phase Status Ledger`,
   `Implementation Checkpoints`, `Phase Order and Dependencies`, and
   `Execution Log`.
2. Inspect the current git worktree and reconcile it against this plan
   before picking new scope.
3. If any phase is already `in_progress`, resume that phase first.
4. If the worktree is dirty, identify which phase owns the changes and
   update that phase's checkpoint or log entry before starting new work.
5. Implement exactly one phase by default.
6. Record verification in `Execution Log` before marking a phase `done`.
7. If blocked, record the blocker here before stopping.

---

## Why Secret Management Needs A Plan

The `secret` grant in `docs/architecture/runtime/permission-model.md`
exists today as an admission declaration only. The doc states verbatim:

> Secret and identity grants are declaration and audit inputs until a
> future secret-store or service-identity API exists. Declaring a
> `secret` grant does not place secret material in `process.env` or
> globals.

Nimbus has key-management (the KMS DEK envelope from
`storage/encryption.md`, used to wrap pages of redb, the SQLite
replica, and encrypted blobs at rest). It does **not** have an
application-secret store. Two pending plans (browser service,
wasi-agent capabilities) both list secret management as a hard
dependency, and ordinary tenant functions that call OpenAI/Stripe/
Slack/databases have nowhere good to put credentials. The workaround —
plain env vars on tenant config — lacks audit, rotation, capability
gating, and per-function allowlist.

The prior-art survey at
`docs/plans/research/secret-management-prior-art.md` makes the answer
shape clear: a `SecretProvider` trait + a small set of cloud adapters +
references (not literal values) in function code + capability admission
+ encrypted-at-rest reuse of the existing KMS envelope. This plan
operationalises that shape.

### Why multi-backend from day one

Nimbus is intended to run in many deployment shapes: a developer
laptop, a single VM, an AWS EKS cluster, a GCP GKE cluster, an Azure
AKS cluster, an on-prem k8s cluster, or as a managed cloud service.
Each operator already has a secret-management surface: AWS Secrets
Manager, GCP Secret Manager, Azure Key Vault, HashiCorp Vault, raw
Kubernetes Secrets, or nothing yet. Forcing operators to mirror their
secrets into a Nimbus-only store is a non-starter and historically the
reason every multi-cloud platform ships a provider model (see External
Secrets Operator, Pulumi ESC, Vault's auth-method abstraction).

The plan therefore commits to the External-Secrets-Operator-style
adapter model from the start. The MVP backend is **Nimbus-native** on
`_nimbus.secrets` (so single-machine and cloudless deployments work
without external systems); cloud adapters land as additive providers
behind the same trait.

## Architecture Boundary

### What this plan owns

- The `SecretProvider` trait and its concrete implementations
  (`NimbusNativeProvider`, `FileProvider`, `AwsSecretsManagerProvider`,
  `GcpSecretManagerProvider`, `AzureKeyVaultProvider`, `VaultProvider`,
  `KubernetesSecretsProvider`).
- The `_nimbus.secrets` storage schema (versioned, KMS-wrapped values
  + audit table).
- The `_nimbus.secret_stores` storage schema (per-tenant routing
  configuration: which provider handles which secret namespace).
- The `ctx.secret.*` host-bridge surface for tenant functions.
- The `secret` admission grant promotion from placeholder to load-
  bearing (capability check at deploy time + at the host-bridge
  boundary).
- The reference syntax (`store://path?version=N#field`) that callers
  pass instead of literal values.
- The caching layer and rotation invalidation (single-node + multi-
  node via iroh-gossip).
- The audit pipeline for secret reads (journal entry +
  `_nimbus.audit_secret_reads` queryable retention table).
- Dynamic / leased credential support as a deferred sub-feature.

### What this plan does NOT own

- The KMS DEK envelope itself — that belongs to
  `docs/architecture/storage/encryption.md`. This plan **reuses** it as
  the wrapping primitive; no new KMS abstraction is introduced.
- The iroh + openraft cluster substrate — that belongs to
  `docs/architecture/horizontal-scaling.md`. This plan **rides** it;
  no new cluster primitives.
- The `secret` grant *syntax* in the permission model — that belongs
  to `docs/architecture/runtime/permission-model.md`. This plan
  promotes its semantics from declaration-only to enforced.
- The desktop operator UI itself — the operator console plan owns the
  shell; this plan owns the secret-management views that plug into it.
- Service identity / short-lived minted credentials (OIDC tokens,
  service-account tokens, mTLS certs). The permission model already
  notes these are a sibling concept to static secrets; they belong to
  `docs/plans/service-identity-provider-auth-plan.md`. This plan exposes
  a typed `LeasedSecret` so dynamic providers can be added later, but
  does not ship a service-identity grant.
- The wasi-agent `http-client` URL allowlist — that belongs to
  `wasi-agent-capabilities-plan.md`. This plan supplies the secret;
  the http-client decides where it's allowed to be sent.

## Reference Implementations

The prior-art survey at
`docs/plans/research/secret-management-prior-art.md` covers each of
these in depth. Summary of what each contributes to this plan:

| System | What it contributes |
|---|---|
| **External Secrets Operator** | The reference adapter model: `SecretStore` (per-tenant routing) + `SecretsClient` trait (`GetSecret`/`GetSecretMap`/`GetAllSecrets`/`Push`/`Delete`/`Validate`/`Close`). 50+ live backends prove the trait shape. |
| **HashiCorp Vault** | Barrier + storage backend + secret engine + auth method + audit layering; lease/renewal/revocation model for dynamic credentials; seal/unseal for at-rest protection. |
| **AWS Secrets Manager** | Versioning with staging labels (`AWSCURRENT`/`AWSPREVIOUS`); rotation-as-write; KMS-CMK-per-secret. Reference for the version model. |
| **GCP Secret Manager** | Resource-name addressing (`projects/.../secrets/.../versions/N`); IAM-based access; replication policy. Reference for URI-style references. |
| **Azure Key Vault** | Secret/key/certificate unified surface; managed-identity bootstrap; soft-delete/purge-protection lifecycle. Reference for soft-delete semantics. |
| **Kubernetes Secrets + CSI driver** | The minimum viable backend operators already have; baseline integration target. |
| **Cloudflare Workers Secrets** | Binding-at-deploy declarative model — closest analog to Nimbus's `secret` grant + reference scheme. |
| **Doppler / Infisical / 1Password SM** | SaaS reference UX (project/environment/config hierarchy); web UI patterns for the future operator console. |
| **SOPS / sealed-secrets** | Encrypted-at-rest-in-git pattern; not directly a backend but useful for the "encrypted bundle" import path. |
| **Pulumi ESC** | Hierarchical environment composition; reference for nested-environment overlays if Nimbus later adds environment promotion (dev → staging → prod). |

## Proposed Internal Shape

### Reference syntax

Function code never sees literal secrets. It passes references:

```text
store_name://path/to/secret?version=N#field
```

Examples:

```text
default://github/token                       // Nimbus-native, latest, single-value
default://stripe/keys#publishable            // Nimbus-native, latest, field of a map
aws://nimbus/prod/openai-key?version=3       // AWS Secrets Manager pinned to version 3
gcp://projects/foo/secrets/db-url            // GCP Secret Manager
vault://kv/data/prod/db#password             // Vault KV v2
k8s://default/db-credentials#password        // K8s Secret in `default` namespace
```

- `store_name` resolves through `_nimbus.secret_stores` for the
  invoking tenant.
- `path` is provider-opaque (each provider parses its own path
  segment).
- `version` is optional; default is "latest".
- `field` is the map key for providers that return key/value maps
  (mirrors ESO's `key` + `property` model).

Adopting URI-shaped references early is the recommendation in the
prior-art research (Decision 1).

### `SecretProvider` trait

```text
SecretProvider (trait, Send + Sync + 'static)
  // Required
  - get_secret(tenant, ref) -> Result<SecretValue>
  - validate(tenant) -> Result<()>          // store reachable, auth ok

  // Optional capabilities — providers advertise via a capability set
  - get_secret_map(tenant, ref) -> Result<BTreeMap<String, SecretValue>>
  - list(tenant, prefix) -> Result<Vec<SecretRef>>
  - push(tenant, ref, value) -> Result<u64>     // returns new version
  - delete(tenant, ref) -> Result<()>
  - rotate(tenant, ref) -> Result<u64>          // provider-driven rotation
  - lease(tenant, ref) -> Result<LeasedSecret>  // dynamic credentials
```

Modeled on External Secrets Operator's `SecretsClient` interface. The
mandatory surface is intentionally tiny (`get_secret` + `validate`); a
provider that supports only reads is still useful. Optional methods are
gated by a `Capabilities` bitset the provider advertises at
construction.

`SecretValue` is a small newtype around `Vec<u8>` with a `Drop` impl
that zeroizes memory. It does not implement `Display`, `Debug`
(non-redacted), or `Serialize` — callers must explicitly convert to
`&str` / `&[u8]` to use the value, mirroring the prior-art consensus
that the in-process type should resist accidental logging.

`LeasedSecret` carries `value: SecretValue`, `lease_duration:
Duration`, `lease_id: String`, `renewable: bool`. The MVP returns it
only from `VaultProvider`'s database-engine reads; other providers
return `Error::CapabilityUnsupported`.

### Concrete providers (MVP set)

| Provider | Backing | Auth | When to use |
|---|---|---|---|
| `NimbusNativeProvider` | `_nimbus.secrets` table, KMS-DEK-wrapped | n/a (in-process) | Default. Single-machine. Operators who want Nimbus to own the secret store outright. |
| `FileProvider` | Encrypted YAML/JSON file on disk (SOPS-compatible) | KMS key from `storage/encryption.md` | Local dev. Booting before any cloud secret store exists. CI fixtures. |
| `AwsSecretsManagerProvider` | AWS Secrets Manager API | AWS SDK default chain (IRSA, instance profile, env, profile) | Production on AWS. |
| `GcpSecretManagerProvider` | GCP Secret Manager API | Google Auth Library default chain (GKE WI, ADC, service account) | Production on GCP. |
| `AzureKeyVaultProvider` | Azure Key Vault Secrets API | DefaultAzureCredential chain (Managed Identity, env, CLI) | Production on Azure. |
| `VaultProvider` | HashiCorp Vault (KV v2 + database engine for leases) | AppRole / Kubernetes auth / token | Operators with an existing Vault. Required for dynamic credentials. |
| `KubernetesSecretsProvider` | K8s API `Secret` objects | In-cluster service account | Operators who want to keep secrets in their existing K8s store. |

The provider is selected per-tenant per-store via
`_nimbus.secret_stores`. Multiple stores can coexist for one tenant
(e.g. AWS for production secrets, Nimbus-native for development
secrets).

### Storage schema

```text
table _nimbus.secret_stores {
  tenant_id:    TenantId
  store_name:   string                 // user-facing store identifier (e.g. "default", "aws", "vault")
  provider:     string                 // discriminant: "nimbus" | "file" | "aws" | "gcp" | "azure" | "vault" | "k8s"
  config:       bytes                  // KMS-wrapped JSON: endpoint, region, auth method config, etc.
  capabilities: u32                    // bitset advertised at validation
  created_ms:   u64
  PK (tenant_id, store_name)
}

table _nimbus.secrets {                                 // backs NimbusNativeProvider only
  tenant_id:     TenantId
  path:          string                // tenant-scoped namespace, slash-delimited
  version:       u64                   // monotonic, starts at 1
  wrapped_value: bytes                 // KMS-DEK-wrapped (envelope from storage/encryption.md)
  dek_id:        DekId
  value_hash:    bytes                 // BLAKE3(plaintext); for audit, never for lookup
  fields:        json                  // optional map-of-fields metadata (key list, no values)
  created_ms:    u64
  created_by:    PrincipalId
  PK (tenant_id, path, version)
  INDEX (tenant_id, path) WHERE version = latest
}

table _nimbus.audit_secret_reads {
  tenant_id:    TenantId
  function_id:  FunctionId
  invocation:   InvocationId
  store_name:   string
  path:         string
  version:      u64
  ts_ms:        u64
  outcome:      enum { Ok, Denied, NotFound, ProviderError }
  PK (tenant_id, ts_ms, invocation)
  INDEX (tenant_id, path)
}
```

`_nimbus.secret_stores` is the analog of External Secrets Operator's
`SecretStore`/`ClusterSecretStore` CRDs, expressed as a Nimbus tenant
table. `_nimbus.secrets` is the analog of the K8s Secret that ESO
materializes; for Nimbus it is the canonical store rather than a
target. Both tables live under the `_nimbus` system tenant, mirroring
`_nimbus.browser_sessions` and the auth/identity tables.

Plaintext is **never** journaled. The mutation journal records writes
as `SecretSet { tenant_id, path, version, dek_id, value_hash }` — the
wrapped value lives in the row, never in the journal payload. This is
Required Invariant 4 below.

### Host-bridge API

```text
ctx.secret.get(ref: string) -> string
  // Resolves the reference, fetches from the routed provider, returns plaintext.
  // Audited. Capability-gated against the function's secret.allow_read list.

ctx.secret.get_bytes(ref: string) -> Uint8Array
  // Same as get() but for binary secrets (TLS keys, etc.).

ctx.secret.get_map(ref: string) -> Record<string, string>
  // For providers that expose key/value maps (Vault KV, AWS multi-field).
  // The ref is the map root (no #field). Capability check applies to the root.

ctx.secret.exists(ref: string) -> boolean
  // Capability-gated; useful for optional credentials.
```

Admin / management calls (`create`, `rotate`, `delete`, `set_store`)
live on the operator console / admin API, **never** on the function-
facing surface. Function code is read-only by default; a separate
`secret_admin` grant (deferred) can be introduced if a use case
emerges.

### Admission gate

Extends the existing `secret` grant in
`docs/architecture/runtime/permission-model.md` from declaration-only
to enforced:

```text
grant secret {
  allow_read: [
    "default://github/*",
    "default://openai/api_key",
    "aws://nimbus/prod/stripe-keys#publishable",
    "vault://kv/data/prod/db#password"
  ]
  // glob support per-component; full-URI matching; default deny
}
```

A function reading a reference not matched by `allow_read` fails at
the host-bridge boundary with a typed `CapabilityDeniedError`, not a
runtime panic. Mirror shape of `wasi-agent-capabilities-plan.md`'s
capability admission.

The admission check happens in **two** places:

1. **Deploy time** — bundle upload validates that every secret
   reference in the function's manifest matches an existing store and
   resolvable path (or warns if the path doesn't exist yet, in case
   the secret is created post-deploy). Wildcard entries (`default://
   github/*`) are accepted as-is.
2. **Invocation time** — every `ctx.secret.get(ref)` call checks the
   reference against the function's compiled allowlist. Failure is a
   typed error, not a panic.

This matches Cloudflare Workers' binding model (declared at deploy,
enforced at call) noted in the prior-art research.

### Caching and rotation invalidation

Two layers must be distinguished. The **persistent layer** is the
openraft-replicated `_nimbus.secrets` row (Nimbus-native provider
only); reads from this layer are strongly consistent via Raft. The
**hot cache layer** is the in-process KMS-unwrapped plaintext, held
per node, TTL-bound, and invalidated by gossip. Cache invalidation is
about the plaintext, not the row.

- **Per-invocation cache**: within a single invocation, repeated
  `ctx.secret.get(ref)` calls return the same value without re-fetching.
- **Per-process cache**: across invocations on the same node, secrets
  are cached with a configurable TTL (default 5 minutes per the prior-
  art recommendation).
- **Invalidation on write**: a local write through
  `NimbusNativeProvider` invalidates the local cache immediately.
- **Multi-node invalidation**: rotation invalidation rides iroh-gossip
  on the canonical `topic:<tenant_id>:secrets:<store_name>` topic
  (matches `topic:<tenant_id>:<resource>` convention from
  `docs/architecture/horizontal-scaling.md` §3). Each node hears
  "secret X bumped to version N" and invalidates its plaintext cache.
  The durable mapping rides openraft, not gossip — gossip carries only
  the invalidation signal. This is the same split the browser plan
  uses for session state (openraft for the durable
  `(tenant_id, session_id) → node_id` mapping; gossip for liveness and
  invalidation signals).
- **Backend-down resilience**: if the backing provider is unreachable
  and a cached value exists past its TTL, return the cached value with
  a `stale: true` flag and journal a `ProviderError` audit entry. If
  no cache exists, fail. This matches Vault SDK client behavior and is
  the recommendation in the prior-art research (Decision 9).
- **Invocation-failure semantics**: if the invocation node dies mid-
  fetch from an external backend, the normal invocation retry on
  another node re-fetches. There is no partial-state to migrate;
  external backends are the source of truth and the hot cache is
  rebuilt on demand.

### Cluster shape

Single-node: in-memory cache per process, invalidated on local write.

Multi-node (post `docs/architecture/horizontal-scaling.md`):

- `_nimbus.secret_stores` rides openraft-replicated metadata (small,
  infrequently written, frequently read — natural fit).
- `_nimbus.secrets` (Nimbus-native provider) rides openraft for
  writes; reads serve from the local per-node redb replica (the
  "redb + openraft" pattern from `horizontal-scaling.md` §7). This is
  the persistent layer; it is strongly consistent.
- Rotation invalidation rides iroh-gossip on
  `topic:<tenant_id>:secrets:<store_name>`. This is the hot cache
  layer invalidation channel; it does not carry secret material, only
  `(path, new_version)`.
- External backends (AWS/GCP/Azure/Vault/K8s) are accessed directly
  from the node handling the invocation; no cross-node fan-out. The
  external backend is the source of truth for its secrets; the node
  cache is the only Nimbus-side state. The single-writer rule does
  not apply — secret reads are stateless from Nimbus's perspective.

No new cluster primitive is introduced. The shared pattern across this
plan and `docs/plans/agent-browser-service-plan.md` is: **openraft for
the durable small-metadata mapping; iroh-blobs for any large content-
addressed payload (browser uses it; secrets do not — wrapped values
are small enough to ride the redb replica directly); iroh-gossip for
fast invalidation and liveness signals.** The browser plan applies it
to session ownership; this plan applies it to secret rotation.

**Multi-Raft forward note.** `horizontal-scaling.md` Open Question 1
calls out that at 10+ nodes the cluster will partition tenants into
separate Raft groups. When that partitioning lands, both
`_nimbus.secret_stores` and `_nimbus.secrets` rows are tenant-scoped
and naturally migrate into their tenant's Raft group; no plan-level
change is required, but the test for "every secret row carries a
`tenant_id`" should be enforced from S3 to make the future migration
mechanical.

### Sandbox integration

Secret resolution runs in the host (Rust) process, not in the V8 /
wasmtime sandbox. The resolved plaintext is passed across the linker
boundary as the function-call return value. The function sees only the
plaintext for that one call. The store auth credentials (AWS access
keys, Vault tokens, kubeconfig) **never** cross the sandbox boundary.

### Migration from existing env-var indirection

Pre-launch (per `AGENTS.md`), this is a breaking change with no
compatibility shim:

1. `ctx.env.get("FOO")` reads of sensitive values become
   `ctx.secret.get("default://foo")`.
2. The function's manifest gains a `secret.allow_read` entry.
3. Tenant operators move the value from env config into the
   Nimbus-native store (or wire up an external backend via
   `_nimbus.secret_stores` and reference it).
4. `ctx.env.get` remains for non-sensitive values (`LOG_LEVEL`,
   feature flags, etc.).

## Required Invariants

- The `SecretProvider` trait must be `Send + Sync + 'static` so it can
  be shared across workers.
- Plaintext secret values must never appear in the mutation journal.
  Writes journal only `(tenant_id, path, version, dek_id, value_hash)`;
  the wrapped value lives in the row.
- Every successful and failed `ctx.secret.get` call must journal an
  audit entry into `_nimbus.audit_secret_reads`. Audit failure must
  fail the read (audit is fatal, matching Vault's audit posture).
- Stored secret values must be wrapped using the existing KMS DEK
  envelope from `storage/encryption.md`. No new key-management
  primitive may be introduced by this plan.
- All secret references must be tenant-scoped. The tenant identifier
  participates in the lookup at every layer; cross-tenant reads are
  impossible by construction.
- Reading a reference not in the function's `secret.allow_read`
  allowlist must fail with a typed `CapabilityDeniedError` at the
  host-bridge boundary. No fallback to "deny silently and return
  empty."
- Backend auth credentials (AWS access keys, Vault tokens, kubeconfig,
  etc.) must never cross the V8 / wasmtime sandbox boundary. Only the
  resolved plaintext of the requested secret crosses; the credentials
  used to fetch it stay in the host.
- The `SecretValue` type must zeroize on `Drop`. It must not implement
  `Display`, unredacted `Debug`, or `Serialize`.
- A tenant without a matching `_nimbus.secret_stores` entry for the
  reference's `store_name` must fail at deploy time, not at invocation
  time.
- Rotation must be a write that bumps version (`v3` → `v4`); the
  "latest" pointer flips atomically with the write. Readers default to
  "latest" and see the new value on the next get after the local
  cache invalidates.
- Multi-node rotation invalidation must ride iroh-gossip on
  `topic:<tenant_id>:secrets:<store_name>` (the
  `topic:<tenant_id>:<resource>` convention from
  `docs/architecture/horizontal-scaling.md` §3, extended by
  `store_name`). The gossip payload must carry only
  `(path, new_version)` — never plaintext, never wrapped values. No
  new cluster primitive may be introduced.
- Capability decisions for `wasi-agent`'s `http-client` (allowlist of
  destination URLs) must remain orthogonal to the `secret` grant.
  Holding a secret reference does not authorize sending it anywhere;
  sending to a URL does not authorize reading a secret. Both checks
  must pass independently.

## Promotion Criteria

Promote this plan only if all of the following are true:

1. The activation gate above is met (at least one of the three
   consumer triggers).
2. `docs/architecture/storage/encryption.md`'s KMS DEK envelope is
   documented and stable; this plan reuses it without modification.
3. A decision has been recorded for the reference-syntax format. The
   recommendation is URI-shaped (`store://path?version=N#field`);
   committing at promotion time avoids churn during execution.
4. At least one concrete provider is scoped (the
   `NimbusNativeProvider` is the floor; cloud providers are optional
   at promotion).

## Phase Status Ledger

| Phase | Status | Summary | Hard Dependencies | Gate Note |
|-------|--------|---------|-------------------|-----------|
| S0 | `todo` | Decision gate, reference-syntax commitment, prior-art reread | activation gate satisfied | record committed reference syntax, MVP provider list, and reference-decision matrix from prior-art Decisions 1–10 in the Execution Log before any code |
| S1 | `todo` | `SecretProvider` trait, `SecretValue`, `LeasedSecret`, `SecretRef` parser, `Capabilities` bitset | S0 | trait + types live in a new `nimbus-secrets` crate; no providers wired yet; unit tests cover reference parsing, capability checks, zeroize-on-drop |
| S2 | `todo` | `FileProvider` (SOPS-compatible) for local dev and CI | S1 | YAML/JSON file backed; read-only; KMS-key decryption reuses `storage/encryption.md`; explicit non-production warning at config validation |
| S3 | `todo` | `_nimbus.secrets` + `_nimbus.secret_stores` schemas; `NimbusNativeProvider` | S1, KMS envelope stable | versioned writes; KMS-DEK-wrapped values; journal entries are `(path, version, dek_id, value_hash)` with no plaintext; admin-only `push`/`delete`/`rotate`; storage atomicity covered by engine mutation path |
| S4 | `todo` | `ctx.secret.*` host-bridge surface; admission promotion of `secret` grant | S1, S3, V8/wasmtime backends each support the host-bridge call shape | host-bridge resolves the reference, routes via `_nimbus.secret_stores`, calls provider; capability-check before call; typed errors; audit-on-every-read; per-invocation cache |
| S5 | `todo` | `AwsSecretsManagerProvider` | S1, S4 | AWS SDK default credential chain; IRSA tested as the production-recommended bootstrap; full integration test against localstack; backend-down resilience honored |
| S6 | `todo` | Multi-backend tenant routing (`_nimbus.secret_stores` write path + provider registry) | S3, S4, S5 | tenant operator can configure multiple stores; references resolve via store_name; admission gate validates `store_name` exists at deploy time |
| S7 | `todo` | Caching + rotation invalidation (single-node and multi-node) | S4, S6, iroh-gossip available | per-process cache with configurable TTL; local invalidation on write; multi-node invalidation via iroh-gossip on `topic:<tenant_id>:secrets:<store_name>`; backend-down stale-flag behavior; openraft replication for `_nimbus.secret_stores` and (for the Nimbus-native provider) `_nimbus.secrets` row replicas; two-layer (replica + cache) split honored |
| S8 | `todo` | `GcpSecretManagerProvider` | S5 (as template) | google-auth-library default chain; GKE WI tested as production-recommended bootstrap; integration test against an emulator or real GCP project |
| S9 | `todo` | `AzureKeyVaultProvider` | S5 (as template) | DefaultAzureCredential chain; Managed Identity tested as production-recommended bootstrap; soft-delete + purge-protection semantics documented |
| S10 | `todo` | `VaultProvider` (KV v2 + database engine for leases) | S5 (as template), `LeasedSecret` API from S1 | AppRole + Kubernetes auth methods; KV v2 read/write/version-pin; database secret-engine returns `LeasedSecret`; lease renewal driven by the per-process cache |
| S11 | `todo` | `KubernetesSecretsProvider` | S5 (as template) | in-cluster service-account auth; reads via the K8s API; cache TTL aligned with the K8s API server's etag/resourceVersion |
| S12 | `todo` | Operator UI / admin API for secret CRUD and store configuration | S3, S4, S6, the desktop operator console exists as a host | admin-only endpoints; UI hides plaintext after creation; rotation as a single button; store config UI matches `_nimbus.secret_stores` schema |
| S13 | `todo` | Dynamic / leased credentials (Vault database engine; AWS STS via Vault AWS engine) | S10, S7 (cache + invalidation) | `LeasedSecret` surface to function code; renewal driven by the cache; revocation on invocation completion if the lease is short enough; documented as an opt-in capability via the `secret.lease` allowlist |
| S14 | `todo` | End-to-end verification | S4, S6, and at least one cloud provider (S5/S8/S9/S10/S11) | full agent scenario: function declares `secret.allow_read`, deploys, reads a secret from AWS, reads a second secret from Vault, reads a third from Nimbus-native, all three audit entries land, rotation on one of them invalidates the cache, sandbox isolation test confirms backend auth never crosses |

## Phase Order and Dependencies

```text
S0 (decision gate)
  └── S1 (trait + types)
        ├── S2 (file provider — local dev unblock)
        ├── S3 (Nimbus-native provider + schemas)
        │     ├── S4 (host bridge + admission)
        │     │     ├── S5 (AWS provider)
        │     │     │     ├── S6 (multi-store routing)
        │     │     │     │     ├── S7 (caching + rotation invalidation)
        │     │     │     │     │     ├── S13 (dynamic / leased credentials; also needs S10)
        │     │     │     │     │     └── S14 (end-to-end verification)
        │     │     │     │     └── S12 (operator UI)
        │     │     │     ├── S8 (GCP)         // parallel
        │     │     │     ├── S9 (Azure)       // parallel
        │     │     │     ├── S10 (Vault)      // parallel
        │     │     │     └── S11 (K8s)        // parallel
        │     │     └── (independent of cloud providers)
        │     └── (S3 alone is enough to ship single-machine deployments)
        └── (S2 unblocks dev work in parallel with S3+)
```

Recommended delivery order: S0 → S1 → S2 → S3 → S4 → S5 → S6 → S7 →
{S8, S9, S10, S11 in parallel} → S12 → S13 → S14.

S8/S9/S10/S11 are independent after S5 establishes the cloud-provider
template and can run in parallel.

S12 (operator UI) can start as soon as S6 lands the data model; it
does not block other provider work.

S13 (dynamic credentials) is the only phase that materially changes
the host-bridge contract for function authors and is therefore last
before end-to-end verification.

## Implementation Checkpoints

| Phase | Checkpoint | Next Step |
|-------|------------|-----------|
| S0 | none yet | trigger on activation gate |
| S1 | none yet | |
| S2 | none yet | |
| S3 | none yet | |
| S4 | none yet | |
| S5 | none yet | |
| S6 | none yet | |
| S7 | none yet | |
| S8 | none yet | |
| S9 | none yet | |
| S10 | none yet | |
| S11 | none yet | |
| S12 | none yet | |
| S13 | none yet | |
| S14 | none yet | |

## Execution Log

| Date | Phase | Outcome | Summary | Verification | Next Step |
|------|-------|---------|---------|--------------|-----------|
| 2026-05-19 | meta | documented | Initial plan authored. Establishes a `SecretProvider` trait + URI-shaped references + capability-gated `ctx.secret.*` host bridge + `_nimbus.secrets` Nimbus-native storage with KMS-DEK-wrapped values + multi-backend routing via `_nimbus.secret_stores` + cache invalidation via iroh-gossip. MVP backend set: NimbusNative, File (SOPS-compatible), AWS Secrets Manager, GCP Secret Manager, Azure Key Vault, HashiCorp Vault, Kubernetes Secrets. Pattern adopted from External Secrets Operator (`SecretStore` + `SecretsClient` trait), Cloudflare Workers Secrets (binding-at-deploy admission), Vault (lease/renew/revoke for dynamic creds), AWS Secrets Manager (versioning + rotation-as-write). Supersedes `docs/plans/research/secret-management-shape.md`. | review against `docs/plans/research/secret-management-prior-art.md`, `docs/architecture/runtime/permission-model.md`, `docs/architecture/storage/encryption.md`, `docs/architecture/horizontal-scaling.md`; cross-reference with `docs/plans/agent-browser-service-plan.md` and `docs/plans/wasi-agent-capabilities-plan.md` | keep deferred until activation gate triggers |
| 2026-05-19 | meta | refined | Horizontal-scaling coherence audit. Fixed three issues introduced in the initial authoring: (a) gossip/openraft conflation — corrected the "same primitive as browser plan for session ownership" misclaim; the durable mapping rides openraft, gossip carries only invalidation signals; (b) topic naming normalised to the canonical `topic:<tenant_id>:<resource>` convention from `horizontal-scaling.md` §3 (now `topic:<tenant_id>:secrets:<store_name>`); (c) two-layer model (openraft-replicated row replica vs. in-process plaintext cache) made explicit so future implementers do not collapse them. Added multi-Raft forward note (Open Question 1 in horizontal-scaling.md) and invocation-failure semantics for external backends. | review against `docs/architecture/horizontal-scaling.md` §3, §7, Open Question 1, and the new Consumer Plans section; cross-checked with `docs/plans/agent-browser-service-plan.md` for shared-pattern claims | keep deferred until activation gate triggers |

## Verification Expectations

When promoted, the secret-management surface should not be considered
viable without:

- `SecretRef` parser tests (every URI shape; reject malformed
  references; reject cross-tenant references).
- `SecretValue` zeroize-on-drop test; refusal-to-serialize test.
- `NimbusNativeProvider` unit tests: versioned write, latest-pointer
  read, version-pinned read, KMS unwrap on read, audit entry on every
  read.
- Storage atomicity test: secret write + index update + journal entry
  in one transaction; failure rolls back all three.
- Admission gate tests: deploy accepted with valid `secret.allow_read`;
  deploy rejected when `store_name` doesn't exist; invocation read
  rejected when reference doesn't match allowlist; audit entry written
  on denied reads.
- Per-provider integration tests against the canonical backend (AWS:
  localstack; GCP: emulator or real project; Azure: real vault; Vault:
  dev-mode server; K8s: kind cluster).
- Workload-identity bootstrap test for at least one cloud provider
  (IRSA on AWS) demonstrating no static credentials in config.
- Rotation invalidation test: write bumps version; local cache
  invalidates; on multi-node, gossip propagates and remote node
  invalidates within bounded latency.
- Backend-down resilience test: cached value past TTL returned with
  `stale: true` flag; no cache → typed error.
- Sandbox isolation test: backend auth credentials are unreachable
  from V8 / wasmtime; only the resolved plaintext crosses the
  boundary.
- Dynamic credential lease test (Vault database engine): function
  receives `LeasedSecret`; lease is renewed by the cache; revocation
  on lease expiry; new lease minted on next read.
- End-to-end multi-store test: one function reads from three different
  stores; all three audit entries land; rotation on one does not
  invalidate caches of the others.
- V8 and wasmtime backend regression suites green after every phase.
- `make ci` green after every phase.

## Relationship To Other Plans

- **`docs/architecture/runtime/permission-model.md`**: this plan
  promotes the `secret` grant from declaration-only to enforced. The
  grant's syntax stays where it is; the enforcement semantics land
  here.
- **`docs/architecture/storage/encryption.md`**: hard prerequisite for
  S3 (Nimbus-native provider). The KMS DEK envelope wraps every
  stored secret value. This plan does **not** introduce a new KMS
  abstraction.
- **`docs/architecture/horizontal-scaling.md`**: substrate for S7
  (multi-node rotation invalidation rides iroh-gossip;
  `_nimbus.secret_stores` rides openraft-replicated metadata). No new
  cluster primitive introduced.
- **`docs/plans/agent-browser-service-plan.md`**: first known consumer.
  Per-session policy credentials (proxy auth, TLS client certs,
  CAPTCHA-service keys) become `SecretRef`s; the BrowserService
  resolves them inside the sandbox boundary. The browser plan's
  Phase B5 should not be considered viable without this plan promoted
  to at least Phase S4 (host bridge live) + one provider (S3 minimum,
  any of S5/S8/S9/S10 if the operator chooses a cloud backend).
- **`docs/plans/wasi-agent-capabilities-plan.md`**: second known
  consumer. `nimbus:agent/http-client` calls (LLM APIs, third-party
  services) read API keys via `ctx.secret.get`. The wasi-agent plan's
  http-client URL allowlist remains orthogonal to this plan's secret
  allowlist — both checks must pass independently. The browser plan
  and the wasi-agent plan are **sibling consumers**, not gate-and-
  substrate; neither blocks the other.
- **`docs/plans/wasmtime-backend-plan.md`**: no direct dependency.
  The `ctx.secret.*` host-bridge surface lands as an extension of the
  existing V8 `HostBridge` and the wasmtime `nimbus:host` interface,
  using the call shapes both backends already support. This plan
  does not introduce new WIT interfaces.
- **`docs/plans/research/secret-management-prior-art.md`**: prior-art
  research that informs every decision in this plan. Read it for
  "why X" context.
- **`docs/plans/research/secret-management-shape.md`**: superseded by
  this plan. The shape note was a gap-identification artifact; its
  content is now load-bearing here (Required Properties became
  Required Invariants; Rough Shape became Proposed Internal Shape;
  Open Questions became Phase S0 Decision Gate inputs). The shape
  note should be archived or redirected to point here.
- **`ARCHITECTURE.md`**: update when each phase lands, documenting
  the secret-management surface as a first-class engine subsystem
  rather than a permission-model placeholder.
- **`docs/plans/service-identity-provider-auth-plan.md`**: the permission
  model's `identity` grant is a sibling concept to `secret`. Short-lived
  minted credentials (OIDC tokens, service-account tokens, mTLS certs) belong
  to that plan, not this one. This plan's `LeasedSecret` shape is the seam
  that makes the transition non-breaking when service identity lands.
