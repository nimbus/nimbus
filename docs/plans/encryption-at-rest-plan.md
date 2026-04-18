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
- [libsql `EncryptionConfig`](https://docs.rs/libsql/latest/libsql/struct.EncryptionConfig.html)
- [libsql `Cipher`](https://docs.rs/libsql/latest/libsql/enum.Cipher.html)
- [Turso embedded replicas encryption docs](https://docs.turso.tech/features/embedded-replicas/introduction)
- [Turso native encryption docs](https://docs.turso.tech/tursodb/encryption)

### KMS

- [AWS KMS `GenerateDataKey`](https://docs.aws.amazon.com/kms/latest/APIReference/API_GenerateDataKey.html)
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
- CLI startup defaults the tenant provider to embedded SQLite.
- retained embedded redb remains selectable for tenant data.
- `EmbeddedRedbControlPlaneProvider` still opens the cross-tenant
  `neovex-control.db` usage store.
- `LibsqlReplicaProvider` materializes local SQLite replica cache files under
  `replica_cache_dir` and opens them through `SqliteTenantStore`.
- Postgres and MySQL providers keep tenant data outside Neovex-owned local
  files.
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
- encrypted-by-default Neovex-owned on-disk snapshot, bootstrap, rebuild, and
  recovery artifacts produced from encryption-enabled local stores
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
7. Every local database and every persisted encrypted export artifact gets its
   own random data-encryption key (DEK).
8. DEKs are wrapped by a provider-managed wrapping mechanism so KEK rotation
   does not require rewriting database pages.
9. The default local enablement path is operationally simple and reasonable
   for self-hosted users.
10. A managed KMS provider is supported in v1 for enterprise deployments.
11. Provider-specific migration and recovery paths exist:
    plaintext to encrypted, encrypted to plaintext where meaningful, and
    encrypted key rotation.
12. Neovex-owned on-disk snapshot, bootstrap, rebuild, and recovery artifacts
    emitted from encryption-enabled local stores are encrypted by default, or
    explicitly surfaced as plaintext exceptions.
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
- Treat Neovex-owned on-disk snapshot, bootstrap, rebuild, and recovery
  artifacts as in-scope persistence once local encryption is enabled.
- Do not regress the unencrypted path.
- Do not invent custom cryptographic primitives.
- Keep key-management vocabulary provider-agnostic. Do not design everything
  around "give me a raw KEK" if the provider is actually AWS KMS.
- Prefer one cross-provider key model and provider-specific storage mechanics.
- Do not silently leave predecessor plaintext artifacts in active Neovex-
  managed paths after cutover.
- Do not emit plaintext exports from an encryption-enabled local store unless
  the operator has explicitly requested that exception.
- Fail fast when encryption is requested for a provider path that is not fully
  wired or not compiled in.
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

Add provider-specific or benchmark verification where the item requires it.

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| EAR1 | todo | Add typed local-encryption config and provider-agnostic key-management interfaces | none |
| EAR2 | todo | Add authenticated sidecar key manifests plus local `master-key-file` and `key-dir` providers | EAR1 |
| EAR3 | todo | Implement retained redb encrypted backend and wire it into tenant/control redb stores | EAR1, EAR2 |
| EAR4 | todo | Implement encrypted embedded SQLite via SQLCipher | EAR1, EAR2 |
| EAR5 | todo | Encrypt local libsql replica cache files using libsql's encryption support | EAR1, EAR2 |
| EAR6 | todo | Wire provider-aware coverage into service startup, status, and diagnostics | EAR3, EAR4, EAR5 |
| EAR7 | todo | Add provider-specific migration, recovery, and rotation flows | EAR3, EAR4, EAR5 |
| EAR8 | todo | Add AWS KMS provider for enterprise-managed keys | EAR1, EAR6 |
| EAR9 | todo | Publish operator docs, performance data, and verification evidence | EAR6, EAR7, EAR8 |

---

## Dependency Graph

- `EAR1` defines the typed config and key-management seam. Everything else
  builds on it.
- `EAR2` defines local manifest handling and the first local key sources.
- `EAR3`, `EAR4`, and `EAR5` are parallel-safe provider implementations once
  `EAR1` and `EAR2` exist.
- `EAR6` depends on the provider implementations because it owns startup
  wiring, coverage reporting, and safe failure behavior.
- `EAR7` depends on each provider implementation because migration and
  rotation are provider-specific.
- `EAR8` depends on the config seam and diagnostics but can proceed in
  parallel with later migration polish once the interface is stable.
- `EAR9` closes the loop with docs, benchmarks, and verification evidence.

---

## Recommended Delivery Order

1. `EAR1`
2. `EAR2`
3. `EAR3`, `EAR4`, and `EAR5`
4. `EAR6`
5. `EAR7`
6. `EAR8`
7. `EAR9`

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
| Managed DEK wrapping for `aws-kms` | All Neovex-owned local databases and encrypted artifacts when AWS-managed keys are selected | **AWS KMS envelope encryption** using `GenerateDataKey`, `Decrypt`, and `ReEncrypt`, with stable subject metadata in `EncryptionContext` | [AWS KMS APIs](https://docs.aws.amazon.com/kms/latest/APIReference/API_GenerateDataKey.html) and [encryption context](https://docs.aws.amazon.com/kms/latest/developerguide/encrypt_context.html) | Enterprise auditability, IAM/policy control, CloudTrail visibility, and no Neovex-managed KEK material on disk |
| redb page encryption | Embedded redb tenant databases and the retained redb control plane | **AES-256-GCM-SIV** per page | [RFC 8452](https://www.rfc-editor.org/rfc/rfc8452.html) | redb requires a custom page layer, so nonce-misuse resilience is worth more than maximizing raw throughput |
| Embedded SQLite page encryption | Default embedded SQLite tenant provider | **SQLCipher profile**: AES-256-CBC per page plus HMAC-SHA512 integrity, using raw 256-bit DEKs | [SQLCipher design](https://www.zetetic.net/sqlcipher/design/) | Mature SQLite-native encryption, WAL/journal coverage, supported rekey/export flows, and avoids inventing our own pager crypto |
| libsql local cache encryption | Local libsql replica cache files only | **Provider-native libsql encryption**, currently `Cipher::Aes256Cbc` | [libsql `Cipher`](https://docs.rs/libsql/latest/libsql/enum.Cipher.html) | Use the provider-supported cache encryption mode instead of layering a second pager; accept that this family is constrained by the current libsql surface |
| Snapshot and bootstrap artifact encryption | Neovex-owned on-disk exports, bootstraps, rebuilds, and recovery artifacts from encrypted local stores | **Encrypted by default** with per-artifact DEKs and the same envelope family | Prevents encrypted deployments from recreating plaintext secondary artifacts through backup and recovery flows |
| External tenant stores | Postgres, MySQL, and remote libsql/Turso primary storage | **Provider-managed at-rest encryption** | Operator/provider responsibility | Neovex does not own those underlying data files, so a Neovex-side file cipher would be fake coverage |

Important trust rule: the libsql local-cache profile is not as strong or as
feature-complete as the redb or SQLCipher paths today. Product docs and
diagnostics must report that honestly instead of implying a uniform crypto
profile across all providers.

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
- libsql can use its provider-native cache encryption
- operators and tooling still see one key-management model across all of them

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

### libsql replica local cache strategy

libsql replica mode already has its own encryption support. Neovex should use
that support for local cache files instead of layering SQLCipher on top.

For local libsql replica caches, Neovex will:

- enable the libsql crate's encryption feature
- construct `EncryptionConfig` from the same per-database DEK model used
  elsewhere
- use the provider-supported cipher set for that family
- keep the wrapped DEK in the same sidecar manifest format

This is still in scope even though libsql is an external provider family,
because the replica cache files are Neovex-owned local artifacts.

The current libsql Rust docs expose `Cipher::Aes256Cbc`; Neovex should accept
that provider limitation for this family rather than pretending every provider
must use the same page cipher internally.

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
- `docs/reference/cli.md`

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
7. Extend the same envelope model to persisted snapshot/bootstrap/recovery
   artifacts emitted by encryption-enabled local stores.

#### Files likely to change

- `crates/neovex-storage/src/` (new key-management module tree)
- `crates/neovex-storage/src/lib.rs`
- `crates/neovex-engine/src/persistence_config.rs`

#### Acceptance criteria

- every encrypted local database can be opened using a wrapped DEK from its
  sidecar manifest
- encrypted persisted snapshot/bootstrap/recovery artifacts use the same
  wrapped-DEK model
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
encryption path.

#### Implementation plan

1. Switch the embedded SQLite build to a SQLCipher-capable configuration once
   verified for Neovex's release targets.
2. Add encrypted open/create helpers for `SqliteTenantStore`.
3. Pass raw per-database DEKs to SQLCipher from the sidecar manifest flow.
4. Harden temporary storage:
   - build and runtime configuration must avoid plaintext temp-file spills
   - verify WAL and rollback journal behavior under the encrypted build
5. Add provider-owned migration helpers:
   - plaintext to encrypted via `sqlcipher_export()`
   - encrypted to plaintext recovery via `sqlcipher_export()`
   - encrypted rekey via `PRAGMA rekey`
6. Verify that the plaintext path still works when encryption is disabled.

#### Files likely to change

- `Cargo.toml`
- `crates/neovex-storage/Cargo.toml`
- `crates/neovex-storage/src/sqlite.rs`
- `crates/neovex-storage/src/async_storage/sqlite.rs`
- `crates/neovex-storage/src/tests.rs`

#### Acceptance criteria

- embedded SQLite tenants can be created and reopened encrypted
- SQLCipher migration flows work for encrypt, decrypt, and rekey
- WAL and rollback journal handling are verified under the encrypted build
- temp-file hardening is enforced for encrypted SQLite mode
- the unencrypted SQLite path remains supported when encryption is disabled

---

### EAR5. Encrypt local libsql replica cache files

**Priority:** highest after EAR1 and EAR2
**Expected impact:** closes the last Neovex-owned SQLite file gap for the
libsql replica provider family.

#### Implementation plan

1. Enable libsql's encryption support in the workspace dependency.
2. Add encrypted open/materialization flows for local replica cache files using
   `EncryptionConfig`.
3. Feed raw DEKs from the same sidecar manifest model used elsewhere.
4. Keep the remote primary encryption story provider-managed and clearly
   documented as such.
5. Use cache rebuild flows for:
   - first-time encryption enablement
   - DEK rotation
   - corruption recovery

#### Files likely to change

- `Cargo.toml`
- `crates/neovex-storage/Cargo.toml`
- `crates/neovex-storage/src/libsql.rs`
- `crates/neovex-engine/src/service/mod.rs`

#### Acceptance criteria

- libsql local cache files can be materialized and reopened encrypted
- local cache re-materialization works under encryption
- diagnostics distinguish local cache encryption from remote primary
  provider-managed encryption
- cache rebuild flows remain correct after restart and refresh

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
2. Add a status endpoint that reports:
   - whether local encryption is enabled
   - key-provider family and safe descriptor
   - per-local-role coverage status
   - plaintext exceptions and retirement-pending residue state
   - provider-managed external coverage for Postgres, MySQL, and remote libsql
   - pending migration state
3. Add fail-fast startup checks for unsupported or partially wired paths.
4. Expose programmatic diagnostics through the facade crate.

#### Files likely to change

- `crates/neovex-engine/src/service/mod.rs`
- `crates/neovex-server/src/lib.rs`
- `crates/neovex-server/src/http/`
- `crates/neovex/src/lib.rs`

#### Acceptance criteria

- startup wiring matches the selected provider family
- status output is accurate and does not leak secrets
- plaintext exceptions and retirement-pending residue are visible in status
- external-provider-managed paths are clearly labeled
- unsupported coverage requests fail before serving traffic

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
2. KEK rotation rules:
   - rewrap sidecar manifest only
   - no database page rewrite
3. DEK rotation rules by provider:
   - retained redb: copy and re-encrypt
   - embedded SQLite: `PRAGMA rekey`
   - libsql replica cache: discard and rebuild under a new DEK
   - redb control plane: same as retained redb
4. Migration rules by provider:
   - retained redb: copy plaintext redb to encrypted redb and vice versa
   - embedded SQLite: use `sqlcipher_export()`
   - libsql replica cache: rebuild cache rather than export
5. Artifact rules:
   - persisted Neovex-owned snapshot/bootstrap/recovery artifacts produced from
     encryption-enabled local stores are encrypted by default
   - plaintext artifact output requires an explicit operator override and is
     tracked as a plaintext exception
6. Cutover and retirement rules:
   - validation completes before the encrypted target becomes authoritative
   - predecessor plaintext databases, WAL/SHM sidecars, rollback journals,
     temp spill files, migration working copies, and retired replica caches are
     removed from active Neovex-managed paths after cutover
   - diagnostics report `retirement_pending` until predecessor plaintext
     artifacts are cleared
   - active-path conflicts fail closed
7. Document live-tenant rules:
   - KEK rotation may run online if reopen semantics are safe
   - redb DEK rotation requires quiesce
   - embedded SQLite rekey requires explicit operational guidance
   - libsql cache rotation may rebuild from remote with documented refresh
     behavior

#### Files likely to change

- `crates/neovex-bin/src/main.rs`
- `crates/neovex-storage/src/`
- `docs/reference/cli.md`

#### Acceptance criteria

- operators can migrate into encryption and back out for recovery where
  applicable
- KEK rotation never rewrites data pages
- DEK rotation is correct for each provider family
- predecessor plaintext artifacts are retired from active paths after cutover
- plaintext exceptions and retirement-pending residue are surfaced clearly
- live-operation requirements are documented and verified

---

### EAR8. Add AWS KMS provider for enterprise-managed keys

**Priority:** high
**Expected impact:** satisfies the v1 enterprise requirement for a managed KMS
path instead of stopping at local files.

#### Implementation plan

1. Add `AwsKmsKeyProvider` as the first managed provider.
2. Use:
   - `GenerateDataKey` on create
   - `Decrypt` on open
   - `ReEncrypt` for wrapping-key rotation where applicable
3. Bind `LocalKeySubject` metadata into AWS `EncryptionContext`.
4. Support safe key descriptors and failure diagnostics without exposing
   ciphertext blobs.
5. Add test coverage against LocalStack or an equivalent AWS KMS test setup.
6. Keep `master-key-file` as the default local path; AWS KMS is the required
   enterprise-managed option, not the default for all deployments.

#### Files likely to change

- `crates/neovex-storage/src/` (new AWS KMS provider module)
- `crates/neovex-engine/src/persistence_config.rs`
- `crates/neovex-bin/src/main.rs`
- `docs/reference/cli.md`

#### Acceptance criteria

- AWS KMS create, open, and rewrap flows work
- `EncryptionContext` is stable and validated
- failures distinguish auth, missing key, policy, and network errors
- managed-key diagnostics are safe and useful

---

### EAR9. Publish docs, performance data, and verification evidence

**Priority:** high
**Expected impact:** turns the feature into something an enterprise reviewer
can actually evaluate.

#### Implementation plan

1. Add `docs/reference/encryption.md` covering:
   - provider coverage matrix
   - key-provider options
   - migration and rotation rules
   - snapshot/bootstrap/export artifact rules
   - plaintext exception and retirement-pending semantics
   - external-provider responsibilities
   - operational caveats and recovery flows
2. Add benchmark coverage for:
   - retained redb plaintext vs encrypted
   - embedded SQLite plaintext vs encrypted
   - libsql replica encrypted local-cache reopen and refresh drills
3. Record representative performance numbers with hardware context.
4. Update the CLI reference with the final operator surface.
5. Document release-build implications for SQLCipher and libsql encryption.

#### Files likely to change

- `docs/reference/encryption.md` (new)
- `docs/reference/cli.md`
- `crates/neovex-storage/benches/`

#### Acceptance criteria

- docs accurately describe coverage and limits
- benchmarks exist and are reproducible
- encrypted and plaintext performance comparisons are published
- operator guidance is sufficient for security and compliance review

---

## Execution Log

| Date | Item | Outcome | Notes |
| --- | --- | --- | --- |
| 2026-04-02 | baseline | created | Created the original redb-first plan. |
| 2026-04-16 | plan | rewritten | Reframed the plan around the live provider architecture: embedded SQLite default, retained redb, separate redb control plane, and libsql local cache coverage. Removed the invalid redb header design, replaced it with authenticated sidecar key manifests, moved embedded SQLite to a SQLCipher-based strategy, added explicit external-provider responsibility rules for Postgres/MySQL, and made AWS KMS a required v1 enterprise-managed key provider. |
| 2026-04-16 | docs | architecture added | Added `docs/architecture/storage/encryption.md` as the stable architecture baseline, including provider-boundary diagrams, key-management rationale, and lifecycle flows; retargeted the plan to it and updated the docs index. |
| 2026-04-16 | review | findings resolved | Expanded scope to cover persisted snapshot/bootstrap/recovery artifacts, broadened the key-subject model beyond databases, and added explicit cutover plus retirement semantics so predecessor plaintext artifacts cannot silently linger after migration or rotation. |
