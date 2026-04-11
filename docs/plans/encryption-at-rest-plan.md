# Encryption At Rest Plan

This is the canonical execution plan for adding encryption at rest to Neovex
tenant and control databases.

Reviewed against:

- `ARCHITECTURE.md`
- `crates/neovex-storage/src/store.rs`
- `crates/neovex-storage/src/usage_store.rs`
- `crates/neovex-storage/src/async_storage.rs`
- `crates/neovex-bin/src/main.rs`
- redb `StorageBackend` trait (redb 2.x)

---

## References

### redb maintainer guidance

- [redb issue #759 — Encryption at rest](https://github.com/cberner/redb/issues/759):
  cberner recommends encrypted filesystem or the `StorageBackend` trait.
- [redb issue #1091 — Page level encryption?](https://github.com/cberner/redb/issues/1091):
  cberner confirms `StorageBackend` enables block-level encryption "just as
  efficient as if it was implemented in redb."
- [redb `StorageBackend` trait docs](https://docs.rs/redb/2.6.3/redb/trait.StorageBackend.html):
  five methods (`read`, `write`, `set_len`, `len`, `sync_data`), requires
  `'static + Debug + Send + Sync`.
- [redb design doc](https://github.com/cberner/redb/blob/master/docs/design.md):
  copy-on-write B-trees, buddy allocator with page reuse, dual commit slots.

### Existing implementations (prior art)

- [SQLCipher design](https://www.zetetic.net/sqlcipher/design/): AES-256-CBC
  with random IV per page, regenerated on every write, stored in page.
  HMAC-SHA512 per page for authentication.
- [Turso/libSQL encryption](https://turso.tech/blog/introducing-fast-native-encryption-in-turso-database):
  AEGIS-256 or AES-GCM per page, random nonce regenerated on every write,
  nonce + auth tag stored in page reserved space. Measured ~6% read / ~14%
  write overhead with AEGIS-256.
  [Turso encryption docs](https://docs.turso.tech/tursodb/encryption).
- [redb-turbo](https://github.com/russellromney/redb-turbo): community redb
  fork with AES-256-GCM page-level encryption. 12-byte random nonce per
  write, 28-byte overhead per page (nonce + auth tag). Validates that the
  `StorageBackend` approach works end-to-end.

### Cipher references

- [RFC 8452 — AES-GCM-SIV](https://www.rfc-editor.org/rfc/rfc8452.html):
  nonce-misuse-resistant AEAD. Decryption speed matches AES-GCM; encryption
  ~50% slower on long messages due to two-pass construction.
- [AEGIS IETF draft](https://datatracker.ietf.org/doc/draft-irtf-cfrg-aegis-aead/):
  AEGIS-256 uses AES round function for speed, 256-bit nonce (no collision
  concern), key-erasure-friendly. On IETF standardization track.
- [`aegis` Rust crate](https://github.com/jedisct1/rust-aegis): maintained by
  jedisct1 (libsodium author). Hardware-accelerated via AES-NI.
- [`ring` crate](https://github.com/briansmith/ring): already a workspace
  dependency. Provides AES-256-GCM with BoringSSL-derived assembly.
- [RustCrypto AES-GCM-SIV](https://github.com/RustCrypto/AEADs/tree/master/aes-gcm-siv):
  pure Rust with optional AES-NI. Partially audited by NCC Group.
- [AES vs ChaCha20 benchmarks (2025)](https://ashvardanian.com/posts/chacha-vs-aes-2025/):
  AES-GCM ~6.4 GB/s vs ChaCha20-Poly1305 ~4.2 GB/s on Apple M3 Pro. AES
  wins by up to 3x on AWS instances with AES-NI.

### Security references

- [AES-GCM nonce reuse attack](https://frereit.de/aes_gcm/): nonce reuse
  under same key leaks XOR of plaintexts and recovers the authentication key.
  Catastrophic, not degraded.
- [GCM key recovery attacks (elttam)](https://www.elttam.com/blog/key-recovery-attacks-on-gcm/):
  detailed write-up of nonce-reuse authentication key recovery.

---

## Purpose

Enterprise multi-tenant deployments require encryption at rest. The current
storage layer writes plaintext redb files to disk. redb's `StorageBackend`
trait provides a clean seam for page-level encryption without forking or
modifying redb itself.

Neovex's database-per-tenant model makes per-tenant encryption keys
architecturally natural: each `TenantStore` already opens an independent redb
file, so each can use a different `StorageBackend` instance with a different
key. This is a stronger isolation story than filesystem-level encryption
(single key for all data).

---

## Current Verified State

- `TenantStore::open_with_simulation(path, ...)` uses `Database::create(path)`
  which uses redb's default `FileBackend`
- `TenantStore::create_in_memory_with_simulation(...)` already uses
  `Database::builder().create_with_backend(InMemoryBackend::new())`
- `UsageStore::open(path)` also uses `Database::create(path)`
- the `StorageBackend` seam is already exercised — the in-memory test path
  proves that `create_with_backend` works end-to-end
- no encryption configuration exists in the CLI, server, or storage layer

---

## Scope

This plan covers:

- an encrypted `StorageBackend` implementation for redb
- per-tenant key provisioning and lifecycle with envelope encryption
- control database (`neovex-control.db`) encryption
- CLI and configuration surface for encryption
- key rotation support (fast KEK rotation + full DEK rotation)
- deterministic testing of the encrypted storage path
- performance measurement and documentation

This plan does not cover:

- wire-level encryption (TLS) — orthogonal transport concern
- field-level encryption — different threat model
- key management service (KMS) integration beyond a pluggable key-provider
  trait — concrete provider implementations (AWS KMS, HashiCorp Vault, etc.)
  are follow-on work after the core encryption layer lands

---

## Success Criteria

This plan is successful only when all of the following are true:

1. Tenant databases are encrypted at rest using authenticated encryption with
   per-tenant data encryption keys.
2. The control database is encrypted at rest.
3. Existing unencrypted databases can be migrated to encrypted format.
4. Key encryption key (KEK) rotation is supported without data rewrite.
5. Data encryption key (DEK) rotation is supported via full database rewrite
   for the rare case of DEK compromise.
6. Encryption is opt-in and configurable through typed runtime config loaded
   from idiomatic CLI/env/config inputs, with programmatic API support and no
   reliance on ad hoc feature-specific env vars as the long-term contract.
7. Performance overhead is measured and documented (target: <15% on write
   path, <10% on read path).
8. The encrypted path is covered by the existing test suite running through
   the encrypted backend.
9. An unencrypted deployment remains the zero-configuration default.

---

## Execution Contract

### General rules

- Follow the same execution discipline as the scalability plan: one item
  in-progress at a time, deterministic tests before or alongside
  implementation, update this plan's ledger and log in the same change set.
- Prefer the `StorageBackend` trait approach over any redb fork or
  modification.
- Do not break the unencrypted path. Encryption is additive and opt-in.
- Keep the crypto implementation as thin as possible. Use established crates
  — do not implement cryptographic primitives.
- Do not use deterministic or offset-derived nonces. Every page encryption
  must use a fresh random nonce (see Design Decisions for rationale).
- Runtime-facing encryption config should follow the same rule as broader
  provider work: CLI flags, environment variables, and config files lower into
  typed config, while any test-only or benchmark-only env vars remain harness
  inputs rather than the product contract.

### Status model

- `todo`: not started
- `in_progress`: actively being implemented
- `blocked`: cannot proceed until the recorded blocker is resolved
- `done`: acceptance criteria are met and verification is recorded
- `deferred`: intentionally parked

### Minimum verification per item

- targeted tests for the touched crate or subsystem
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo test -p neovex-storage` for any storage-layer changes
- `cargo test -p neovex-engine -p neovex-server` for integration changes

---

## Roadmap Status Ledger

| Item | Status | Summary | Hard Dependencies |
| --- | --- | --- | --- |
| EAR1 | todo | Implement `EncryptedBackend` behind redb `StorageBackend` trait | none |
| EAR2 | todo | Add key-provider trait with envelope encryption and file-based provider | none |
| EAR3 | todo | Integrate encrypted backend into `TenantStore` and `UsageStore` | EAR1, EAR2 |
| EAR4 | todo | Add typed runtime CLI/env/config surface for encryption and key-provider selection | EAR3 |
| EAR5 | todo | Add migration path for unencrypted-to-encrypted databases | EAR3 |
| EAR6 | todo | Add key rotation support (KEK rotation + DEK rotation) | EAR3 |
| EAR7 | todo | Performance measurement and documentation | EAR3 |
| EAR8 | deferred | Pluggable KMS provider implementations (AWS KMS, Vault, etc.) | EAR2, EAR4 |

---

## Dependency Graph

- `EAR1` and `EAR2` are independent foundations that can proceed in parallel.
- `EAR3` depends on both `EAR1` and `EAR2` — it wires the encrypted backend
  into the storage layer using the key provider.
- `EAR4`, `EAR5`, `EAR6`, and `EAR7` all depend on `EAR3`.
- `EAR4`, `EAR5`, `EAR6`, and `EAR7` are independent of each other.
- `EAR4` should define the operator-facing config contract first so later
  migration, rotation, and performance work can reuse one consistent runtime
  surface.
- `EAR8` is deferred until the core encryption layer is stable and the
  configuration surface from `EAR4` defines how providers are selected.

---

## Recommended Delivery Order

1. `EAR1` and `EAR2` (parallel-safe)
2. `EAR3`
3. `EAR4`
4. `EAR5`
5. `EAR6`
6. `EAR7`
7. `EAR8`

---

## Design Decisions

### Why page-level encryption via `StorageBackend`

redb's maintainer explicitly endorsed this path (issue #1091) and declined to
add encryption inside redb. The `StorageBackend` trait is a clean five-method
interface that Neovex already exercises via `InMemoryBackend` in tests. The
community fork redb-turbo validates that the approach works end-to-end.

Alternatives considered:

| Approach | Rejected because |
| --- | --- |
| Encrypted filesystem (LUKS/dm-crypt) | Single key for all data. No per-tenant isolation. Requires OS-level configuration outside Neovex's control. |
| Encrypt values before storage | Indexes cannot operate on encrypted values. Breaks query planning. Higher per-field overhead. |
| Fork redb | Maintenance burden. Diverges from upstream. `StorageBackend` achieves the same result without forking. |
| Field-level encryption | Different threat model (protects individual fields, not the database file). Does not address the enterprise requirement of full at-rest encryption. |

### Cipher evaluation

Four AEAD ciphers were evaluated for page-level database encryption:

| Cipher | Nonce | Tag | Per-page overhead | HW accel | Throughput (AES-NI) | Nonce safety | Status |
| --- | --- | --- | --- | --- | --- | --- | --- |
| AES-256-GCM | 12 B | 16 B | 28 B (~0.7% of 4 KB) | AES-NI, ARM CE | ~6.4 GB/s | Birthday bound at 2^32 per key. Catastrophic on reuse. | NIST standard |
| AES-256-GCM-SIV | 12 B | 16 B | 28 B (~0.7% of 4 KB) | AES-NI | ~3-4 GB/s encrypt | Nonce-misuse resistant (leaks only plaintext equality). | RFC 8452 |
| XChaCha20-Poly1305 | 24 B | 16 B | 40 B (~1.0% of 4 KB) | None (software only) | ~4.2 GB/s | 2^192 birthday bound. Random nonces effectively collision-free. | IETF RFC 8439 + extended nonce |
| AEGIS-256 | 32 B | 16-32 B | 48-64 B (~1.2-1.6% of 4 KB) | AES-NI (uses AES round function) | >10 GB/s | 2^256 birthday bound. Random nonces collision-free. Key-erasure-friendly. | IETF draft, nearing RFC |

**Decision: AES-256-GCM as initial default, with AEGIS-256 as a planned
upgrade path.**

Rationale:

- `ring` is already a workspace dependency and provides AES-256-GCM with
  production-grade BoringSSL-derived assembly. No new dependency needed.
- AES-256-GCM's 2^32 nonce birthday bound is safe for per-tenant keys: a
  tenant would need >4 billion page writes under one key before collision risk
  becomes non-negligible. Key rotation (EAR6) resets the counter.
- AEGIS-256 is the better long-term choice (Turso chose it for exactly this
  use case, faster than AES-GCM, larger nonce, key-erasure-friendly) but adds
  a new dependency (`aegis` crate) and is not yet a finalized RFC.
- The file header includes a cipher identifier byte so the upgrade from
  AES-256-GCM to AEGIS-256 is a format-compatible change — existing databases
  keep their cipher, new databases can use the new default.
- AES-256-GCM-SIV was considered for nonce-misuse resistance but rejected
  because the plan already mandates random nonces per write (eliminating the
  misuse case) and GCM-SIV's ~50% encryption slowdown is unnecessary overhead.
- XChaCha20-Poly1305 was considered for its large nonce space but rejected
  because it lacks hardware acceleration and is ~40% slower than AES-GCM on
  the x86/ARM64 targets Neovex is deployed on.

### Nonce construction: random per write

**Every page write gets a fresh cryptographically random nonce.** The nonce is
stored alongside the ciphertext in the on-disk page.

This is the same approach used by SQLCipher, Turso/libSQL, and redb-turbo. It
is the only safe approach for redb because:

1. **redb reuses page offsets.** The copy-on-write B-tree frees pages into a
   buddy allocator. Once all referencing read transactions complete, a freed
   page offset is reallocated to a new transaction with different data. The
   dual commit slots also rewrite fixed offsets on every transaction commit.
   (Source: [redb design doc](https://github.com/cberner/redb/blob/master/docs/design.md))

2. **Deterministic nonces derived from offset would cause nonce reuse.**
   If `nonce = f(offset, salt)` and the same offset receives different
   plaintext across transactions (which redb's page reuse guarantees), then
   AES-GCM is used with the same (key, nonce) pair on different plaintexts.
   This is catastrophic: it leaks the XOR of plaintexts and allows full
   recovery of the authentication key.
   (Source: [AES-GCM nonce reuse](https://frereit.de/aes_gcm/),
   [elttam GCM key recovery](https://www.elttam.com/blog/key-recovery-attacks-on-gcm/))

3. **Random nonces with AES-256-GCM are safe within the birthday bound.**
   With 12-byte (96-bit) random nonces, the probability of a collision
   reaches 2^-32 (one in ~4 billion) after 2^48 encryptions under the same
   key. Per-tenant keys mean each tenant has an independent nonce space.
   A tenant performing 1,000 writes/second would reach 2^32 writes (~4
   billion) after ~136 years. Key rotation (EAR6) resets this counter.

### Per-page on-disk layout

Each page on disk is stored as:

```
[ nonce (12 bytes) ][ ciphertext (N bytes) ][ auth tag (16 bytes) ]
```

Total overhead per page: **28 bytes** (AES-256-GCM). For redb's default page
size (OS page size, typically 4096 bytes), this is ~0.7% space overhead.

The `EncryptedBackend` translates between logical offsets (what redb sees) and
physical offsets (what the file contains):

```
physical_page_size = logical_page_size + NONCE_SIZE + TAG_SIZE
physical_offset = file_header_size + (logical_offset / logical_page_size) * physical_page_size + (logical_offset % logical_page_size)
```

For sub-page writes (redb sometimes writes less than a full page), the
backend must handle partial encryption. The simplest correct approach: always
read-decrypt-modify-encrypt the full enclosing page. redb's `write()` calls
are page-aligned in practice (the `StorageBackend` operates on pages), but
the implementation must handle edge cases defensively.

### Associated data (AAD) for page integrity

The page's **logical offset** is passed as associated data (AAD) during both
encryption and decryption. AAD is authenticated but not encrypted — it
prevents an attacker from swapping two encrypted pages at different file
offsets. Without AAD, an attacker who can modify the file could swap pages A
and B; both would decrypt successfully but at the wrong logical position,
silently corrupting the B-tree structure.

SQLCipher uses the page number as AAD for the same reason.

### File header

The encrypted database file starts with a small cleartext header:

```
[ magic (8 bytes): "NVXCRYPT" ]
[ format version (1 byte): 0x01 ]
[ cipher identifier (1 byte): 0x01 = AES-256-GCM, 0x02 = AEGIS-256, ... ]
[ logical page size (4 bytes): redb's page size, little-endian u32 ]
[ encrypted DEK (48 bytes): DEK encrypted by KEK via AES-256-GCM ]
[   - nonce (12 bytes) ]
[   - ciphertext (20 bytes, padded to fixed size) ]
[   - auth tag (16 bytes) ]
[ header HMAC (32 bytes): HMAC-SHA256 of all preceding header bytes ]
```

Total header size: **94 bytes** (fixed, independent of page size).

The header is validated on open:

- magic bytes must match
- format version must be supported
- cipher identifier must be recognized
- HMAC must verify (detects header tampering)
- encrypted DEK must decrypt successfully with the provided KEK

redb sees everything after the header as its normal storage space.

### Envelope encryption (KEK/DEK model)

The plan uses **envelope encryption** rather than using the operator-provided
key directly as the data encryption key. This is the standard model used by
AWS KMS, GCP CMEK, Azure Key Vault, and HashiCorp Vault.

**How it works:**

1. The operator provides or provisions a **key encryption key (KEK)** — this
   is what the `KeyProvider` trait returns.
2. When a new encrypted database is created, a random 256-bit **data
   encryption key (DEK)** is generated.
3. The DEK is encrypted by the KEK and stored in the file header.
4. On open, the KEK decrypts the DEK from the header. The DEK is used for
   all page encryption/decryption. The KEK is not retained after
   initialization.
5. Page encryption uses the DEK, never the KEK.

**Why envelope encryption:**

- **Fast KEK rotation:** rotating the KEK only requires re-encrypting the DEK
  in the header (48 bytes). No data rewrite. This is the common operational
  rotation and completes in microseconds.
- **Rare DEK rotation:** if the DEK itself is compromised (rare — it only
  exists in memory and in the encrypted header), a full database rewrite is
  needed. This is the expensive path but should almost never be required.
- **KMS compatibility:** enterprise KMS systems (AWS, GCP, Vault) are
  designed around envelope encryption. The KEK maps directly to a KMS-managed
  key. `GenerateDataKey` returns a plaintext DEK + an encrypted DEK blob.
- **Key erasure:** the KEK can be erased from memory immediately after
  decrypting the DEK on startup. Only the DEK needs to persist in memory
  during operation.

### Key-per-tenant model

Each `TenantStore` receives its own KEK via the `KeyProvider` trait. Each
tenant's database has its own independent DEK stored in its file header. This
gives per-tenant key isolation at both the KEK and DEK level.

The control database (`neovex-control.db`) uses a separate system KEK
(`KeyProvider::system_key()`) and its own DEK.

### Expected performance overhead

Based on prior art measurements and the design:

| Operation | Expected overhead | Reasoning |
| --- | --- | --- |
| Page write | ~10-15% | Random nonce generation (~50ns) + AES-256-GCM encrypt (~0.15μs/KB with AES-NI for 4KB page) + write nonce+tag (28 extra bytes). Turso measured ~14% write overhead with AEGIS-256; AES-GCM is slightly slower. |
| Page read | ~5-8% | AES-256-GCM decrypt (~0.15μs/KB with AES-NI) + read nonce+tag. Turso measured ~6% read overhead. Decryption is faster than encryption for GCM. |
| fsync | ~0% | fsync latency is dominated by disk I/O, not crypto. The extra 28 bytes per page is negligible. |
| Storage size | ~0.7% | 28 bytes overhead per 4096-byte page. Plus 94-byte file header (negligible). |
| Memory | ~negligible | One DEK (32 bytes) per open tenant database. Nonces are generated per write, not stored in memory. |

On modern hardware with AES-NI (which covers all x86_64 and ARM64 targets
Neovex supports), AES-256-GCM processes ~6.4 GB/s. A 4KB page encrypts in
~0.6μs. For comparison, an NVMe fsync takes ~10-20μs. The crypto overhead is
dwarfed by the I/O overhead.

**The write path is the more affected path** because:
- encryption is slightly slower than decryption in GCM mode
- random nonce generation adds ~50ns per page (negligible)
- the read path benefits from OS page cache (many reads avoid disk entirely)

**AEGIS-256 upgrade would reduce overhead further.** AEGIS-256 achieves >10
GB/s with AES-NI (source: [jedisct1/rust-aegis benchmarks](https://github.com/jedisct1/rust-aegis)).
A 4KB page would encrypt in ~0.4μs. This is why Turso chose it.

---

## Work Items

### EAR1. Implement `EncryptedBackend` behind redb `StorageBackend` trait

**Priority:** highest
**Expected impact:** provides the core encryption primitive that all other
items build on.

#### Implementation plan

1. Create `crates/neovex-storage/src/encrypted_backend.rs`.
2. Verify the exact `StorageBackend` trait signature in redb 2.6.3 (the
   `read` method may take `&mut [u8]` output buffer rather than returning
   `Vec<u8>` — this affects internal buffer management).
3. Implement `StorageBackend` for `EncryptedBackend`:
   - `new(path, dek: [u8; 32])` — wraps a `FileBackend` with encryption
   - `write(offset, data)`:
     1. generate 12-byte random nonce via `ring::rand::SystemRandom`
     2. encrypt data with AES-256-GCM using DEK, with logical offset as AAD
     3. write `[nonce][ciphertext][tag]` to inner backend at physical offset
   - `read(offset, len)`:
     1. read `[nonce][ciphertext][tag]` from inner backend at physical offset
     2. decrypt with DEK, verify tag, passing logical offset as AAD
     3. return plaintext into caller's buffer
   - `set_len(len)` — translate logical length to physical length, delegate
   - `sync_data(eventual)` — delegate to inner
   - `len()` — translate physical length to logical length
4. Implement offset translation between logical (redb-visible) and physical
   (on-disk) address spaces, accounting for the file header and per-page
   nonce/tag overhead.
5. Write the cleartext file header on first create (see Design Decisions for
   format).
6. Validate the header on open (magic, version, cipher, HMAC).
7. Use `ring::aead::AES_256_GCM` for page encryption and
   `ring::aead::AES_256_GCM` for DEK encryption in the header.

#### Files likely to change

- `crates/neovex-storage/src/encrypted_backend.rs` (new)
- `crates/neovex-storage/src/lib.rs`
- `crates/neovex-storage/Cargo.toml` (if additional deps needed beyond `ring`)

#### Acceptance criteria

- `EncryptedBackend` passes redb's own read/write/sync contract
- round-trip: write encrypted, read back, data matches
- tampered ciphertext is detected and returns an error
- tampered nonce is detected and returns an error
- swapped pages (same ciphertext at different offset) are detected via AAD
- wrong DEK returns an error on read
- wrong KEK returns an error on header open
- header validation rejects unknown versions or ciphers
- nonces are random and non-deterministic (verified by checking that
  encrypting the same plaintext twice at the same offset produces different
  ciphertext)

---

### EAR2. Add key-provider trait with envelope encryption and file-based provider

**Priority:** highest
**Expected impact:** defines how encryption keys are provisioned and
retrieved without coupling to a specific key store.

#### Implementation plan

1. Define a `KeyProvider` trait:
   ```rust
   pub trait KeyProvider: Send + Sync + 'static {
       /// Returns the key encryption key (KEK) for a tenant.
       /// The KEK is used to encrypt/decrypt the per-database DEK
       /// stored in the file header.
       fn tenant_kek(&self, tenant_id: &TenantId) -> Result<[u8; 32]>;

       /// Returns the system KEK for the control database.
       fn system_kek(&self) -> Result<[u8; 32]>;
   }
   ```
2. Implement `FileKeyProvider` that reads KEKs from a directory:
   - `{key_dir}/{tenant_id}.kek` for tenant KEKs (32 bytes, hex-encoded)
   - `{key_dir}/system.kek` for the system KEK
   - keys are read once and cached in memory (KEKs are only needed on
     database open, not per-operation)
3. Implement `DerivedKeyProvider` that derives tenant KEKs from a master key
   using HKDF:
   - `tenant_kek = HKDF-Expand(master_key, "neovex-tenant-kek:" || tenant_id, 32)`
   - simpler operational model (one master key), weaker isolation (master key
     compromise exposes all tenant KEKs, though DEKs remain independently
     random)
4. Implement DEK lifecycle helpers:
   - `generate_dek()` — generates a random 256-bit DEK
   - `encrypt_dek(kek, dek)` — encrypts DEK with KEK for header storage
   - `decrypt_dek(kek, encrypted_dek)` — decrypts DEK from header
5. Add key generation CLI helper (`neovex keygen`) that writes a random key
   file.

#### Files likely to change

- `crates/neovex-storage/src/key_provider.rs` (new)
- `crates/neovex-storage/src/lib.rs`
- `crates/neovex-bin/src/main.rs`

#### Acceptance criteria

- `FileKeyProvider` reads KEKs from disk and returns them by tenant
- `DerivedKeyProvider` produces deterministic KEKs from a master key
- DEK encrypt/decrypt round-trips correctly
- wrong KEK fails DEK decryption with a clear error
- missing key files return a clear error
- key generation produces cryptographically random 32-byte keys

---

### EAR3. Integrate encrypted backend into `TenantStore` and `UsageStore`

**Priority:** highest after EAR1 + EAR2
**Expected impact:** the storage layer can open encrypted databases
end-to-end.

#### Implementation plan

1. Add an `open_encrypted_with_simulation(path, kek, clock, fault_injector)`
   constructor to `TenantStore` that:
   - if the file does not exist: generates a new DEK, creates the encrypted
     file header, and opens via
     `Database::builder().create_with_backend(EncryptedBackend::new(...))`
   - if the file exists: reads the header, decrypts the DEK with the KEK,
     and opens with the DEK
2. Add an `open_encrypted(path, kek)` constructor to `UsageStore`.
3. Update `EmbeddedRedbProvider` to accept an optional `Arc<dyn KeyProvider>`
   and use it when opening or creating tenant stores.
4. Add a `create_in_memory_encrypted_with_simulation(...)` constructor for
   tests that exercises the encryption path without disk I/O.
5. Run the full existing test suite through both encrypted and unencrypted
   backends to verify behavioral equivalence.

#### Files likely to change

- `crates/neovex-storage/src/store.rs`
- `crates/neovex-storage/src/usage_store.rs`
- `crates/neovex-storage/src/async_storage.rs`
- `crates/neovex-storage/src/tests.rs`

#### Acceptance criteria

- all existing storage tests pass on both encrypted and unencrypted backends
- `TenantStore` opened with encryption cannot be read with the wrong KEK
- `TenantStore` opened without encryption remains unchanged
- the encrypted file on disk is not readable as a valid redb database
  without the correct key material
- creating a new encrypted tenant generates a unique random DEK

---

### EAR4. Add typed runtime CLI / env / config surface

**Priority:** high
**Expected impact:** operators can enable encryption through the existing
CLI and configuration model.

#### Implementation plan

1. Define one typed runtime encryption config that can be populated from CLI
   flags, environment variables, and config files.
2. Add CLI flags and matching config-file/env inputs for the first provider
   choices:
   - `--encryption-key-dir <path>` / matching config key / matching env input
     — enables file-based key provider
   - `--encryption-master-key-file <path>` / matching config key / matching
     env input — enables derived key provider
   - the two are mutually exclusive at the typed-config validation layer
3. Lower that typed runtime config into the concrete `KeyProvider` and pass it
   to `EmbeddedRedbProvider`.
4. Add `GET /debug/encryption/status` endpoint exposing:
   - whether encryption is enabled
   - which provider type is active
   - cipher in use per tenant
   - per-tenant encryption status (encrypted vs. plaintext)
   - **do not expose key material in the status endpoint**
5. Document the CLI/env/config contract in the CLI reference.

#### Files likely to change

- `crates/neovex-bin/src/main.rs`
- `crates/neovex-server/src/lib.rs`
- `crates/neovex-server/src/http/`
- `docs/reference/cli.md`

#### Acceptance criteria

- encryption is disabled by default (no config = plaintext)
- providing a valid key dir or master key source through the typed
  CLI/env/config surface enables encryption for new tenants
- conflicting or incomplete runtime config fails validation before the service
  starts
- the debug endpoint reports accurate encryption status without leaking keys
- invalid configuration (both flags, missing files) produces clear errors

---

### EAR5. Add migration path for unencrypted-to-encrypted databases

**Priority:** high
**Expected impact:** existing deployments can adopt encryption without
data loss.

#### Implementation plan

1. Add a `neovex migrate-encrypt` CLI command that:
   - opens an existing unencrypted tenant database (read-only)
   - creates a new encrypted database with a fresh DEK encrypted by the
     tenant's KEK
   - copies all data (using redb read transaction on source → write
     transaction on destination, table by table)
   - verifies the copy by comparing document counts, commit log heads, and
     schema checksums
   - atomically replaces the old file with the new one (`rename(2)` on the
     same filesystem)
2. Support `--all-tenants` for batch migration.
3. Add a reverse `neovex migrate-decrypt` for operational recovery.
4. Document expected migration time: sequential read + write of the entire
   database. At ~1 GB/s effective throughput on NVMe, a 10 GB database
   migrates in ~10-15 seconds. Disk-bound, not crypto-bound.

#### Files likely to change

- `crates/neovex-bin/src/main.rs`
- `crates/neovex-storage/src/` (migration helpers)

#### Acceptance criteria

- unencrypted database migrates to encrypted with identical logical content
- migration is atomic (old file not removed until new file is verified)
- reverse migration works for operational recovery
- migration of a large database completes without excessive memory usage
  (streaming, not bulk-load)
- migration is idempotent (re-running on an already-encrypted database is a
  no-op or clear error)

---

### EAR6. Add key rotation support (KEK rotation + DEK rotation)

**Priority:** high
**Expected impact:** operators can rotate encryption keys per enterprise
compliance requirements.

#### Two rotation modes

**KEK rotation (fast, common):** re-encrypts the DEK in the file header with
a new KEK. The data on disk is unchanged — only the 48-byte encrypted DEK
blob in the header is rewritten. Completes in microseconds. This is the
normal operational rotation for compliance (e.g., 90-day key rotation
policies).

**DEK rotation (slow, rare):** rewrites the entire database with a new DEK.
Required only if the DEK itself is compromised (the DEK only exists in memory
and in the KEK-encrypted header, so compromise is unlikely). Uses the same
copy-and-replace mechanism as EAR5 migration.

#### Implementation plan

1. Add `neovex rotate-kek --tenant <id>` CLI command:
   - reads the current file header
   - decrypts the DEK with the old KEK
   - re-encrypts the DEK with the new KEK
   - rewrites the header atomically (write to temp, fsync, rename)
   - update the header HMAC
2. Add `neovex rotate-dek --tenant <id>` CLI command:
   - full database rewrite with a new random DEK (same as migration)
   - the new DEK is encrypted with the current KEK
3. Support `--all-tenants` for batch rotation.
4. Document the difference between KEK and DEK rotation and when each is
   appropriate.

#### Expected rotation times

| Mode | 1 GB database | 10 GB database | 100 GB database |
| --- | --- | --- | --- |
| KEK rotation | <1 ms | <1 ms | <1 ms |
| DEK rotation | ~1-2 s | ~10-15 s | ~100-150 s |

KEK rotation time is constant (header-only rewrite). DEK rotation time is
proportional to database size at ~1 GB/s effective throughput on NVMe.

#### Files likely to change

- `crates/neovex-bin/src/main.rs`
- `crates/neovex-storage/src/encrypted_backend.rs`
- `crates/neovex-storage/src/` (rotation helpers)

#### Acceptance criteria

- KEK rotation updates the header without touching data pages
- DEK rotation produces a valid encrypted database with a new DEK
- the old KEK (for KEK rotation) or old DEK (for DEK rotation) no longer
  opens the rotated database
- both rotation modes are atomic (no data loss on failure)
- rotation of a live tenant is documented (KEK rotation needs no quiesce;
  DEK rotation requires quiesce → rotate → resume)

---

### EAR7. Performance measurement and documentation

**Priority:** high
**Expected impact:** operators have concrete numbers to evaluate the
encryption overhead against the estimates in this plan.

#### Implementation plan

1. Add a benchmark suite (`cargo bench`) that measures:
   - single-document insert (encrypted vs. unencrypted)
   - batch insert (32 documents via journal)
   - point read by ID (cold cache + warm cache)
   - table scan (100, 1k, 10k documents)
   - index scan (single-field equality)
   - mixed read/write workload (75% read / 25% write)
2. Run on representative hardware:
   - NVMe SSD (target deployment)
   - standard SATA SSD (lower bound)
3. Compare measured overhead against the estimates in this plan's Design
   Decisions section.
4. Document results in `docs/reference/encryption.md` with:
   - absolute throughput numbers
   - relative overhead percentages
   - hardware specifications used
   - guidance on when encryption overhead is and is not a concern
5. If measured overhead exceeds targets (>15% write, >10% read), investigate
   and optimize before shipping. Likely optimization paths:
   - switch to AEGIS-256 (faster cipher)
   - buffer pool for avoiding redundant page re-encryption
   - batch nonce generation

#### Files likely to change

- `crates/neovex-storage/benches/` (new)
- `docs/reference/encryption.md` (new)

#### Acceptance criteria

- benchmark suite exists and is reproducible via `cargo bench`
- overhead is measured and documented with hardware context
- results are compared against this plan's estimates
- if targets are exceeded, optimization is attempted or targets are revised
  with documented justification

---

### EAR8. Pluggable KMS provider implementations

**Priority:** deferred
**Expected impact:** enterprise deployments can use managed key services
instead of file-based keys.

#### Gate

Do not start this item until `EAR4` is done and the configuration surface
defines how providers are selected. Concrete provider scope should be driven
by actual deployment requirements rather than speculative coverage.

#### Implementation plan

1. Add `AwsKmsKeyProvider` using the AWS SDK for envelope encryption:
   - operator configures a KMS key ARN per tenant (or a single ARN with
     per-tenant encryption context)
   - `tenant_kek()` calls `Decrypt` with the stored encrypted KEK blob
   - on first tenant creation, calls `GenerateDataKey` to produce a new KEK
   - the encrypted KEK blob is stored alongside the tenant database
     (e.g., `{tenant_id}.kek.enc`)
2. Add `VaultKeyProvider` for HashiCorp Vault Transit secrets engine:
   - operator configures a Vault transit key name
   - `tenant_kek()` calls the `decrypt` endpoint with the stored wrapped KEK
   - on first creation, calls `datakey/plaintext` to generate a new KEK
3. Each provider implements the same `KeyProvider` trait from `EAR2`.

#### Acceptance criteria

- at least one KMS provider is implemented and tested against a real or
  emulated service (LocalStack for AWS, Vault dev mode)
- the provider selection is driven by CLI configuration from `EAR4`
- KMS provider failures produce clear errors that distinguish auth failures,
  key-not-found, and network errors

---

## Execution Log

| Date | Item | Outcome | Notes |
| --- | --- | --- | --- |
| 2026-04-02 | baseline | created | Created this plan based on architecture review findings and redb maintainer guidance on the `StorageBackend` encryption path. |
| 2026-04-02 | plan | revised | Incorporated review feedback: fixed critical nonce construction flaw (deterministic offset-derived nonces → random per-write nonces), added envelope encryption (KEK/DEK) model, added AAD for page-swap protection, added cipher evaluation table with sources, added per-page on-disk layout specification, added expected performance overhead estimates with reasoning, added redb-turbo and Turso/libSQL as prior art references, split key rotation into fast KEK rotation and slow DEK rotation, added security and cipher reference links throughout. |
