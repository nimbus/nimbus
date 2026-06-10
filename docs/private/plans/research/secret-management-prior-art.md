# Research: Secret Management Prior Art

Comprehensive survey of how successful secret-management systems handle
multi-backend adapters, references, caching, audit, workload identity,
and the boundary between key-management and secret-management. Pairs
with the deferred execution plan at
`docs/plans/secret-management-plan.md`.

This document captures durable findings only. It is not progress state
and does not own execution sequencing.

---

## Why This Question Exists For Nimbus

Nimbus runs in radically different environments — a developer laptop,
a single VPS, a GCP project with Workload Identity, an EKS cluster
with IRSA, an on-prem K8s with Vault, an Azure VM with Managed
Identity, a CI runner with static credentials. Each environment has a
*different* canonical secret manager, and most non-trivial deployments
will *combine* several (e.g., per-tenant overrides where Tenant A uses
GCP Secret Manager but Tenant B uses HashiCorp Vault).

A secret-management surface that supports only one backend forces
operators to either (a) duplicate secrets into Nimbus's store on every
rotation, (b) wrap a custom integration around their existing
infrastructure, or (c) abandon Nimbus's secret API and stuff secrets
into environment variables. All three are failure modes that
production-grade open source projects (Vault, External Secrets
Operator, SOPS, Doppler) have learned to avoid. Nimbus should learn
from them rather than relearn.

The architectural question this note answers: **what does a
multi-backend secret-management surface look like when the predominant
open-source projects have already converged on patterns?**

---

## Two Layers Often Confused

A common source of design confusion in this space is the difference
between **key management** and **secret management**. They are
adjacent but not identical.

| Layer | What it does | What it returns | Examples |
|---|---|---|---|
| **Key management (KMS)** | Wrap/unwrap data-encryption keys, sign, encrypt/decrypt envelope, manage HSM-backed roots | Cryptographic operations on opaque keys (you never see the key material) | AWS KMS, GCP KMS, Azure Key Vault (Keys), HashiCorp Vault (Transit engine), HSMs |
| **Secret management (SM)** | Store small values (API keys, passwords, certs) and return them on demand | Plaintext byte strings to authorised callers | AWS Secrets Manager, GCP Secret Manager, Azure Key Vault (Secrets), HashiCorp Vault (KV/dynamic engines), Doppler, Infisical |

Most products ship both (Azure Key Vault, AWS, GCP, HashiCorp Vault),
but the APIs are different. Nimbus already has KMS via
`docs/architecture/storage/encryption.md` for wrapping *its own
storage's* DEKs. This plan is about the *application secret* layer
that sits on top — what `ctx.secret.get("openai_api_key")` returns to
function code.

The two layers share **authentication infrastructure** (the same
workload identity that talks to KMS also talks to Secrets Manager) but
have **separate APIs**. Keep them separate in design; share the
plumbing where natural.

---

## Survey Of Prior Art

### HashiCorp Vault — the canonical reference

Vault is the most-studied open-source secret-management system. Its
architecture decisions inform every other system in this survey, even
those built to compete with it.

**Layered architecture:**

```text
   ┌──────────────────────────────────────┐
   │ HTTP/RPC API                         │
   ├──────────────────────────────────────┤
   │ Auth methods │ Audit devices         │
   ├──────────────────────────────────────┤
   │ Policy engine (ACL)                  │
   ├──────────────────────────────────────┤
   │ Token store · Lease manager · Core   │
   ├──────────────────────────────────────┤
   │ Secret engines (KV, AWS, PKI, …)     │
   ├──────────────────────────────────────┤
   │ Barrier — AES-GCM encryption layer   │
   ├──────────────────────────────────────┤
   │ Storage backend (Consul, file, …)    │
   └──────────────────────────────────────┘
```

Key decisions worth borrowing:

