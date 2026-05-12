# Encryption At Rest Plan

This is the canonical execution plan for optional, enterprise-ready encryption
at rest across Neovex-owned local persistence.

Canonical architecture baseline:

- [`docs/architecture/storage/encryption.md`](../architecture/storage/encryption.md)
  owns the durable design, diagrams, provider boundaries, and algorithm
  rationale
- this plan owns rollout order, acceptance criteria, and verification

Reviewed against:

- `ARCHITECTURE.md`
- `crates/neovex-engine/src/persistence_config.rs`
- `crates/neovex-storage/src/async_storage/engine.rs`
- `crates/neovex-storage/src/async_storage/control.rs`
- `crates/neovex-storage/src/store/write/store_entry.rs`
- `crates/neovex-storage/src/usage_store.rs`
- `crates/neovex-storage/src/sqlite.rs`
- `crates/neovex-storage/src/libsql.rs`
- `crates/neovex-bin/src/main.rs`

---

## References

### redb

- [redb `StorageBackend` trait](https://docs.rs/redb/2.6.3/redb/trait.StorageBackend.html)
- [redb issue #759 - Encryption at rest](https://github.com/cberner/redb/issues/759)
- [redb issue #1091 - Page level encryption?](https://github.com/cberner/redb/issues/1091)
- [redb design doc](https://github.com/cberner/redb/blob/master/docs/design.md)

### SQLite and libsql

- [SQLCipher design](https://www.zetetic.net/sqlcipher/design/)
- [SQLCipher API](https://www.zetetic.net/sqlcipher/sqlcipher-api/)
- [SQLite SEE overview](https://sqlite.org/com/see.html)
- [SQLite SEE docs](https://www.sqlite.org/see/doc/trunk/www/readme.wiki)
- [rusqlite feature flags](https://docs.rs/crate/rusqlite/latest/features)
- [libsqlite3-sys feature flags](https://docs.rs/crate/libsqlite3-sys/latest/features)
- [libsql Rust reference](https://docs.turso.tech/sdk/rust/reference)
- [Turso embedded replicas encryption docs](https://docs.turso.tech/features/embedded-replicas/introduction)
- [Turso native encryption docs](https://docs.turso.tech/tursodb/encryption)

### KMS

- [AWS KMS `Decrypt`](https://docs.aws.amazon.com/kms/latest/APIReference/API_Decrypt.html)
- [AWS KMS `ReEncrypt`](https://docs.aws.amazon.com/kms/latest/APIReference/API_ReEncrypt.html)
- [AWS KMS encryption context](https://docs.aws.amazon.com/kms/latest/developerguide/encrypt_context.html)

### Cryptographic standards and adjacent options

- [RFC 8452 - AES-GCM-SIV](https://www.rfc-editor.org/rfc/rfc8452.html)
- [NIST SP 800-38D - GCM](https://csrc.nist.gov/pubs/sp/800/38/d/final)
- [NIST SP 800-38F - AES Key Wrap](https://csrc.nist.gov/pubs/sp/800/38/f/final)
- [RFC 5297 - AES-SIV](https://www.rfc-editor.org/rfc/rfc5297)
- [Libsodium XChaCha20-Poly1305 guidance](https://libsodium.gitbook.io/doc/secret-key_cryptography/aead/chacha20-poly1305)
- [CFRG AEGIS draft](https://datatracker.ietf.org/doc/html/draft-irtf-cfrg-aegis-aead-18)
- [NIST SP 800-232 - Ascon lightweight crypto](https://csrc.nist.gov/pubs/sp/800/232/final)

---

## Purpose

Enterprise deployments expect encryption at rest to be available, explicit,
auditable, and aligned with real storage ownership boundaries. Neovex now has
those boundaries:

- tenant persistence is provider-shaped, not redb-only
- embedded SQLite is the default embedded tenant provider
- embedded redb remains a supported embedded tenant provider
- the cross-tenant control plane is a separate retained redb store
- libsql replica mode materializes local SQLite cache files that Neovex owns
- Postgres and MySQL tenant data live in external databases that Neovex does
  not own at the file level

This plan replaces the old redb-only framing with an architecture-correct
model:

- encryption remains optional and opt-in
- when enabled, Neovex encrypts the local files it owns
- external providers remain responsible for their own at-rest encryption
- one typed config and key-management model spans redb, embedded SQLite, and
  local libsql replica cache files

The goal is to ship something that can credibly support enterprise trust and
compliance reviews without pretending that a single feature makes a whole
deployment "compliant" on its own.

---

## Current Verified State

- `ServicePersistenceConfig` already splits `tenant_provider` from the
  redb-backed control plane.
- service persistence config already lowers through typed CLI, environment, and
  JSON config-file inputs with precedence `CLI > env > file`.
- `data_dir` still defaults to `./data`.
- `control_data_dir` defaults to `data_dir` but is already independently
  configurable, so the host control-plane file root may diverge from tenant
  data roots.
- CLI startup defaults the tenant provider to embedded SQLite.
- embedded providers still route tenant files through `DirectoryPerTenant`
  under `data_dir`, producing `<tenant>.sqlite3` for SQLite and
  `<tenant>.redb` for retained redb.
- retained embedded redb remains selectable for tenant data.
- `EmbeddedRedbControlPlaneProvider` still opens the cross-tenant
  `neovex-control.db` usage store under `control_data_dir`.
- `LibsqlReplicaProvider` materializes local SQLite replica cache files under
  `replica_cache_dir` and opens them through `SqliteTenantStore`.
- libsql replica config already has separate host-side routing state:
  `metadata_namespace` defaults to `neovex_provider`,
  `tenant_namespace_prefix` defaults to `tenant_`, and both are overridable.
- provider-specific host config overrides already fail closed unless the
  matching `tenant_provider` is selected.
- Postgres and MySQL providers keep tenant data outside Neovex-owned local
  files.
- `crates/neovex-storage/src/libsql.rs` is already near the repo's 1,500-line
  signal threshold, so `EAR5` cannot land as additional inline mixed ownership
  in that root.
- `crates/neovex-engine/src/service/mod.rs`,
  `crates/neovex-engine/src/persistence_config.rs`, and
  `crates/neovex-bin/src/main.rs` are currently cohesive and should remain thin
  composition surfaces rather than absorb encryption-specific orchestration
  inline.
- there is no typed encryption config in the CLI, server, or storage layer.
- there is no migration, rotation, or diagnostics surface for encryption.

---

## Coverage Model

| Persistence family | Local file ownership | v1 encryption posture |
| --- | --- | --- |
| Embedded SQLite tenant provider | Neovex-owned `.sqlite3` tenant files | Neovex encrypts |
| Embedded redb tenant provider | Neovex-owned `.redb` tenant files | Neovex encrypts |
| Control plane | Neovex-owned `neovex-control.db` redb file | Neovex encrypts |
| libsql replica provider | Neovex-owned local replica cache files | Neovex encrypts local cache files; remote primary remains provider-managed |
| Postgres provider | No Neovex-owned tenant data files | External provider responsibility |
| MySQL provider | No Neovex-owned tenant data files | External provider responsibility |

This table is the core design rule for the entire plan: Neovex encrypts the
local files it owns and does not pretend to encrypt files or volumes owned by
external database systems.

---

## Scope

This plan covers:

- opt-in encryption at rest for embedded SQLite tenant databases
- opt-in encryption at rest for retained embedded redb tenant databases
- opt-in encryption at rest for the retained redb control-plane database
- opt-in encryption at rest for local libsql replica cache files
- encrypted-by-default Neovex-managed on-disk working artifacts that already
  exist in local-provider flows, including rebuild staging files, migration
  working copies, and retired local replica caches pending cleanup
- the same encrypted-by-default rule for any future built-in persisted
  snapshot/bootstrap/recovery file export surface; current HTTP or in-memory
  bootstrap/snapshot responses remain transport payloads, not at-rest artifacts
- one typed runtime config surface for local encryption
- one cross-provider key-management model with wrapped per-subject keys
- migration, recovery, and rotation flows tailored to each provider family
- a sane local default key source plus at least one managed KMS provider in v1
- diagnostics, documentation, and verification for enterprise review

This plan does not cover:

- transport encryption or TLS
- field-level or application-level selective encryption
- Neovex-managed at-rest encryption of external Postgres or MySQL data files
- encryption of remote libsql/Turso primary storage
- in-memory snapshot/bootstrap structs or HTTP responses that are not persisted
  to disk by a Neovex-owned workflow
- blanket claims of HIPAA, SOC2, ISO, or FIPS compliance

For external providers, Neovex must document operator responsibilities and
report status clearly, but it should not build a fake second encryption layer
inside remote schemas or databases that it does not physically own.

Plaintext exports for recovery or interoperability are not a default feature of
an encrypted deployment. If such flows are supported at all, they must require
an explicit operator override and stay visible as plaintext exceptions.

---

## Success Criteria

This plan is successful only when all of the following are true:

1. Encryption remains disabled by default. No encryption config means the
   current plaintext behavior remains unchanged.
2. Embedded SQLite, embedded redb, and the retained redb control plane all
   support encrypted-at-rest operation when enabled.
3. Local libsql replica cache files support encrypted-at-rest operation when
   encryption is enabled for that provider family.
4. The default embedded tenant provider, SQLite, has a first-class encryption
   story. Encryption is not gated on switching to retained redb.
5. Postgres and MySQL are explicitly treated as external-provider-managed for
   tenant data encryption, while Neovex still encrypts any local control-plane
   files it owns.
6. One typed config surface governs local encryption across CLI, env, config
   file, and programmatic API use.
7. Every protected local database and every covered Neovex-owned persisted
   artifact gets its own random data-encryption key (DEK).
8. DEKs are wrapped by a provider-managed wrapping mechanism so KEK rotation
   does not require rewriting database pages.
9. The default local enablement path is operationally simple and reasonable
   for self-hosted users.
10. A managed KMS provider is supported in v1 for enterprise deployments.
11. Provider-specific migration and recovery paths exist:
    plaintext to encrypted, encrypted to plaintext where meaningful, and
    encrypted key rotation.
12. Current Neovex-managed on-disk migration/rebuild/cutover artifacts emitted
    from encryption-enabled local stores are encrypted by default, and any
    future persisted snapshot/bootstrap/recovery file export must follow the
    same rule or be surfaced as a plaintext exception.
13. Successful migration and rotation retire predecessor plaintext artifacts
    from active Neovex-managed paths and surface `retirement_pending` until
    residue is cleared.
14. Status and diagnostics report what is encrypted, what is externally
    provider-managed, which plaintext exceptions exist, and what remains to be
    migrated or retired, without leaking key material.
15. Performance and verification coverage exist for the encrypted paths that
    Neovex owns.

---

## Execution Contract

### General rules

- Keep the architecture aligned with
  `docs/architecture/storage/encryption.md`; update both together when a
  design decision changes.
- Keep the architecture framing correct: storage is provider-shaped now.
- Treat the concrete Neovex-managed on-disk migration, rebuild, cutover, and
  retired-cache artifacts that exist today as in-scope persistence once local
  encryption is enabled. Any future built-in persisted snapshot/bootstrap/
  recovery export must follow the same rule.
- Do not regress the unencrypted path.
- Do not invent custom cryptographic primitives.
- Keep key-management vocabulary provider-agnostic. Do not design everything
  around "give me a raw KEK" if the provider is actually AWS KMS.
- Prefer one cross-provider key model and provider-specific storage mechanics.
- Do not silently leave predecessor plaintext artifacts in active Neovex-
  managed paths after cutover.
- Do not emit plaintext exports from an encryption-enabled local store unless
  the operator has explicitly requested that exception.
- Do not treat in-memory structs or HTTP/JSON responses as at-rest artifacts
  unless a Neovex-owned workflow persists them to disk.
- Fail fast when encryption is requested for a provider path that is not fully
  wired or not compiled in.
- Keep existing composition roots thin. In particular,
  `crates/neovex-bin/src/main.rs`,
  `crates/neovex-engine/src/service/mod.rs`,
  `crates/neovex-storage/src/lib.rs`,
  `crates/neovex-storage/src/sqlite.rs`, and
  `crates/neovex-storage/src/libsql.rs` should remain entry or provider roots,
  not turn into encryption helper piles.
- Prefer concept-owned module trees over feature-shaped utility piles:
  storage key management belongs under a dedicated storage encryption tree,
  provider-specific encryption behavior belongs beside the owning provider,
  engine startup and diagnostics belong under service-owned modules, and CLI
  admin workflows belong under a dedicated CLI surface instead of `main.rs`.
- `EAR5` must not add another large mixed-owner layer inline to
  `crates/neovex-storage/src/libsql.rs`; extract a `crates/neovex-storage/src/libsql/`
  subtree or equivalent concept-owned module layout before landing cache
  encryption lifecycle work there.
- `EAR6` and `EAR7` must preserve the current thin engine and CLI roots by
  introducing dedicated encryption startup, diagnostics, HTTP, and admin
  command modules rather than centralizing behavior in `service/mod.rs` or
  `main.rs`.
- Update this plan's ledger and execution log in the same change set as
  meaningful plan changes.

### Status model

- `todo`: not started
- `in_progress`: actively being implemented
- `blocked`: waiting on a recorded blocker
- `done`: acceptance criteria met and verification recorded
- `deferred`: intentionally parked

### Minimum verification per item

- targeted tests for the touched subsystem
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p neovex-storage`
- `cargo test -p neovex-engine -p neovex-server`
- packaging- or linkage-affecting items must add release-target or packaging
  proof before they can be marked `done`; `EAR4` specifically must prove the
  SQLCipher-capable build across the supported release matrix and Linux
  packaging consumers

Add provider-specific or benchmark verification where the item requires it.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| EAR1 | done | Add typed local-encryption config and provider-agnostic key-management interfaces | none |
| EAR2 | done | Add authenticated sidecar key manifests plus local `master-key-file` and `key-dir` providers | EAR1 |
| EAR3 | done | Implement retained redb encrypted backend and wire it into tenant/control redb stores | EAR1, EAR2 |
| EAR4 | done | Establish the shared local-SQLite encrypted-open seam and implement encrypted embedded SQLite via SQLCipher, including release-matrix and packaging proof | EAR1, EAR2 |
| EAR5 | done | Encrypt local libsql replica cache files using SQLCipher encryption via the shared local-SQLite seam | EAR1, EAR2, EAR4 |
| EAR6 | done | Wire provider-aware coverage into service startup, status, and diagnostics | EAR3, EAR4, EAR5 |
| EAR7 | done | Add provider-specific migration, recovery, and rotation flows | EAR3, EAR4, EAR5 |
| EAR8 | done | Add AWS KMS provider for enterprise-managed keys | EAR1, EAR2, EAR6 |
| EAR9 | done | Publish operator docs, performance data, and verification evidence | EAR6, EAR7, EAR8 |

---

## Dependency Graph

- `EAR1` defines the typed config and key-management seam. Everything else
  builds on it.
- `EAR2` defines local manifest handling and the first local key sources.
- `EAR3` and `EAR4` are parallel-safe provider implementation lanes once
  `EAR1` and `EAR2` exist.
- `EAR4` owns the shared local-SQLite encrypted-open seam because the current
  libsql replica cache still lowers through `SqliteTenantStore`.
- `EAR5` depends on `EAR4` so libsql cache encryption reuses that shared
  local-SQLite contract instead of forking a second cache-open path.
- `EAR6` depends on the provider implementations because it owns startup
  wiring, coverage reporting, and safe failure behavior.
- `EAR7` depends on each provider implementation because migration and
  rotation are provider-specific.
- `EAR8` depends on `EAR2` because AWS KMS must reuse the authenticated sidecar
  manifest and wrapped-DEK envelope rather than inventing a KMS-only format.
- `EAR8` depends on the config seam and diagnostics but can proceed in
  parallel with later migration polish once the interface is stable.
- `EAR9` closes the loop with docs, benchmarks, and verification evidence.

---

## Recommended Delivery Order

1. `EAR1`
2. `EAR2`
3. `EAR3` and `EAR4`
4. `EAR5`
5. `EAR6`
6. `EAR7`
7. `EAR8`
8. `EAR9`

---

## Design Decisions

### Architecture-first framing

The old plan treated "encryption at rest" as a redb project. That is no longer
correct for Neovex. The live architecture uses:

- embedded SQLite as the default embedded tenant provider
- retained embedded redb as another embedded tenant provider
- a separate retained redb control-plane provider
- libsql replica mode with local SQLite cache files
- external Postgres and MySQL providers for tenant data

The encryption plan must therefore answer four different questions:

- how Neovex encrypts retained redb files it owns
- how Neovex encrypts embedded SQLite files it owns
- how Neovex encrypts local libsql replica cache files it owns
- how Neovex reports external-provider responsibility for tenant data it does
  not own

### Sane operator default

Neovex needs two defaults, not one:

- the zero-configuration default remains `disabled`
- the default enablement path, once an operator opts in, is
  `master-key-file`

`master-key-file` is the sane self-hosted baseline because it:

- requires one operator-managed 32-byte root key outside the data directory
- avoids per-tenant key sprawl for small deployments
- still derives or wraps per-subject keys so each protected local object has
  an independent DEK
- works for embedded SQLite, retained redb, the redb control plane, and local
  libsql cache files

`key-dir` remains a supported advanced mode for explicit per-subject or per-
role key files. AWS KMS is required in v1 as the first enterprise-managed
provider.

### One key-management model across local providers

The old plan centered everything on a `tenant_kek()` style interface. That is
too low-level for managed KMS providers and too redb-specific.

The durable abstraction should be provider-agnostic and operate on protected
local subjects, not just databases:

```rust
pub enum LocalKeySubjectKind {
    Database(LocalDatabaseRole),
    Artifact(LocalArtifactRole),
}

pub struct LocalKeySubject {
    pub provider_family: LocalPersistenceFamily,
    pub kind: LocalKeySubjectKind,
    pub tenant_id: Option<TenantId>,
    pub logical_name: String,
}

pub trait LocalKeyProvider: Send + Sync + 'static {
    fn generate_database_key(&self, subject: &LocalKeySubject) -> Result<GeneratedDatabaseKey>;
    fn unwrap_database_key(
        &self,
        subject: &LocalKeySubject,
        wrapped: &WrappedDatabaseKey,
    ) -> Result<[u8; 32]>;
    fn rewrap_database_key(
        &self,
        subject: &LocalKeySubject,
        wrapped: &WrappedDatabaseKey,
        next: &dyn LocalKeyProvider,
    ) -> Result<WrappedDatabaseKey>;
    fn descriptor(&self) -> KeyProviderDescriptor;
}
```

Key rules:

- every local database and every persisted encrypted artifact gets a random
  256-bit DEK
- the DEK is wrapped by the selected key provider
- provider-specific storage engines receive only the plaintext DEK they need
  at open time
- a KEK rotation rewraps metadata only
- a DEK rotation is provider-specific

### Algorithm and standards matrix

The plan needs to distinguish key-management configuration from data-page
cipher choice. `master-key-file`, `key-dir`, and `aws-kms` decide how Neovex
protects per-subject DEKs; each storage family still uses its own file-level
encryption mechanism.

| Concern | Applies to | v1 choice | Standard or implementation | Why |
| --- | --- | --- | --- | --- |
| Local DEK wrapping for `master-key-file` and `key-dir` | All Neovex-owned local databases and encrypted artifacts | **AES-256-GCM-SIV** for the wrapped-DEK envelope, with manifest metadata as AAD | [RFC 8452](https://www.rfc-editor.org/rfc/rfc8452.html) | One consistent local envelope format, misuse-resistant AEAD, and better failure characteristics than plain AES-GCM if nonce handling is ever imperfect |
| Managed DEK wrapping for `aws-kms` | All Neovex-owned local databases and encrypted artifacts when AWS-managed keys are selected | **AWS KMS envelope encryption** using `GenerateDataKey`, `Decrypt`, and `ReEncrypt`, with stable subject metadata in `EncryptionContext` | [AWS KMS APIs](https://docs.aws.amazon.com/kms/latest/APIReference/API_Decrypt.html), [GenerateDataKey](https://docs.aws.amazon.com/kms/latest/APIReference/API_GenerateDataKey.html), and [encryption context](https://docs.aws.amazon.com/kms/latest/developerguide/encrypt_context.html) | Enterprise auditability, IAM/policy control, and CloudTrail visibility while preserving the shared manifest-backed envelope contract |
| redb page encryption | Embedded redb tenant databases and the retained redb control plane | **AES-256-GCM-SIV** per page | [RFC 8452](https://www.rfc-editor.org/rfc/rfc8452.html) | redb requires a custom page layer, so nonce-misuse resilience is worth more than maximizing raw throughput |
| Embedded SQLite page encryption | Default embedded SQLite tenant provider | **SQLCipher profile**: AES-256-CBC per page plus HMAC-SHA512 integrity, using raw 256-bit DEKs | [SQLCipher design](https://www.zetetic.net/sqlcipher/design/) | Mature SQLite-native encryption, WAL/journal coverage, supported rekey/export flows, and avoids inventing our own pager crypto |
| libsql local cache encryption | Local libsql replica cache files only | **SQLCipher via the shared local-SQLite seam** | Shared `SqliteTenantStore` SQLCipher path | The cache materializes as a local SQLite file and is reopened through the same seam as embedded SQLite, which keeps key management and reopen behavior aligned |
| Snapshot and bootstrap artifact encryption | Neovex-owned on-disk exports, bootstraps, rebuilds, and recovery artifacts from encrypted local stores | **Encrypted by default** with per-artifact DEKs and the same envelope family | Prevents encrypted deployments from recreating plaintext secondary artifacts through backup and recovery flows |
| External tenant stores | Postgres, MySQL, and remote libsql/Turso primary storage | **Provider-managed at-rest encryption** | Operator/provider responsibility | Neovex does not own those underlying data files, so a Neovex-side file cipher would be fake coverage |

Important trust rule: the libsql local-cache profile protects only the local
derivative cache. It does not say anything about the remote libsql primary, and
product docs plus diagnostics must keep that boundary explicit.

### Why keep the DEK envelope consistent

The wrapped-DEK envelope is not the place where Neovex should express
provider-specific personality. It is the control-plane contract for:

- key provenance
- auditability
- KEK rotation
- database identity binding
- status reporting
- recovery tooling

Those concerns are cross-provider concerns, so the design goal is one
consistent envelope shape unless a provider gives Neovex a materially better
reason to do otherwise.

The architecture decision is therefore:

- keep **page encryption provider-specific**
- keep **DEK ownership and wrapping semantics uniform**

That split gives Neovex a simpler and more trustworthy story:

- redb can use a custom page AEAD tuned for redb
- SQLite can use SQLCipher's mature pager encryption
- libsql can reuse the same shared local-SQLite seam for derivative cache files
- operators and tooling still see one key-management model across all of them

### Packaging guardrails for implementation

This workstream is allowed to add new code, but it is not allowed to regress
the ownership cleanup the repo has already landed.

Packaging guardrails:

- `crates/neovex-storage/src/encryption/` should become the concept-owned home
  for cross-provider local encryption contracts such as subject models,
  manifests, key-provider traits, local key providers, and diagnostics-safe
  descriptors.
- retained redb encryption should live beside the redb ownership boundary,
  not inside a generic helper pile.
- the shared local-SQLite encrypted-open seam should live beside
  `SqliteTenantStore` ownership, with `sqlite.rs` staying a thin provider root.
- libsql cache encryption should live beside libsql ownership in a dedicated
  `libsql/` module tree so cache open, materialization, rebuild, and
  encryption lifecycle code stay local and `libsql.rs` remains a thin root.
- engine startup validation and encryption diagnostics should live under
  service-owned modules so `Service::new_with_persistence_config` remains a
  readable composition root.
- HTTP status exposure should land in focused server-side modules instead of
  scattering encryption-specific branches across unrelated handlers.
- CLI inspection, migration, rotation, and retirement flows should live under
  a dedicated command surface so `main.rs` stays an entrypoint, not an
  encryption control tower.

### Why the local envelope uses AES-256-GCM-SIV

For the local `master-key-file` and `key-dir` providers, the choice is not
"what is the strongest cipher in the abstract?" but "what is the safest and
clearest envelope for a Neovex-owned design that binds key metadata and will
be reused across multiple storage families?"

AES-256-GCM-SIV was chosen because it balances the properties we care about:

- misuse-resistant AEAD semantics are valuable in a Neovex-owned envelope
  format
- it supports associated data cleanly, which lets us bind manifest metadata
- it keeps the envelope small and operationally simple
- it stays in the AES family, which is easier to explain to enterprise buyers
  than less familiar alternatives

This is a design-safety choice, not a claim that AES-256-GCM-SIV is globally
"stronger" than every other sound option. It is the best fit for the
cross-provider local envelope role.

### Stronger or more specialized envelope alternatives still worth considering

There are credible alternatives. They were not rejected as "bad crypto"; they
were rejected because they are worse fits for the v1 architecture goal.

| Envelope option | Why it is attractive | Why it is not the v1 default |
| --- | --- | --- |
| **AWS KMS only** | Strongest governance and audit story for enterprises; no local KEK material on disk | Too opinionated as a universal default; worse self-hosted usability; does not help non-AWS deployments |
| **AES-KW / AES-KWP** | NIST-approved key-wrapping modes explicitly designed for protecting keys | Great for pure key wrap, but not as natural when Neovex also wants authenticated metadata binding; would likely push us toward extra envelope machinery around AAD |
| **AES-SIV** | Strong misuse-resistance story and natural AAD binding | Heavier and less implementation-friendly for this role than GCM-SIV; no compelling advantage here over RFC 8452 |
| **XChaCha20-Poly1305** | Excellent random-nonce story and strong practical safety properties | Weaker enterprise standardization story than AES-family options; harder sell as the common envelope across all local providers |
| **Provider-specific local wrap formats** | Could optimize each family independently | Fragments status, tooling, recovery, and rotation; higher audit and maintenance cost without a clear security win |

If Neovex later needs a stricter enterprise profile, the most credible upgrade
paths are:

- `aws-kms` as the recommended enterprise key-provider baseline
- an explicit NIST-leaning profile that swaps the local envelope to
  **AES-KWP** or another reviewed key-wrap mode
- a future hardened profile per provider where the operational value clearly
  outweighs the extra complexity

### Why we still allow provider-specific data encryption

The storage-family ciphers are a different decision from the envelope because
the threat model and integration seam differ by provider.

- For **redb**, Neovex owns the page layer, so misuse resistance is worth
  prioritizing.
- For **SQLite**, correctness and maturity of the pager integration matter
  more than chasing the newest primitive.
- For **libsql**, Neovex should not fight the provider surface for a
  derivative local cache file; the honest choice is to use the supported
  provider mode and disclose its limitations.

In other words, the architecture is intentionally mixed:

- **uniform key-management semantics**
- **provider-appropriate data encryption**

That is the design we can explain clearly in an enterprise review without
pretending every provider has the same crypto surface.

### Authenticated sidecar key manifests

The old header design created avoidable complexity and, in the redb case,
incorrect offset math. Neovex should use one adjacent authenticated sidecar
manifest per local database or persisted encrypted artifact instead:

`<protected-path>.neovex-enc`

Each manifest stores:

- format version
- storage family and subject role
- cipher/profile identifier
- wrapped DEK blob
- key-provider descriptor and safe key identifier
- provider-specific parameters needed to reopen the database
- creation timestamp
- last KEK rotation timestamp

The manifest must be authenticated:

- local providers bind manifest metadata as AEAD associated data when wrapping
  the DEK
- AWS KMS binds the same metadata through `EncryptionContext`
- `aws-kms` changes who wraps the DEK, not the manifest shape; it must reuse
  the same authenticated sidecar contract from `EAR2`

This gives Neovex one consistent key-metadata story across redb, SQLCipher,
and libsql replica cache files.

### Why manifests instead of file headers

Manifest sidecars are chosen because they are the least surprising cross-
provider model:

- SQLite and libsql already own their database file formats
- redb does not need a custom cleartext header in front of its page store
- one rotation and diagnostics flow can span all local provider families
- provider-specific on-disk page mechanics stay local to each engine

### Retained redb strategy

retained redb remains the correct place for a custom encrypted page backend
because upstream explicitly exposes `StorageBackend` for alternative storage
implementations.

For redb, Neovex will:

- implement a new `EncryptedBackend` behind `StorageBackend`
- keep key metadata in the sidecar manifest, not in the redb file
- encrypt each logical redb page slot independently
- authenticate page location as associated data

Per-page layout:

```text
[ nonce ][ ciphertext ][ tag ]
```

Offset math must stay slot-based:

```text
physical_page_size = logical_page_size + nonce_size + tag_size
page_index = logical_offset / logical_page_size
page_slot_start = page_index * physical_page_size
ciphertext_start = page_slot_start + nonce_size
```

Logical byte zero is never allowed to alias the stored nonce bytes. The
implementation should operate on whole logical pages or whole logical
multi-page spans and translate slot by slot.

AAD for redb pages must include:

- page index
- logical page size
- format version

This prevents page-swap attacks across logical positions.

### redb cipher choice

For the custom redb backend, three practical choices were considered:

| Cipher | Strengths | Weaknesses |
| --- | --- | --- |
| AES-256-GCM | Fast and familiar | Random-nonce misuse is catastrophic and forces a much tighter rekey budget for a custom page layer |
| AES-256-GCM-SIV | Misuse resistant, same basic AES hardware story, same 28-byte overhead as GCM | Slower than plain GCM and not a FIPS claim |
| XChaCha20-Poly1305 | Extremely large nonce space, simple random-nonce story | No AES hardware path and weaker enterprise standardization story |

Decision for redb v1: **AES-256-GCM-SIV**.

Rationale:

- Neovex is writing a custom page-encryption layer here, so nonce-misuse
  resilience is worth more than squeezing out the last few percent of
  throughput.
- It avoids the old plan's fragile "AES-GCM is fine forever with random
  nonces" argument.
- It preserves small page overhead and a simple per-page random-nonce design.

Neovex should not market this as a FIPS feature. If a future deployment needs
a strict NIST-only or validated-crypto profile, that should land as an
explicit profile with its own verification story rather than weakening the v1
default.

### Embedded SQLite strategy

Neovex should not build its own SQLite pager encryption. The correct v1 choice
for embedded `rusqlite`-backed SQLite is **SQLCipher**.

Reasons:

- it is purpose-built to encrypt SQLite pages transparently
- it supports raw key material, rekey, and export workflows
- it encrypts the main database plus WAL and rollback journal pages
- `rusqlite` and `libsqlite3-sys` already expose SQLCipher-capable build
  features

Rejected alternatives:

| Approach | Rejected because |
| --- | --- |
| SQLite SEE | proprietary license and distribution friction for public Neovex binaries |
| Homegrown VFS/codec layer | high maintenance, fragile against SQLite internals, reinvents a specialized database encryption project |
| Filesystem-only encryption | useful defense in depth, but not sufficient as Neovex's product-level enterprise story |

For embedded SQLite, Neovex will:

- switch the bundled SQLite build to a SQLCipher-capable configuration once
  verified
- generate a random 32-byte DEK per database and pass raw key material to
  SQLCipher
- keep the wrapped DEK in the sidecar manifest
- use SQLCipher's provider-owned migration and rekey flows instead of trying
  to bolt page encryption on top

Important hardening rules:

- use raw DEKs, not passphrase-derived product contracts
- set the key before the first database operation
- harden temporary storage so encrypted SQLite does not spill plaintext temp
  files to disk
- verify WAL and journal behavior under the encrypted build

Migration and rotation consequences:

- plaintext SQLite to encrypted SQLite uses `sqlcipher_export()`
- encrypted SQLite to plaintext recovery also uses `sqlcipher_export()`
- encrypted SQLite DEK rotation uses `PRAGMA rekey` with a newly generated raw
  key

### Shared local-SQLite seam before libsql cache encryption

The current libsql replica cache path materializes a local SQLite file and then
reopens it through `SqliteTenantStore`. That means embedded SQLite encryption
and libsql local-cache encryption are not independent implementation lanes
today.

Before libsql cache encryption can land, Neovex needs one explicit local-SQLite
open/materialization seam that:

- separates shared local SQLite lifecycle ownership from provider-specific
  page-encryption mechanics
- allows embedded SQLite tenants to use a SQLCipher-capable `rusqlite` path
- allows libsql replica caches to reuse that same SQLCipher-capable
  materialization and reopen path instead of forking a second local-SQLite
  lifecycle
- keeps diagnostics and per-subject key ownership uniform across both families

This seam belongs to `EAR4`; `EAR5` builds on it instead of forking
incompatible local-cache open logic.

### libsql replica local cache strategy

For local libsql replica caches, Neovex will:

- reuse the shared local-SQLite seam from `EAR4`
- resolve a per-cache DEK from the same manifest model used elsewhere
- materialize snapshot rebuilds straight into SQLCipher-protected local cache
  files
- reopen cache files through `SqliteTenantStore` with the same encrypted-open
  path as embedded SQLite
- keep the wrapped DEK in the same sidecar manifest format

This is still in scope even though libsql is an external provider family,
because the replica cache files are Neovex-owned local artifacts.

Because the cache is derivative, its lifecycle is easier than the embedded
authoritative stores:

- enabling encryption can rebuild the cache from the remote primary
- DEK rotation can rebuild the cache under a new DEK
- corruption recovery can discard and re-materialize the local cache

### External Postgres and MySQL posture

Neovex does not own Postgres or MySQL data files. Therefore:

- Neovex does not implement its own at-rest encryption layer for tenant data
  inside those providers
- operator and provider controls remain responsible for remote at-rest
  encryption
- local Neovex-owned control-plane files remain in scope
- diagnostics must say `external-provider-managed` for tenant data encryption,
  not `disabled`

The same rule applies to the remote primary in libsql/Turso. Neovex owns the
local cache file, not the remote storage engine.

### Diagnostics and trust posture

The feature should inspire trust by being explicit and honest:

- report exactly which local files are encrypted
- report which tenant provider families are externally managed
- report the configured key-provider family and safe key identifiers
- report pending migrations or coverage gaps
- never emit raw key material or wrapped-key blobs in status endpoints

The docs and product copy must avoid claiming that this one feature makes an
entire deployment compliant. The correct position is:

- this is an enterprise-grade control
- it materially improves local at-rest protection
- operator environment, access controls, backups, KMS policy, audit logging,
  and deployment choices still determine actual compliance outcomes

---

## Work Items

### EAR1. Add typed local-encryption config and provider-agnostic key interfaces

**Priority:** highest
**Expected impact:** defines the durable product contract before provider
implementations land.

#### Implementation plan

1. Add `LocalEncryptionConfig` to `ServicePersistenceConfig` instead of hiding
   encryption knobs inside provider-specific path flags.
2. Model `LocalEncryptionConfig` as `Disabled` by default plus an enabled
   variant with a `LocalKeyProviderConfig`.
3. Add typed CLI, env, and config-file inputs for:
   - `master-key-file`
   - `key-dir`
   - `aws-kms`
4. Make `master-key-file` the default enablement path in docs and examples,
   while still requiring an explicit key source to turn encryption on.
5. Add provider-aware validation:
   - embedded SQLite, embedded redb, and libsql replica may enable local
     tenant-file encryption
   - Postgres and MySQL may enable local control-plane encryption only
   - startup fails if encryption is requested for a path that is not supported
     or not compiled in
6. Add public-safe descriptors for diagnostics and programmatic API use.

#### Files likely to change

- `crates/neovex-engine/src/persistence_config.rs`
- `crates/neovex-bin/src/main.rs`
- `crates/neovex/src/lib.rs`
- `docs/operating/cli.md`

#### Acceptance criteria

- one typed config surface exists for local encryption
- zero config still means plaintext
- enabling encryption without a valid key source fails before startup
- provider coverage validation is explicit and correct
- the config model works for embedded SQLite, retained redb, the redb control
  plane, and libsql replica cache files

---

### EAR2. Add authenticated sidecar manifests and local key providers

**Priority:** highest
**Expected impact:** gives every local database and persisted encrypted
artifact its own wrapped DEK and rotation metadata without coupling to one
storage engine.

#### Implementation plan

1. Add an adjacent sidecar manifest format for all local encrypted databases
   and persisted encrypted artifacts.
2. Bind manifest metadata through authenticated wrapping:
   - local providers use AEAD AAD
   - AWS KMS uses `EncryptionContext`
3. Implement `MasterKeyFileProvider`:
   - root key read from one file outside the data directory
   - derive per-subject wrapping keys with HKDF using provider family, subject
     kind, and tenant id where applicable
4. Implement `KeyDirectoryProvider`:
   - per-subject or per-role key files for advanced deployments
5. Add atomic sidecar writes and rewrap helpers.
6. Add safe manifest inspection helpers for diagnostics.
7. Extend the same envelope model to the concrete Neovex-managed on-disk
   artifacts in scope today: migration working copies, rebuild staging files,
   and retired local replica cache generations pending cleanup. Any future
   built-in persisted snapshot/bootstrap/recovery file export must reuse the
   same envelope instead of inventing a second format.

#### Files likely to change

- `crates/neovex-storage/src/` (new key-management module tree)
- `crates/neovex-storage/src/encryption/` (new concept-owned module tree for
  subjects, manifests, key-provider traits, local providers, and safe
  descriptors)
- `crates/neovex-storage/src/lib.rs`
- `crates/neovex-engine/src/persistence_config.rs`

#### Acceptance criteria

- every encrypted local database can be opened using a wrapped DEK from its
  sidecar manifest
- current in-scope on-disk artifacts use the same wrapped-DEK model, and any
  future built-in persisted snapshot/bootstrap/recovery file export is required
  to do the same
- manifest tampering is detected
- `master-key-file` and `key-dir` both support create, open, and rewrap flows
- sidecar writes are atomic and crash-safe

---

### EAR3. Implement retained redb encrypted backend

**Priority:** highest after EAR1 and EAR2
**Expected impact:** restores redb coverage for retained embedded tenant data
and the retained control plane.

#### Implementation plan

1. Add `EncryptedBackend` for redb behind `StorageBackend`.
2. Use per-page authenticated encryption with:
   - AES-256-GCM-SIV
   - fresh random nonce per page write
   - AAD containing page index, logical page size, and format version
3. Keep key metadata in the sidecar manifest; do not prepend a custom cleartext
   header to the redb file.
4. Correctly translate logical page slots to physical encrypted page slots.
5. Wire encrypted open/create flows into:
   - retained redb tenant stores
   - `UsageStore` for the control plane
6. Add in-memory and on-disk tests for tamper detection, wrong-key behavior,
   page swap protection, and reopen behavior.

#### Files likely to change

- `crates/neovex-storage/src/encrypted_redb.rs` (new)
- `crates/neovex-storage/src/store/`
- `crates/neovex-storage/src/usage_store.rs`
- `crates/neovex-storage/src/async_storage/engine.rs`
- `crates/neovex-storage/src/async_storage/control.rs`

#### Acceptance criteria

- retained redb tenant databases can run encrypted
- `neovex-control.db` can run encrypted
- tampered pages fail authentication
- wrong keys fail cleanly
- the physical offset math is slot-correct and never aliases nonce bytes

---

### EAR4. Implement encrypted embedded SQLite via SQLCipher

**Priority:** highest after EAR1 and EAR2
**Expected impact:** gives the default embedded tenant provider a first-class
encryption path and establishes the shared local-SQLite contract that the
libsql replica cache path can build on.

#### Implementation plan

1. Extract a shared local-SQLite open/materialization seam from the current
   `SqliteTenantStore` path so embedded SQLite and libsql replica caches do not
   fork incompatible open logic.
2. Keep `crates/neovex-storage/src/sqlite.rs` as a thin provider root by
   placing SQLCipher-specific and encrypted-open logic in concept-owned sqlite
   modules instead of growing the root inline.
3. Switch the embedded SQLite build to a SQLCipher-capable configuration once
   verified for Neovex's release targets.
4. Add encrypted open/create helpers for `SqliteTenantStore` on top of that
   seam.
5. Pass raw per-database DEKs to SQLCipher from the sidecar manifest flow.
6. Harden temporary storage:
   - build and runtime configuration must avoid plaintext temp-file spills
   - verify WAL and rollback journal behavior under the encrypted build
7. Add provider-owned migration helpers:
   - plaintext to encrypted via `sqlcipher_export()`
   - encrypted to plaintext recovery via `sqlcipher_export()`
   - encrypted rekey via `PRAGMA rekey`
8. Prove the SQLCipher-capable build on every supported Neovex release target
   and on the Linux packaging workflows that consume the Linux tarballs:
   - `x86_64-unknown-linux-gnu`
   - `aarch64-unknown-linux-gnu`
   - `aarch64-apple-darwin`
   - `x86_64-pc-windows-msvc`
   - `release.yml` must run
     `bash scripts/verify-sqlcipher-proof.sh cargo-lanes` on each supported
     release build job
   - `linux-packages.yml` must run
     `bash scripts/verify-sqlcipher-proof.sh packaged-binary ...` against the
     downloaded Linux tarball before packaging so the consumer path proves the
     shipped binary can encrypt and export SQLite data
   - hosted release jobs must upload per-target proof bundles named
     `sqlcipher-proof-<target>` so the first green release-matrix run leaves
     behind durable evidence instead of only transient logs
   - hosted Linux packaging jobs must upload package-consumer proof bundles
     named `sqlcipher-package-proof-<arch>` so the tarball-consumer lane can be
     linked directly from closeout notes
   - closeout should collect those hosted artifacts with
     `bash scripts/collect-sqlcipher-proof-bundles.sh --run-id <run-id> ...`
     (or `make collect-sqlcipher-proof-bundles RUN_ID=<run-id>`) and record the
     resulting local proof-bundle path or downloaded artifact names in this
     execution log
9. Verify that the plaintext path still works when encryption is disabled.

#### Files likely to change

- `Cargo.toml`
- `crates/neovex-storage/Cargo.toml`
- `crates/neovex-storage/src/sqlite.rs`
- `crates/neovex-storage/src/sqlite/` (expanded concept-owned module tree for
  encrypted open, SQLCipher configuration, and migration helpers)
- `crates/neovex-storage/src/async_storage/sqlite.rs`
- `crates/neovex-storage/src/libsql.rs`
- `crates/neovex-storage/src/tests.rs`

#### Acceptance criteria

- a shared local-SQLite open/materialization seam exists for embedded SQLite
  and libsql replica-cache consumers
- embedded SQLite tenants can be created and reopened encrypted
- SQLCipher migration flows work for encrypt, decrypt, and rekey
- WAL and rollback journal handling are verified under the encrypted build
- temp-file hardening is enforced for encrypted SQLite mode
- the SQLCipher-capable build is proven on Neovex's supported release targets
  and Linux packaging consumers before `EAR4` closes
- the unencrypted SQLite path remains supported when encryption is disabled
- `sqlite.rs` remains a thin provider root with SQLCipher-specific ownership in
  dedicated sibling modules

---

### EAR5. Encrypt local libsql replica cache files

**Priority:** highest after EAR1 and EAR2
**Expected impact:** closes the last Neovex-owned SQLite file gap for the
libsql replica provider family.

#### Implementation plan

1. Build on the shared local-SQLite seam from `EAR4`; do not fork
   `SqliteTenantStore`'s open/materialization contract inside libsql.
2. Extract a dedicated `crates/neovex-storage/src/libsql/` module tree or an
   equivalent concept-owned layout before landing more cache-open,
   materialization, rebuild, or encryption lifecycle logic so `libsql.rs`
   stays a thin provider root.
3. Reuse the shared SQLCipher-capable local-SQLite seam from `EAR4` instead of
   introducing a second libsql-only encrypted-open contract.
4. Add encrypted open/materialization flows for local replica cache files
   through `SqliteTenantStore`.
5. Feed raw DEKs from the same sidecar manifest model used elsewhere.
6. Keep the remote primary encryption story provider-managed and clearly
   documented as such.
7. Use cache rebuild flows for:
   - first-time encryption enablement
   - DEK rotation
   - corruption recovery

#### Files likely to change

- `Cargo.toml`
- `crates/neovex-storage/Cargo.toml`
- `crates/neovex-storage/src/libsql.rs`
- `crates/neovex-storage/src/libsql/` (new concept-owned cache encryption,
  materialization, rebuild, and open-lifecycle module tree)
- `crates/neovex-engine/src/service/mod.rs`

#### Acceptance criteria

- libsql local cache files can be materialized and reopened encrypted
- local cache re-materialization works under encryption
- libsql cache encryption reuses the shared local-SQLite seam from `EAR4`
  instead of introducing a second cache-open contract
- diagnostics distinguish local cache encryption from remote primary
  provider-managed encryption
- cache rebuild flows remain correct after restart and refresh
- `libsql.rs` remains a thin provider root instead of absorbing cache
  encryption orchestration inline

---

### EAR6. Wire provider-aware coverage into startup and diagnostics

**Priority:** high
**Expected impact:** turns implementation slices into a coherent operator
experience.

#### Implementation plan

1. Wire local encryption config through service startup for:
   - embedded SQLite tenant provider
   - retained redb tenant provider
   - retained redb control plane
   - libsql replica local cache path
2. Keep `crates/neovex-engine/src/service/mod.rs` as a composition root by
   extracting encryption startup validation, coverage resolution, and
   diagnostics shaping into dedicated service-owned modules.
3. Add a status endpoint that reports:
   - whether local encryption is enabled
   - key-provider family and safe descriptor
   - per-local-role coverage status
   - plaintext exceptions and retirement-pending residue state
   - provider-managed external coverage for Postgres, MySQL, and remote libsql
   - pending migration state
4. Add fail-fast startup checks for unsupported or partially wired paths.
5. Expose programmatic diagnostics through the facade crate.
6. Keep the HTTP surface focused by landing encryption status wiring in a
   dedicated server-owned module rather than scattering logic across unrelated
   handlers.

#### Files likely to change

- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-engine/src/service/encryption/` (new startup validation and
  diagnostics module tree, or equivalent concept-owned service surface)
- `crates/neovex-server/src/lib.rs`
- `crates/neovex-server/src/http/`
- `crates/neovex-server/src/http/encryption.rs` (new or equivalent focused
  status surface)
- `crates/neovex/src/lib.rs`

#### Acceptance criteria

- startup wiring matches the selected provider family
- status output is accurate and does not leak secrets
- plaintext exceptions and retirement-pending residue are visible in status
- external-provider-managed paths are clearly labeled
- unsupported coverage requests fail before serving traffic
- `service/mod.rs` remains a readable composition root rather than the owner of
  encryption-specific validation and reporting details

---

### EAR7. Add provider-specific migration, recovery, and rotation flows

**Priority:** high
**Expected impact:** makes the feature operable rather than merely buildable.

#### Implementation plan

1. Add an explicit admin CLI namespace for encryption operations. The exact
   command names should follow Neovex's CLI taxonomy, but the required
   capabilities are:
   - inspect coverage
   - migrate plaintext to encrypted
   - export encrypted to plaintext where meaningful
   - rotate wrapping keys
   - rotate database keys
   - retire predecessor plaintext artifacts
2. Keep `crates/neovex-bin/src/main.rs` as an entrypoint by landing the admin
   surface under a dedicated CLI module tree instead of wiring encryption
   workflows inline in the root.
3. KEK rotation rules:
   - rewrap sidecar manifest only
   - no database page rewrite
4. DEK rotation rules by provider:
   - retained redb: copy and re-encrypt
   - embedded SQLite: `PRAGMA rekey`
   - libsql replica cache: discard and rebuild under a new DEK
   - redb control plane: same as retained redb
5. Migration rules by provider:
   - retained redb: copy plaintext redb to encrypted redb and vice versa
   - embedded SQLite: use `sqlcipher_export()`
   - libsql replica cache: rebuild cache rather than export
6. Artifact rules:
   - current Neovex-managed on-disk migration/rebuild/cutover artifacts
     produced from encryption-enabled local stores are encrypted by default
   - any future built-in persisted snapshot/bootstrap/recovery file export must
     reuse the same envelope and default-encrypted posture
   - plaintext artifact output requires an explicit operator override and is
     tracked as a plaintext exception
7. Cutover and retirement rules:
   - validation completes before the encrypted target becomes authoritative
   - predecessor plaintext databases, WAL/SHM sidecars, rollback journals,
     temp spill files, migration working copies, and retired replica caches are
     removed from active Neovex-managed paths after cutover
   - diagnostics report `retirement_pending` until predecessor plaintext
     artifacts are cleared
   - active-path conflicts fail closed
8. Document live-tenant rules:
   - KEK rotation may run online if reopen semantics are safe
   - redb DEK rotation requires quiesce
   - embedded SQLite rekey requires explicit operational guidance
   - libsql cache rotation may rebuild from remote with documented refresh
     behavior
9. Keep provider-specific migration ownership beside the provider
   implementations: retained redb flows beside redb ownership, embedded SQLite
   flows beside sqlite ownership, and libsql rebuild flows beside libsql cache
   ownership.

#### Files likely to change

- `crates/neovex-bin/src/main.rs`
- `crates/neovex-bin/src/encryption/` (new CLI module tree for inspect,
  migrate, rotate, export, and retirement commands)
- `crates/neovex-storage/src/`
- `docs/operating/cli.md`

#### Acceptance criteria

- operators can migrate into encryption and back out for recovery where
  applicable
- KEK rotation never rewrites data pages
- DEK rotation is correct for each provider family
- predecessor plaintext artifacts are retired from active paths after cutover
- plaintext exceptions and retirement-pending residue are surfaced clearly
- live-operation requirements are documented and verified
- `main.rs` remains a thin entrypoint and provider-specific migration logic
  lives with the owning provider surfaces

---

### EAR8. Add AWS KMS provider for enterprise-managed keys

**Priority:** high
**Expected impact:** satisfies the v1 enterprise requirement for a managed KMS
path instead of stopping at local files.

#### Implementation plan

1. Reuse the `EAR2` sidecar manifest and wrapped-DEK envelope; AWS KMS changes
   the wrapping provider, not the protected-subject metadata contract.
2. Add `AwsKmsKeyProvider` as the first managed provider.
3. Use:
   - `GenerateDataKey` on create
   - `Decrypt` on open
   - `ReEncrypt` for wrapping-key rotation where applicable
4. Bind `LocalKeySubject` metadata into AWS `EncryptionContext`.
5. Support safe key descriptors and failure diagnostics without exposing
   ciphertext blobs.
6. Add test coverage against LocalStack or an equivalent AWS KMS test setup.
7. Keep `master-key-file` as the default local path; AWS KMS is the required
   enterprise-managed option, not the default for all deployments.

#### Files likely to change

- `crates/neovex-storage/src/` (new AWS KMS provider module)
- `crates/neovex-engine/src/persistence_config.rs`
- `crates/neovex-bin/src/main.rs`
- `docs/operating/cli.md`

#### Acceptance criteria

- AWS KMS create, open, and rewrap flows work
- AWS KMS reuses the shared manifest/envelope contract from `EAR2`; no
  KMS-only sidecar format is introduced
- `EncryptionContext` is stable and validated
- failures distinguish auth, missing key, policy, and network errors
- managed-key diagnostics are safe and useful

---

### EAR9. Publish docs, performance data, and verification evidence

**Priority:** high
**Expected impact:** turns the feature into something an enterprise reviewer
can actually evaluate.

#### Implementation plan

1. Add `docs/operating/encryption.md` covering:
   - provider coverage matrix
   - key-provider options
   - migration and rotation rules
   - current on-disk working-artifact coverage plus the rule for any future
     persisted snapshot/bootstrap/recovery file export
   - plaintext exception and retirement-pending semantics
   - external-provider responsibilities
   - operational caveats and recovery flows
2. Add benchmark coverage for:
   - retained redb plaintext vs encrypted
   - embedded SQLite plaintext vs encrypted
   - libsql replica encrypted local-cache reopen and refresh drills
   - keep the benchmark entrypoints reproducible from repo-owned commands
     rather than manual cargo invocations
3. Record representative performance numbers with hardware context. The
   capture flow should emit:
   - a host-context log (`uname`, CPU, memory, Rust toolchain, and any bench
     round overrides)
   - embedded plaintext and embedded encrypted markdown reports
   - an encrypted libsql replica markdown report when the local libsql
     benchmark environment is available
   - per-command logs suitable for attaching to a closeout bundle
4. Update the CLI reference with the final operator surface.
5. Document release-build implications for SQLCipher and libsql encryption.

#### Files likely to change

- `docs/operating/encryption.md` (new)
- `docs/operating/cli.md`
- `crates/neovex-engine/benches/`
- `scripts/collect-encryption-benchmark-evidence.sh`

#### Acceptance criteria

- docs accurately describe coverage and limits
- benchmarks exist and are reproducible
- a single repo-owned capture flow records hardware context plus benchmark logs
- encrypted and plaintext performance comparisons are published in repo-owned
  research docs
- operator guidance is sufficient for security and compliance review

---

## Execution Log

| Date | Item | Outcome | Notes |
| --- | --- | --- | --- |
| 2026-04-02 | baseline | created | Created the original redb-first plan. |
| 2026-04-16 | plan | rewritten | Reframed the plan around the live provider architecture: embedded SQLite default, retained redb, separate redb control plane, and libsql local cache coverage. Removed the invalid redb header design, replaced it with authenticated sidecar key manifests, moved embedded SQLite to a SQLCipher-based strategy, added explicit external-provider responsibility rules for Postgres/MySQL, and made AWS KMS a required v1 enterprise-managed key provider. |
| 2026-04-16 | docs | architecture added | Added `docs/architecture/storage/encryption.md` as the stable architecture baseline, including provider-boundary diagrams, key-management rationale, and lifecycle flows; retargeted the plan to it and updated the docs index. |
| 2026-04-16 | review | findings resolved | Expanded scope to cover persisted snapshot/bootstrap/recovery artifacts, broadened the key-subject model beyond databases, and added explicit cutover plus retirement semantics so predecessor plaintext artifacts cannot silently linger after migration or rotation. |
| 2026-04-18 | review | plan hardened | Tightened roadmap dependencies so `EAR4` owns the shared local-SQLite seam and `EAR5` depends on it, made `EAR8` depend on the `EAR2` manifest contract, added release-matrix plus packaging proof to `EAR4` exit criteria, and narrowed persisted-artifact scope to concrete on-disk working files plus future built-in export rules rather than in-memory or HTTP responses. |
| 2026-04-18 | review | config state refreshed | Re-read the live service-persistence assembly path after recent host-side config and file-layout changes. Recorded the current precedence (`CLI > env > file`), `data_dir` and `control_data_dir` defaults, concrete tenant/control/cache file roots, libsql namespace defaults, and fail-closed provider override behavior in `Current Verified State`. |
| 2026-04-20 | review | packaging guardrails tightened | Tightened the active control plane so implementation preserves concept-owned ownership boundaries: recorded the near-threshold `libsql` root and current thin service or CLI roots in `Current Verified State`, added execution-contract guardrails against regrowing those roots, and amended `EAR2`, `EAR4`, `EAR5`, `EAR6`, and `EAR7` to predeclare dedicated storage-encryption, libsql, service-diagnostics, server-HTTP, and CLI module trees before feature code lands. |
| 2026-04-20 | EAR1 | done | Added typed local-encryption config surface. `LocalEncryptionConfig` with `Disabled` default and `Enabled(LocalKeyProviderConfig)` variants. `LocalKeyProviderConfig` supports `MasterKeyFile`, `KeyDirectory`, and `AwsKms`. Added CLI (`--encryption-key-provider`, `--encryption-master-key-file`, `--encryption-key-dir`, `--encryption-aws-kms-key-id`, `--encryption-aws-region`, `--encryption-aws-endpoint-url`), env (`NEOVEX_ENCRYPTION_*`), and config-file inputs with `CLI > env > file` precedence. Added provider-aware validation, diagnostics-safe descriptors (`KeyProviderDescriptor`, `EncryptionConfigDescriptor`), and `LocalPersistenceFamily` for coverage reporting. Added 8 focused tests for config parsing and validation. Verification: `cargo fmt --all --check`, `cargo clippy -p neovex-engine -p neovex-bin -p neovex --all-targets -- -D warnings`, `cargo test -p neovex-storage`, `cargo test -p neovex-engine -p neovex-server`, all passed. Next: EAR2. |
| 2026-04-20 | EAR2 | done | Added authenticated sidecar key manifests and local key providers. Created `crates/neovex-storage/src/encryption/` module tree with: `LocalKeySubject` (subject model with derivation context and descriptors), `LocalKeySubjectKind` (`Database`/`Artifact`), `LocalDatabaseRole`, `LocalArtifactRole`, `WrappedDatabaseKey` (cipher + ciphertext), `GeneratedDatabaseKey` (plaintext + wrapped, zeroes on drop), `WrappingCipher::Aes256GcmSiv`, `LocalKeyProvider` trait, `LocalKeyProviderError`, `KeyProviderKind`, `MasterKeyFileProvider` (HKDF-SHA256 derivation + AES-256-GCM-SIV wrapping), `KeyDirectoryProvider` (per-subject key files), `KeyManifest`, `KeyManifestHeader` (with `to_aad()` for authenticated binding), `ManifestCipher`, manifest binary serialization with atomic write-to-temp-then-rename. Exported from `lib.rs`. Module-level integration tests verify manifest round-trip, provider integration, unique DEK generation, per-subject key derivation, cross-subject unwrap rejection, control plane subjects, artifact subjects, libsql cache subjects, and manifest tampering detection. Verification: `cargo fmt --all --check`, `cargo clippy -p neovex-storage --all-targets -- -D warnings`, `cargo test -p neovex-storage` (152 passed), `cargo test -p neovex-engine -p neovex-server` (177 passed). Next: EAR3 or EAR4. |
| 2026-04-20 | EAR3 | done | Implemented retained redb encrypted backend. Created `crates/neovex-storage/src/encrypted_redb.rs` with: `EncryptedFileBackend` (file-backed encrypted storage), `EncryptedMemoryBackend` (in-memory encrypted storage for testing), per-page AES-256-GCM-SIV encryption with 12-byte random nonces and 16-byte tags, AAD binding (format version + page index + logical page size) to prevent page-swap attacks, logical-to-physical offset translation (4096-byte logical pages → 4124-byte physical pages), transparent read/write/set_len operations. Added `open_encrypted` and `create_in_memory_encrypted` methods to `TenantStore` and `UsageStore`. Tests verify: basic operations, cross-page reads/writes, wrong-key rejection, redb integration (create, write, reopen, verify), page-swap attack detection. Exported `EncryptedFileBackend`, `EncryptedMemoryBackend`, `ENCRYPTED_FORMAT_VERSION`, `LOGICAL_PAGE_SIZE`, `PHYSICAL_PAGE_SIZE` from `lib.rs`. Verification: `cargo fmt --all --check`, `cargo clippy -p neovex-storage --all-targets -- -D warnings`, `cargo test -p neovex-storage` (159 passed), `cargo test -p neovex-engine -p neovex-server` (177 passed). Next: EAR4. |
| 2026-04-20 | EAR4 | in_progress | Implemented the shared local-SQLite seam and SQLCipher runtime helpers. `rusqlite` now builds with `bundled-sqlcipher`, `crates/neovex-storage/src/sqlite/encryption.rs` owns encrypted open/export/rekey helpers, and the embedded SQLite open path applies raw per-database DEKs from the manifest flow. Added `scripts/verify-sqlcipher-proof.sh` plus release-workflow and Linux-packaging hooks so the supported target matrix and tarball-consumer path exercise the SQLCipher helper lane, tenant-store lane, and packaged CLI migration/export flow. The helper now emits durable proof bundles, the hosted workflows upload them as `sqlcipher-proof-<target>` and `sqlcipher-package-proof-<arch>` artifacts, and `scripts/collect-sqlcipher-proof-bundles.sh` / `make collect-sqlcipher-proof-bundles RUN_ID=...` can download those artifacts into a closeout bundle. The remaining step is still the first green hosted multi-target run plus Linux packaging consumers, then linking those artifact names or collected bundle paths here before `EAR4` can close. |
| 2026-04-20 | EAR5 | done | Implemented encrypted local libsql replica cache files through the same SQLCipher-backed local-SQLite seam as embedded SQLite. `LibsqlReplicaProviderConfig` now carries `encryption_provider: Option<Arc<dyn LocalKeyProvider>>`, cache materialization and reopen resolve per-cache DEKs from sidecar manifests, and refresh/rebuild/delete flows keep cache files plus manifests aligned. Updated service wiring, tests, and benchmarks to use the provider-based contract instead of a shared `encryption_dek`. |
| 2026-04-20 | EAR6 | done | Reworked service startup onto manifest-backed per-path key resolution. `InitializedKeyProvider` now initializes `LocalKeyProvider` instances, all embedded SQLite, embedded redb, control-plane redb, and libsql cache startup paths receive provider handles instead of family-wide derived DEKs, and runtime opens now resolve manifests at the protected path they are about to serve. The status endpoint remains configuration-oriented, but the startup plumbing now matches the `EAR2` manifest contract. |
| 2026-04-20 | EAR7 | done | Finished manifest-aware admin workflows. `migrate` generates random per-target DEKs plus sidecar manifests, `export` unwraps the DEK recorded for the source path, `rotate-kek` uses the canonical manifest path and atomically rewrites manifests, SQLite DEK rotation checkpoints WAL state and backs up the full SQLite artifact set before `PRAGMA rekey`, redb DEK rotation preserves the runtime AAD contract, and libsql cache DEK rotation rewrites the manifest then retires local cache files so restart rebuilds them under the new DEK. |
| 2026-04-20 | EAR8 | done | Landed the manifest-backed AWS KMS provider in `crates/neovex-storage/src/encryption/aws_kms.rs` and moved service wiring onto the shared `LocalKeyProvider` contract. AWS KMS now uses `GenerateDataKey` on create, `Decrypt` on open, and provider-native `ReEncrypt` during KEK rotation, with stable manifest metadata bound into `EncryptionContext`. `rotate-kek` now accepts explicit replacement-provider inputs for `master-key-file`, `key-dir`, and `aws-kms`, rejects conflicting flag mixes, and can rotate a manifest onto KMS without rewriting pages. Added KMS stub-server coverage for create/open/rewrap plus focused CLI rotation inference tests. Verification: `cargo check --workspace`, `cargo test -p neovex-storage --features aws-kms aws_kms -- --nocapture`, `cargo test -p neovex-bin infer_new_provider -- --nocapture`. |
| 2026-04-20 | EAR9 | in_progress | Refreshed the operator docs, CLI reference, and architecture notes so they describe the real manifest-backed runtime, current libsql SQLCipher strategy, concrete artifact scope, and live AWS KMS support. The checked-in benchmark harnesses now accept repo-owned encryption modes (`make bench-embedded-providers ENCRYPTION=temp-master-key-file`, `make bench-libsql-replica-provider ENCRYPTION=temp-master-key-file`), and `scripts/collect-encryption-benchmark-evidence.sh` / `make collect-encryption-benchmark-evidence OUTPUT_DIR=...` capture host context, per-command logs, embedded plaintext vs encrypted reports, and the encrypted libsql replica report when local libsql endpoints are configured. Published the first local embedded plaintext-vs-encrypted summary in `docs/plans/research/encryption-at-rest-benchmark-report.md` and checked in the matching raw embedded reports at `docs/plans/research/encryption-at-rest-embedded-plaintext-benchmark-report.md` and `docs/plans/research/encryption-at-rest-embedded-encrypted-benchmark-report.md`, sourced from a real bundle captured on macOS arm64 with the manifest-backed startup path. The remaining `EAR9` blockers are narrower now: collect the encrypted libsql local-cache benchmark evidence on a configured local primary/admin pair, and link the first hosted SQLCipher proof artifacts from `sqlcipher-proof-<target>` and `sqlcipher-package-proof-<arch>`. |
| 2026-04-21 | EAR9 | in_progress | Extended the checked-in performance evidence with a deeper retained-redb cold-open attribution pass. `docs/plans/research/encryption-at-rest-journal-cold-open-profile.md` and the summary report now record that the remaining encrypted redb reopen cost is concentrated in two redb-owned v2 phases: region-header reads inside `Allocators::from_bytes(...)` and the `sync_data` durability barrier inside `begin_writable()`. The same follow-up also records the negative results for the remaining obvious repo-owned levers (`set_cache_size(...)` and `create_with_file_format_v3(true)`), so the next cold-open optimization should be framed as upstream redb work or a read-first-open architecture change rather than more crypto-policy tuning. |
| 2026-04-21 | EAR9 | in_progress | Added a benchmark-only deferred-writable experiment to the checked-in performance narrative and tightened the follow-up claim with a stronger rerun. On the reduced-round encrypted cold `journal-stream` drill, a temporary local `redb` fork that deferred `begin_writable()` until the first real write cut the cold journal median from `12.97 ms` to `7.96 ms` and cut `redb::Database` open from `10.23-10.84 ms` to `4.76-5.65 ms`, while open-time decrypt work stayed almost flat. The first one-sample cold `indexed-query` rerun looked worse, but a follow-up three-sample rerun did not reproduce that regression: cold indexed-query median improved from `3.00 ms/op` to `2.60 ms/op`, and first-query totals fell from `42.05-46.71 ms` to `31.77-32.14 ms`. The roadmap now records the more precise conclusion: an explicit read-first or read-only redb open mode is the most promising remaining cold-open lever, but a naive "skip eager writable mode" patch is still not merge-ready because it shifts writable/durability work rather than removing it. |
| 2026-04-21 | EAR9 | in_progress | Added a profiled standard-sample SQLite cold indexed-query follow-up in `docs/plans/research/encryption-at-rest-indexed-query-refresh.md`. The repo evidence now records that manifest unwrap is usually only `0.21-0.22 ms`, SQLCipher `apply_key` is about `0.04 ms`, `verify_key` is the largest crypto-specific step at about `0.27-0.62 ms`, and pooled open plus schema load stays around `0.85-1.36 ms`. The dominant remaining cost on the tuned SQLite cold path is the first indexed query execution after reopen (`10.99-13.32 ms`), so the next SQLite cold-open optimization should focus on first-query or page-cache warmup rather than more encryption-policy tuning. |
| 2026-04-21 | EAR9 | in_progress | Tested a benchmark-only SQLite query warmup experiment in the cold indexed-query lane via `NEOVEX_SQLITE_INDEX_QUERY_WARMUP=<limit1|full>`. The result was useful but negative for product adoption: both modes collapsed the next measured query to warm-state (`~0.94-1.03 ms`), but the warmup itself cost about `10.54-12.89 ms`, which is roughly the same as the cold query it removed. End-to-end SQLite cold medians therefore regressed slightly (`1.57 ms` baseline, `1.66 ms` with `limit1`, `1.65 ms` with `full`). The roadmap now records the narrower recommendation: do not add eager app-level query warmups as a cold-open optimization; if we pursue SQLite warmup further, it should be a cheaper lower-level page-cache or file-read strategy. |
| 2026-04-21 | EAR9 | in_progress | Tested the next lower-level SQLite warmup hypothesis with `NEOVEX_SQLITE_INDEX_QUERY_WARMUP=raw-id-only`, which opens the cloned SQLite file through a separate raw `rusqlite` connection, applies the manifest-backed SQLCipher key, and runs `SELECT id ... LIMIT 1` before the measured batch. This was a stronger negative result than the app-level warmup: on the same reduced-round drill, SQLite cold median regressed from `1.63 ms/op` on the fresh baseline rerun to `4.79 ms/op`, the raw probe itself still cost about `5.55-10.21 ms`, and two measured SQLite cold samples saw the first service query spike to about `69.18-78.68 ms` total. The roadmap conclusion is now sharper: do not pursue separate pre-touch probe connections as a cold-open optimization. If SQLite gets another reopen pass, it should be a fundamentally different in-process open-path strategy; otherwise the next effort should shift back to redb read-first/open work and the remaining libsql plus release-proof evidence. |
| 2026-04-21 | EAR9 | in_progress | Rechecked the live repo seam and the exact pinned `redb 2.6.3` source so the next optimization claim rests on an explicit API boundary, not inference. The checked-in notes now record that Neovex's repo-owned redb open path is already down to `TenantStore::open_* -> redb::Database::builder().create(...)`, while upstream `Database::new()` still eagerly calls `mem.begin_writable()` and then opens an immediate internal write transaction to restore persistent savepoint tracker state. That means the measured read-first/deferred-writable win is real, but it is not available through a supported repo-only builder or page-cache knob. The cold-open investigation can therefore close the remaining "cheaper lower-level warmup" branch: the next real redb step is either an upstream/local dependency patch that introduces a supported read-first open contract, or a deliberate pivot to the remaining libsql benchmark and hosted SQLCipher proof evidence. |
| 2026-04-21 | EAR9 | in_progress | Recorded the roadmap decision to defer the redb read-first/open direction for now. The checked-in research keeps the deferred-writable measurement as evidence of a credible future lever, but active scope will not pursue a local `redb` patch or upstream open-mode work in this wave. With the cold-open investigation now closed, the remaining EAR9 execution focus is the libsql local-cache benchmark evidence plus the hosted SQLCipher release-matrix proof artifacts. |
| 2026-04-21 | EAR9 | done | Published the remaining repo-owned evidence slice for replica-connected SQLite local-cache encryption. The libsql replica benchmark now supports repeated workload filters, the repo-owned Make target plus `scripts/collect-encryption-benchmark-evidence.sh` narrow the libsql capture to the plan-owned reopen and freshness drills (`point-read`, `indexed-query`, `composite-indexed-query`, `barrier-refresh`, and `peer-catch-up`), and the checked-in report `docs/plans/research/encryption-at-rest-libsql-replica-encrypted-cache-benchmark-report.md` records representative encrypted local-cache results. Updated the benchmark summary and operator docs to link that report and clarified that hosted SQLCipher artifact linkage remains separate EAR4 release-proof work rather than an EAR9 blocker. |
| 2026-05-12 | EAR4 | done | Closed the SQLCipher release and package proof gate. Collected the green hosted `v0.1.22` release-matrix proof from GitHub Actions run `24905210213` into `docs/plans/research/sqlcipher-release-proof-24905210213/`; all four uploaded artifacts report `status=passed`: `sqlcipher-proof-aarch64-unknown-linux-gnu`, `sqlcipher-proof-x86_64-unknown-linux-gnu`, `sqlcipher-proof-aarch64-apple-darwin`, and `sqlcipher-proof-x86_64-pc-windows-msvc`. Fixed the Linux package workflow so arm64 package proof runs on `ubuntu-24.04-arm` and repinned the Linux package contract to the existing `agentstation/neovex-crun` release `v1.27-neovex.1`, then collected green package-consumer proof from GitHub Actions run `25718813353` into `docs/plans/research/sqlcipher-package-proof-25718813353/`; both `sqlcipher-package-proof-amd64` and `sqlcipher-package-proof-arm64` report `status=passed`. Also captured supplemental local Docker ARM64 package proof at `docs/plans/research/sqlcipher-docker-arm64-package-proof-v0.1.22/`. |
| 2026-04-20 | review follow-up | gap closure | Resolved the post-review control-plane gaps that blocked implementation trust: startup and CLI now use per-path manifests instead of family-wide HKDF-derived DEKs, libsql cache encryption is documented and implemented as part of the shared SQLCipher seam, `rotate-kek` uses the canonical manifest filename, SQLite DEK rotation checkpoints WAL state and backs up the full artifact set, libsql cache DEK rotation keeps the new manifest while retiring only cache database files, and the roadmap/docs now distinguish completed KMS work from the still-open release-proof and benchmark-evidence slices. |