| Decision | Why it matters |
|---|---|
| **The barrier is the encryption boundary.** All data written below the barrier is encrypted; storage backends are untrusted. | Decouples the durability layer from the security layer. Lets Vault run on Consul, etcd, S3, filesystem — any backend that can write bytes. |
| **Storage backends and secret engines are separate.** The first is *where Vault keeps its own state*; the second is *what Vault returns to clients*. | A single trait per role. Storage backends implement `physical.Backend` (`Get`, `Put`, `Delete`, `List`). Secret engines implement `logical.Backend` (`HandleRequest`, `HandleExistenceCheck`). |
| **Plugin model: everything is a plugin.** Auth methods, secret engines, audit devices, even storage backends. | Adding a new cloud provider is a plugin, not a fork. Nimbus's `SecretProvider` trait should mirror this. |
| **Lease-based dynamic credentials.** Vault's AWS secret engine *generates* IAM users on demand with attached leases; on lease expiry, Vault revokes the IAM user. | Dynamic credentials are *more secure* than static rotation because the credential's lifetime is bounded by the lease. Nimbus's API must distinguish static-vs-dynamic-vs-leased. |
| **Audit failure is fatal.** "If any audit device cannot record a request, Vault refuses to service it." | If your audit pipe is down, you must not silently lose visibility. Security-critical. |
| **Seal/unseal via Shamir.** Vault starts sealed; unsealing requires a quorum of key shards (Shamir's Secret Sharing). | This is overkill for Nimbus's single-binary local default, but the *seal* concept (no plaintext key material on disk at rest) is worth preserving via the existing KMS DEK envelope. |
| **Policy is path-based.** `path "secret/data/prod/*" { capabilities = ["read"] }`. | Fine-grained capabilities mapped to URI-like paths. Maps cleanly to Nimbus's permission-model `secret` grant if extended to allowlist *paths* rather than bare names. |

**Auth methods worth knowing exist:**

`Token` (the always-present primitive), `AppRole` (machine-to-machine
with role_id + secret_id), `Kubernetes` (service-account token →
Vault token), `JWT/OIDC` (federate from any IdP), `AWS` (IAM identity
via STS), `Azure` (Managed Identity), `GCP` (service account →
identity token), `LDAP`, `Userpass`, `TLS certificates`, `Okta`,
`GitHub`.

The lesson for Nimbus: **the auth-method abstraction is its own thing,
separate from the secret-engine abstraction.** A `VaultProvider`
implementation in Nimbus needs an `AuthMethod` sub-trait the operator
can swap.

**The lease model in one paragraph:**

When a Vault client reads a dynamic secret (e.g., a generated AWS
access key), Vault returns the value *plus* a lease: `lease_id`,
`lease_duration`, `renewable`. The client uses the secret until lease
expiry. Before expiry, the client `POST /sys/leases/renew` to extend.
On revocation (explicit or expiry), Vault undoes the side effect (the
AWS engine calls `DeleteUser`). This is the most security-impactful
feature in the survey — dynamic secrets reduce blast radius
dramatically — and Nimbus should design for it from the start even if
MVP only ships static reads.

### External Secrets Operator (ESO) — the multi-backend gold standard

ESO is the canonical Kubernetes-native multi-backend secret bridge.
The patterns it pioneered are now standard across the industry. ESO's
own docs (`https://external-secrets.io/main/api/components/`) list
50+ supported backends — the largest such list in any open-source
project today.

**Core CRD model:**

| CRD | What it owns | Lifecycle |
|---|---|---|
| `SecretStore` / `ClusterSecretStore` | A *backend* — auth config + provider type. Scoped to namespace (SecretStore) or cluster-wide (ClusterSecretStore). | Long-lived; created by operators. |
| `ExternalSecret` / `ClusterExternalSecret` | A *request* — references a `SecretStore`, declares which remote keys to fetch, and how to map them into a K8s Secret. | Long-lived; reconciles on refresh interval. |
| `PushSecret` / `ClusterPushSecret` | Reverse direction — push a K8s Secret out to a remote backend. | Recent addition; useful for bootstrapping. |

The separation between *store* (a backend) and *secret* (a request)
is the single most important pattern in this whole document. **A
secret reference is not just a path; it is a `(store, path)` pair.**
This is what lets one cluster fetch some secrets from AWS, others
from Vault, others from GCP — all transparent to the consumer.

**The provider interface (ESO's Go SecretsClient):**

```text
type SecretsClient interface {
    GetSecret(ctx, ref) -> []byte               // single value
    GetSecretMap(ctx, ref) -> map[string][]byte // structured (e.g. JSON)
    GetAllSecrets(ctx, ref) -> map[string][]byte // by tag/selector
    PushSecret(ctx, value, ref) -> error        // optional
    DeleteSecret(ctx, ref) -> error             // optional
    Validate() -> ValidationResult
    Close(ctx) -> error
}
```

This is the *narrow* trait. Optional capabilities (push, delete) are
runtime-introspected. Worth mirroring almost exactly in Nimbus.

**`remoteRef` data model:**

```yaml
remoteRef:
  key: "prod/database/password"   # the path
  property: "value"                # optional projection into JSON
  version: "v3"                    # optional version pin
  decodingStrategy: Base64        # optional decode
  conversionStrategy: Default      # optional transform
```

Every field a Nimbus reference would want: path, property, version,
encoding hint. Treat as the canonical shape.

**Templating:**

ESO supports Go-template materialisation: combine N remote refs into
one rendered K8s Secret. This is operationally powerful — a
single ExternalSecret can pull a username from one backend, a password
from another, and template them into a single connection string. The
function-facing analog for Nimbus would be `ctx.secret.template(...)`
or per-reference projections; defer past MVP.

**Refresh strategy:**

ESO defaults to 1-hour `refreshInterval`. Can set to `0` to disable
refresh (one-shot). Can be triggered by webhook for push-style
invalidation. The polling model is simple and works; webhook
invalidation is the optimisation.

**Backends ESO supports (the list to aspire to, eventually):**

AWS Secrets Manager, AWS Parameter Store, GCP Secret Manager,
Azure Key Vault, HashiCorp Vault (KV, dynamic), Akeyless, IBM
Secrets Manager, 1Password, Bitwarden, Doppler, Pulumi ESC,
CyberArk Conjur, GitLab CI Variables, GitHub Actions Variables,
Fortanix DSM, Kubernetes (in-cluster Secrets), Webhook (custom
HTTP), File, KeeperSecurity, Oracle Vault, Senhasegura, OnePassword
Connect, Beyondtrust, Delinea/Thycotic, Passbolt, Yandex Lockbox,
Alibaba Cloud KMS, Tencent Cloud SSM, Scaleway, Linode, Infisical,
Cloudflare Workers Secrets, NetApp.

Nimbus does not need all 50+. MVP that covers AWS, GCP, Azure,
Vault, K8s, and a local file backend ships ~95% of real-world
deployments.

### AWS Secrets Manager + AWS Systems Manager Parameter Store

Two related AWS services with overlapping use cases:

| | Secrets Manager | Parameter Store |
|---|---|---|
| Built for | True secrets (DB passwords, API keys) | Configuration + secrets (SecureString) |
| Rotation | Built-in via Lambda | None native |
| Versioning | Yes | Yes (history) |
| Per-secret pricing | $0.40/secret/month | Free tier, then small charge |
| Cross-region replication | Built-in | Manual |
| Resource policy | Yes | Yes (via IAM) |

Both are accessed via the AWS SDK and authenticated via the standard
AWS IAM model (workload identity via IRSA on EKS, EC2 IMDS, AWS SSO,
or static access keys in dev). Nimbus should ship one
`AwsSecretsManagerProvider` that handles both — Parameter Store
SecureString is just a path-prefix routing decision.

**Workload identity on AWS:**

- **EKS:** IAM Roles for Service Accounts (IRSA) — pod's service
  account is annotated with an IAM role; AWS SDK picks it up via
  projected token + STS AssumeRoleWithWebIdentity.
- **EC2:** Instance metadata service (IMDSv2) — SDK auto-discovers
  credentials from `169.254.169.254`.
- **ECS/Fargate:** Task role via the same metadata mechanism on a
  different IP.
- **AWS SSO / IAM Identity Center:** for developer machines, the
  SDK uses `~/.aws/sso/cache/` tokens.
- **Static creds:** `~/.aws/credentials` or env vars — dev only.

The AWS SDK chain handles all of this transparently. The provider
just initialises the SDK; the SDK finds creds. Nimbus inherits this
for free.

### GCP Secret Manager

- Simple model: per-project `secrets/$name/versions/$version`.
- Versioning is first-class. Disabling/destroying old versions is
  explicit.
- Replication policy per secret: automatic (multi-region) or
  user-managed (pick regions).
- IAM-controlled at the secret level (resource-level IAM).
- Workload identity: GKE Workload Identity Federation, Cloud Run /
  Cloud Functions service accounts via the metadata server,
  Application Default Credentials for dev.
- No native rotation engine (unlike AWS); operators rotate via Cloud
  Functions or scheduled jobs.

### Azure Key Vault (Secrets surface)

- Single service for keys, secrets, and certificates — confusing
  because the *Secrets* API is one of three surfaces.
- Hierarchical: `https://<vault>.vault.azure.net/secrets/$name/$version`.
- Auth via Managed Identity (Azure VMs, AKS, App Service), Service
  Principal, or Azure CLI for dev.
- Versioning baked in; soft-delete + purge protection.
- Network ACLs (VNet, firewall rules) at the vault level.

The shape difference from AWS/GCP: Azure puts the vault name in the
URL, so a Nimbus provider needs `vault_url` or `vault_name` as part
of its `SecretStore` config, not just a region.

### Kubernetes Secrets (the in-cluster baseline)

- Native K8s primitive: a `Secret` is a base64-encoded map of
  key→value, etcd-stored.
- Etcd encryption-at-rest is a *configuration*, not a default;
  many real clusters store secrets in plaintext-on-disk via etcd.
- ServiceAccount tokens are also Secrets in pre-1.24 K8s; this is
  the bootstrap chain.
- No versioning, no rotation, no audit beyond the K8s API audit log.
- Useful as a **last-mile delivery target** (a Pod mounts a Secret as
  files or env vars), not as a primary secret store.

ESO's pattern is to *write into* K8s Secrets from remote backends, so
applications keep using the K8s primitive while the source of truth
lives elsewhere. Nimbus is not K8s, so we don't have this constraint —
but a `KubernetesSecretProvider` is useful when Nimbus runs inside K8s
and operators want to use the existing K8s-Secret-managed-by-ESO
infrastructure as Nimbus's backend.

### SOPS (Mozilla / CNCF)

- Encrypts files at rest using AWS KMS, GCP KMS, Azure Key Vault, age,
  or PGP.
- Different shape: not a network service. SOPS is `git`-friendly —
  encrypted YAML/JSON in your repo; decrypt in CI or at deploy time.
- Lessons for Nimbus:
  - The **file-as-secret-store** pattern is valid for some users.
  - The provider abstraction can target *key services for unwrapping*
    rather than *secret stores for fetching* — same auth surface,
    different role.
  - SOPS files in git + Nimbus loading them at start = a viable
    "no external secret store" deployment for small users. Worth
    shipping as a `SopsFileProvider`.

### sealed-secrets (Bitnami)

- K8s-only. Asymmetric encryption: public key in cluster, private key
  only the controller has. Encrypted secrets safe in git.
- Different shape from ESO (in-cluster decryption, not remote fetch).
- Not directly applicable to Nimbus but reinforces the **encrypted-at-
  rest, public-key-in-public** model that's worth knowing exists.

### SaaS Secret Managers

Doppler, Infisical, 1Password Secrets Automation, Bitwarden Secrets
Manager, Akeyless, Pulumi ESC. They share a shape:

- API-based fetch, often with per-project namespacing.
- SDKs in major languages.
- Service-account auth (token issued out-of-band).
- Often have CLI integration (`doppler run --` style wrappers).
- Audit logs in their UI.

Nimbus should ship at least one (Doppler is the most-asked-for in
small-to-mid teams). The provider shape is identical to the cloud
providers — just HTTP API + token auth.

### CyberArk Conjur, Akeyless, Delinea — enterprise

- Policy-rich; declarative access control.
- Often paired with regulated-industry deployments.
- Lessons: the *policy* surface is sometimes more important than the
  *secret* surface for enterprise buyers. Nimbus's
  permission-model `secret` grant is the right hook — it can later
  carry policy bound to the secret reference if needed.

---

## Patterns Common To All

Every system in the survey shares the following. Nimbus should match
unless there is a *specific* reason not to.

### 1. The (store, reference) pair

A secret is *not* identified by path alone. It is identified by `(which
backend, what path)`. The store carries auth and backend type; the
reference carries the path, optional projection, optional version.
ESO's `SecretStore + remoteRef` is the cleanest expression. Vault's
mount points + paths are an inline equivalent.

### 2. Narrow provider trait + optional capabilities

The required trait surface is small: `get`, `get_map`, `get_all`,
`close`. Optional capabilities — push, delete, watch, rotate — are
introspectable per provider. Don't put rare capabilities in the core
trait; gate them behind an `Optional<Capability>` introspection.

### 3. References, not literal values

Consumer code passes references; resolution happens in a trusted
boundary. The plaintext never appears in source, configuration files
committed to git, journal entries, log lines, or function-readable
return values until the moment of use.

### 4. Workload identity is the production auth

Static credentials are dev-only. Production deployments use cloud
workload identity (IRSA, GKE WI, Azure Managed Identity, K8s SA
tokens) or Vault auth methods that federate from those. The provider
must support workload identity from day one.

### 5. Audit every read

Every secret access is logged with caller identity, secret reference,
version, timestamp — but never the value. This is the
trust-but-verify pattern; the existence of an audit trail is what
makes the rest of the system trustable.

### 6. Caching with TTL + invalidation

Pure pull-on-read is too expensive (network call every read). Pure
cache-forever is too dangerous (rotated secret is missed). The common
middle: TTL cache, with optional push invalidation when the backend
supports it. ESO's `refreshInterval` defaults to 1 hour.

### 7. Versioning is first-class

Every major secret manager versions secrets. The reference may pin a
version (deterministic rollouts) or use latest (transparent
rotation). Skipping versioning is technical debt that hurts later.

### 8. The seal/encrypt boundary

The store-of-record must be encrypted at rest with keys not stored
alongside it. Vault's barrier, AWS/GCP/Azure's service-managed
encryption, SOPS's KMS-wrapping — all express the same property.
Nimbus reuses `storage/encryption.md` for this; do not invent a new
KMS.

### 9. Rotation is a write that bumps version

Not a separate operation. Rotation = create new version → flip
"latest" pointer → optionally retire old version. This shape is
forced by versioning being first-class.

### 10. Dynamic > static where the backend supports it

A short-lived dynamically-generated credential is *strictly safer*
than a long-lived rotated one. Vault's dynamic engines, AWS STS
session tokens, GCP service account impersonation — all express the
"short-lived bounded-blast-radius" pattern. Nimbus's API must
distinguish so callers can opt in to leased credentials when
available.

---

## Patterns That Diverge

The interesting choices systems make differently:

| Choice | Options | Tradeoff |
|---|---|---|
| Materialisation surface | Env var injection (12-factor, Doppler, sealed-secrets), file-mount (CSI driver, sealed-secrets), in-process API (Vault SDK, ESO target K8s Secrets, AWS SDK direct) | Env-var is most compatible; API is most secure (lifetime-bounded, audit-precise). Nimbus's `ctx.secret.get` is API. |
| Push vs pull | Push (sealed-secrets, K8s Secret materialisation) vs pull (Vault SDK, AWS SDK direct, ESO controller-pulls-and-writes-K8s-Secret) | Pull keeps secrets out of the consumer environment when not in use; push makes secrets available even if the source is unreachable. ESO is hybrid. |
| Auth bootstrap | Workload identity (best), static creds (worst), one-time bootstrap token + token-renewal (Vault AppRole, middle ground) | Workload identity is the goal; design must not preclude it. |
| Secret lifecycle abstraction | Static (most), versioned (most), leased/dynamic (Vault, AWS STS), session (Cloudflare Workers per-invocation) | Lease is the gold standard but the rarest implementation; static is the floor. |
| Reference scheme | Path-only (Vault: `secret/data/prod/db`), path+property (ESO `key+property`), URI-based (`vault://prod/db?version=3#password`) | URI-based composes most cleanly; Nimbus should use it. |
| Multi-tenant model | Namespaces (Vault Enterprise), per-tenant store config (ESO), per-tenant API key (SaaS), no isolation (K8s Secrets in shared namespace) | Per-tenant store config is the most flexible; matches Nimbus's existing per-tenant model. |
| Backend discovery | Static config file (Vault, ESO SecretStore), service-discovery (rare), env vars (cloud SDKs) | Static config is the operator-controllable default; Nimbus's tenant config table is the home. |

---

## The Workload Identity Bootstrap Problem

Every secret manager has a chicken-and-egg: *to read a secret you
must authenticate; to authenticate you need credentials; if those
credentials are secrets, where do they come from?*

The chain ends at one of:

- **Cloud-provider attestation** — IRSA, GKE WI, Azure Managed
  Identity. The platform vouches for the workload via signed tokens;
  no credentials in source or config. **This is the goal.**
- **One-shot bootstrap token** — Vault AppRole (`secret_id` + `role_id`,
  the latter often baked into config). The token is itself a secret
  but it's the *only* one. Reduces blast radius without eliminating
  it.
- **Static credentials** — env vars, `~/.aws/credentials`, mounted
  files. Always the worst option; OK for dev.
- **mTLS via cluster-issued cert** — used by Vault's `cert` auth and
  service-mesh-issued identities. Strong but operationally heavy.

Nimbus must support all of these (the cloud providers expose them
trivially via their SDKs), but the production-recommended path must
be workload identity. The provider's auth-method abstraction is the
seam.

---

## Where Nimbus Is Different

The survey makes clear that the multi-backend pattern is well-
understood. What makes a Nimbus secret-management surface non-
redundant is the integration story, exactly mirroring the browser
plan's analysis:

1. **Engine-owned host bridge.** `ctx.secret.get(name)` reaches the
   secret store the same way `ctx.db.read(...)` reaches storage. No
   separate API, no separate auth.
2. **Capability admission gates the read at deploy time.** Pre-launch
   functions cannot read secrets they didn't declare; the existing
   `secret` grant becomes load-bearing.
3. **Mutation journal is the audit trail.** Reads journal through the
   same path as DB writes; no separate audit pipeline to operate.
4. **Multi-backend via `SecretProvider` trait + per-tenant routing.**
   ESO's SecretStore concept, expressed as native Nimbus tenant
   configuration in `_nimbus.secret_stores`.
5. **Encrypted-at-rest reuses `storage/encryption.md`.** No new KMS;
   the existing DEK envelope wraps stored secret values.
6. **Multi-node coordination reuses iroh + openraft.** Rotation
   invalidation rides iroh-gossip; the store registry rides openraft.
   Same pattern as the browser plan's session ownership.
7. **No separate operator infrastructure.** A Nimbus operator
   manages secrets through the same desktop UI / admin API as the
   rest of the system. No external Vault-style server unless the
   *operator chooses* to delegate to one via the `VaultProvider`.

These are *integration* wins, not *secret-management* wins. The
secret layer itself looks like everyone else's: trait + adapters +
references + versioning + workload identity.

---

## Decisions A Future Plan Must Make

Drawn from the survey above. The execution plan owns resolving these.

1. **Reference syntax.** URI-based (`store_name://path?version=N#field`)
   is the recommendation; explicitly committed at promotion.
2. **MVP provider set.** AWS Secrets Manager, GCP Secret Manager,
   Azure Key Vault, HashiCorp Vault, Kubernetes Secrets, local file,
   Nimbus-native (the default).
3. **Auth abstraction.** Provider-specific or unified? Recommendation:
   provider-specific (the cloud SDKs already abstract their own auth
   chains; trying to unify on top is reinvention).
4. **Lease/dynamic credentials.** MVP-ship static reads only; design
   the API to allow leases (return a `LeasedSecret` with
   `lease_duration` and `renewable` fields) so dynamic providers can
   be added without API churn.
5. **Materialisation surface.** API-only (`ctx.secret.get`); no env
   injection. (Pre-launch breaking change from any current env
   indirection.)
6. **Secret materialisation type in JS.** Plain `string` (simple,
   matches Convex's existing surface) or `Secret<string>` newtype that
   refuses to serialise into logs/responses? Recommendation: plain
   string for MVP; revisit once a real abuse pattern shows up.
7. **Push API.** Whether `ctx.secret.set` exists for function code
   (vs admin-only). Recommendation: admin-only by default; function-
   level write capability is a separate grant.
8. **Cache TTL default.** 5 minutes? 15? Should be configurable per
   secret. ESO default is 1 hour, biased toward batch consumers.
   For Nimbus's per-invocation cache, 5–15 minutes is more
   appropriate.
9. **Failure semantics.** If the backend is down, do reads fail or
   return cached values past TTL? Recommendation: return cached past
   TTL with a "stale" flag, fail only when no cache exists. This is
   the resilient choice and matches Vault's own caching behavior on
   client SDKs.
10. **Audit storage.** Journal entries or a dedicated table?
    Recommendation: journal for the read event, dedicated
    `_nimbus.audit_secret_reads` table for queryable retention. Two
    sinks, same write.

---

## References

External:

- HashiCorp Vault architecture:
  `https://developer.hashicorp.com/vault/docs/internals/architecture`
- HashiCorp Vault secret engines:
  `https://developer.hashicorp.com/vault/docs/secrets`
- External Secrets Operator:
  `https://external-secrets.io/main/`
- External Secrets Operator providers list:
  `https://external-secrets.io/main/provider/aws-secrets-manager/`
  (and sibling pages for the 50+ supported backends)
- AWS Secrets Manager:
  `https://docs.aws.amazon.com/secretsmanager/`
- GCP Secret Manager:
  `https://cloud.google.com/secret-manager/docs`
- Azure Key Vault Secrets:
  `https://learn.microsoft.com/en-us/azure/key-vault/secrets/`
- SOPS:
  `https://github.com/getsops/sops`
- sealed-secrets:
  `https://github.com/bitnami-labs/sealed-secrets`
- CSI Secrets Store Driver:
  `https://secrets-store-csi-driver.sigs.k8s.io/`
- Doppler:
  `https://docs.doppler.com/`
- Infisical:
  `https://infisical.com/docs/`
- 1Password Secrets Automation:
  `https://developer.1password.com/docs/secrets-automation/`
- Pulumi ESC:
  `https://www.pulumi.com/docs/esc/`
- Cloudflare Workers Secrets:
  `https://developers.cloudflare.com/workers/configuration/secrets/`
- Akeyless Distributed Fragments Cryptography:
  `https://docs.akeyless.io/docs/dfc`

Internal:

- `docs/plans/secret-management-plan.md` — the execution plan this
  research informs.
- `docs/architecture/runtime/permission-model.md` — defines the
  `secret` grant placeholder this plan makes load-bearing.
- `docs/architecture/storage/encryption.md` — defines the KMS DEK
  envelope reused for storing secret values at rest.
- `docs/architecture/horizontal-scaling.md` — defines the iroh +
  openraft cluster substrate used for multi-node rotation
  invalidation and store registry replication.
- `docs/plans/agent-browser-service-plan.md` — first known plan with
  a hard product need for tenant-scoped secrets (per-session policy
  credentials).
- `docs/plans/wasi-agent-capabilities-plan.md` — second known plan
  (external HTTP API keys for agent capabilities).
