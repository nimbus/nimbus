# Encryption at Rest Reference

Neovex supports optional encryption at rest for Neovex-owned local files. This
guide covers operator setup, current coverage, migration and rotation flows,
and the limits that matter in an enterprise review.

For architecture rationale and design details, see
[Storage encryption architecture](../architecture/storage/encryption.md).

---

## Quick Start

Encryption is disabled by default. To enable it:

1. Generate a 32-byte random key file:
   ```bash
   openssl rand -out /secure/path/master.key 32
   chmod 400 /secure/path/master.key
   ```

2. Start Neovex with encryption enabled:
   ```bash
   neovex serve \
     --encryption-key-provider master-key-file \
     --encryption-master-key-file /secure/path/master.key
   ```

---

## Coverage

Neovex encrypts local files it owns. External databases remain the operator's
responsibility.

| Provider family | Local file ownership | Encryption posture |
| --- | --- | --- |
| Embedded SQLite | `.sqlite3` tenant files | Neovex encrypts via SQLCipher |
| Embedded redb | `.redb` tenant files | Neovex encrypts via AES-256-GCM-SIV |
| Control plane | `neovex-control.db` | Neovex encrypts via AES-256-GCM-SIV |
| libsql replica | Local cache files | Neovex encrypts via SQLCipher through the shared local-SQLite seam |
| Postgres | External database | Provider-managed |
| MySQL | External database | Provider-managed |
| Remote libsql/Turso | Remote primary | Provider-managed |

At-rest coverage also applies to the on-disk working files created by Neovex
migration, rebuild, cutover, and recovery tooling. In-memory bootstrap
payloads and HTTP responses are not at-rest artifacts unless Neovex later adds
a persisted export command for them.

---

## Key Provider Options

### master-key-file (Recommended)

A single 32-byte key file for self-hosted deployments.

```bash
neovex serve \
  --encryption-key-provider master-key-file \
  --encryption-master-key-file /secure/path/master.key
```

Requirements:
- Exactly 32 bytes of raw key material
- Store outside the data directory
- Restrict permissions, for example `chmod 400`

Neovex generates a fresh random DEK per protected path, stores the wrapped DEK
in `<protected-path>.neovex-enc`, and uses the master key file to derive
per-subject wrapping keys via HKDF-SHA256. One operator-managed key can
therefore protect many local databases without reusing their DEKs.

### key-dir

Per-subject key files for advanced deployments.

```bash
neovex serve \
  --encryption-key-provider key-dir \
  --encryption-key-dir /secure/path/keys/
```

This provider expects one 32-byte wrapping key file per subject. File names are
derived from the manifest subject descriptor, for example:

`db_sqlite_tenant_demo_demo.sqlite3.key`

Use `key-dir` when operators want explicit per-tenant or per-role key custody.

### aws-kms

Managed envelope encryption for enterprise deployments that want AWS IAM,
CloudTrail visibility, and KMS-managed wrapping keys without changing the
per-database manifest contract.

```bash
neovex serve \
  --encryption-key-provider aws-kms \
  --encryption-aws-kms-key-id alias/neovex-production \
  --encryption-aws-region us-east-1
```

When `aws-kms` is selected, Neovex:
- generates one random DEK per protected path with `GenerateDataKey`
- stores the KMS ciphertext blob in the same `<protected-path>.neovex-enc`
  sidecar manifest used by the local providers
- binds manifest metadata into AWS `EncryptionContext`
- reopens databases with `Decrypt`
- uses `ReEncrypt` during KEK rotation when the wrapped key is already a KMS
  ciphertext blob

---

## Environment Variables

All encryption flags have environment variable equivalents:

| Flag | Environment Variable |
| --- | --- |
| `--encryption-key-provider` | `NEOVEX_ENCRYPTION_KEY_PROVIDER` |
| `--encryption-master-key-file` | `NEOVEX_ENCRYPTION_MASTER_KEY_FILE` |
| `--encryption-key-dir` | `NEOVEX_ENCRYPTION_KEY_DIR` |
| `--encryption-aws-kms-key-id` | `NEOVEX_ENCRYPTION_AWS_KMS_KEY_ID` |
| `--encryption-aws-region` | `NEOVEX_ENCRYPTION_AWS_REGION` |
| `--encryption-aws-endpoint-url` | `NEOVEX_ENCRYPTION_AWS_ENDPOINT_URL` |

Precedence: CLI > environment > config file.

`neovex encryption ...` admin commands read the current provider and
persistence settings from environment variables and config-file resolution.
`rotate-kek` also accepts replacement-provider flags on the command itself
(`--new-key-provider`, `--new-master-key-file`, `--new-key-dir`,
`--new-aws-kms-key-id`, `--new-aws-region`, `--new-aws-endpoint-url`).

---

## Status and Diagnostics

### HTTP Endpoint

```bash
curl http://localhost:8080/debug/encryption/status
```

Response shape:

```json
{
  "enabled": true,
  "descriptor": {
    "status": "enabled",
    "provider": "master_key_file",
    "path": "/secure/path/master.key"
  },
  "encrypted_families": [
    "embedded_sqlite",
    "control_plane_redb"
  ]
}
```

The descriptor never includes raw key material or wrapped-key blobs.

The current status surfaces are configuration-oriented: they report which local
families are configured for encryption, not a full on-disk residue scan for
every plaintext exception or retirement-pending artifact.

### CLI Status

```bash
neovex encryption status
```

Reports enabled state, key provider, and configured family coverage.

---

## Benchmark Evidence

The checked-in benchmark harnesses can measure the encryption delta through the
real manifest-backed startup path instead of a synthetic in-process shortcut.

Embedded local storage:

```bash
make bench-embedded-providers \
  REPORT=/tmp/neovex-bench/embedded-plaintext.md

make bench-embedded-providers \
  ENCRYPTION=temp-master-key-file \
  REPORT=/tmp/neovex-bench/embedded-encrypted.md
```

Replica-connected SQLite with encrypted local cache:

```bash
NEOVEX_LIBSQL_URL='http://127.0.0.1:18080' \
NEOVEX_LIBSQL_ADMIN_URL='http://127.0.0.1:18081' \
make bench-libsql-replica-provider \
  WORKLOADS='point-read indexed-query composite-indexed-query barrier-refresh peer-catch-up' \
  ENCRYPTION=temp-master-key-file \
  REPORT=/tmp/neovex-bench/libsql-replica-encrypted-cache.md
```

To capture hardware context plus the benchmark reports in one directory:

```bash
make collect-encryption-benchmark-evidence \
  OUTPUT_DIR=/tmp/neovex-bench-evidence
```

That collector writes `system-info.log`, per-command logs, embedded plaintext
and encrypted reports, and the focused encrypted libsql replica local-cache
report when the local libsql environment variables are set. The libsql capture
is intentionally scoped to the plan-owned reopen and freshness drills
(`point-read`, `indexed-query`, `composite-indexed-query`, `barrier-refresh`,
and `peer-catch-up`) rather than the broader write-heavy embedded contrast
lanes. The benchmark-only `temp-master-key-file` mode exists only for
reproducible measurement; production operators should use their real
`master-key-file`, `key-dir`, or `aws-kms` configuration.

Checked-in benchmark artifacts from the first manifest-backed embedded capture:

- [Summary report](../research/encryption-at-rest-benchmark-report.md)
- [Embedded plaintext raw report](../research/encryption-at-rest-embedded-plaintext-benchmark-report.md)
- [Embedded encrypted raw report](../research/encryption-at-rest-embedded-encrypted-benchmark-report.md)
- [Replica-connected SQLite local-cache raw report](../research/encryption-at-rest-libsql-replica-encrypted-cache-benchmark-report.md)

---

## Migration Workflows

`neovex encryption migrate`, `export`, and `rotate-*` reuse the same
encryption-provider configuration as `serve`. In practice that means setting
`NEOVEX_ENCRYPTION_*` or using the config file loaded through `NEOVEX_CONFIG`
before invoking the admin command.

### Migrate Plaintext to Encrypted (SQLite)

```bash
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/path/master.key \
neovex encryption migrate \
  --source /data/tenant.sqlite3 \
  --target /data/tenant-encrypted.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a
```

The migration:
1. Opens the plaintext source database.
2. Generates a fresh random DEK for the target.
3. Writes a manifest sidecar for the target path.
4. Creates the encrypted target with SQLCipher's `sqlcipher_export()`.
5. Publishes the encrypted database only after the export succeeds.
6. Optionally retires the predecessor plaintext artifact set.

### Export Encrypted to Plaintext (Recovery)

```bash
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/path/master.key \
neovex encryption export \
  --source /data/tenant-encrypted.sqlite3 \
  --target /data/tenant-recovery.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a
```

Use this for disaster recovery or interoperability when plaintext is required.

---

## Key Rotation

### KEK Rotation

KEK rotation rewraps the manifest only; database pages are not rewritten.

```bash
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/path/old.key \
neovex encryption rotate-kek \
  --path /data/tenant.sqlite3 \
  --new-master-key-file /secure/path/new.key
```

Use `--all` with a directory path to rotate every manifest sidecar in that
directory.

You can also rotate a manifest to AWS KMS:

```bash
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/path/old.key \
neovex encryption rotate-kek \
  --path /data/tenant.sqlite3 \
  --new-key-provider aws-kms \
  --new-aws-kms-key-id alias/neovex-production \
  --new-aws-region us-east-1
```

### DEK Rotation (SQLite)

DEK rotation rewrites database pages.

```bash
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/path/master.key \
neovex encryption rotate-dek \
  --path /data/tenant.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a
```

For SQLite, Neovex checkpoints WAL state first, backs up the SQLite artifact
set, runs `PRAGMA rekey`, and then updates the manifest so restart uses the new
DEK.

### DEK Rotation (redb)

```bash
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/path/master.key \
neovex encryption rotate-dek \
  --path /data/tenant.redb \
  --provider redb \
  --tenant-id tenant-a
```

For redb, Neovex re-encrypts pages into a new file using fresh nonces and the
same AAD contract as the runtime backend, then rewrites the manifest.

### DEK Rotation (libsql replica)

```bash
NEOVEX_ENCRYPTION_KEY_PROVIDER=master-key-file \
NEOVEX_ENCRYPTION_MASTER_KEY_FILE=/secure/path/master.key \
neovex encryption rotate-dek \
  --path /data/cache/tenant-a/tenant.sqlite3 \
  --provider libsql-cache \
  --tenant-id tenant-a
```

For libsql replica caches, Neovex rotates the manifest to a fresh DEK and
retires the local cache database files. The next service start rebuilds the
cache from the remote primary under the new key.

---

## Operational Notes

### Startup Behavior

- Zero encryption config means plaintext behavior.
- Encryption requires a valid local key source; missing keys fail before
  startup.
- Embedded SQLite, embedded redb, the retained redb control plane, and local
  libsql replica caches are all wired through manifest-backed startup opens.
- Postgres, MySQL, and the remote libsql primary remain
  `external-provider-managed`.

### Performance

Representative local embedded benchmark numbers are published in
[Encryption-at-rest benchmark report](../research/encryption-at-rest-benchmark-report.md).
Those numbers come from the repo-owned benchmark harness and evidence
collector, and they measure the real manifest-backed startup path rather than
an in-process shortcut. The corresponding focused replica-connected local-cache
reopen and freshness drills are published in
[Replica-connected SQLite local-cache raw report](../research/encryption-at-rest-libsql-replica-encrypted-cache-benchmark-report.md).
Those libsql numbers should be read as provider-path latency rather than as a
pure local-crypto delta, because the cold-start and freshness drills include
replica refresh semantics in addition to local cache reopen.

### Backup and Recovery

- Backups must include the configured local key material, or preserved AWS KMS
  access plus the manifest sidecars for KMS-backed deployments.
- Encrypted backups without keys are unrecoverable.
- Test recovery procedures before production.

### Key File Security

- Store key files outside the data directory.
- Restrict permissions to the minimal operator account set.
- Consider filesystem encryption for defense in depth.
- Never commit key files to version control.

### AWS KMS Considerations

- Grant `kms:GenerateDataKey`, `kms:Decrypt`, and `kms:ReEncrypt*` for the
  selected key.
- Keep the manifest sidecars with the database files; KMS unwrap alone is not
  enough without the subject metadata recorded in the manifest.
- `--encryption-aws-endpoint-url` is available for LocalStack or private VPC
  endpoints.
- Wrong key, wrong encryption context, or tampered manifest metadata should
  surface as decrypt failures rather than silent data corruption.

---

## Troubleshooting

### "key file not found"

The configured key file path does not exist or is not readable.

### "key file must contain exactly 32 bytes"

Key files must be exactly 32 bytes of raw binary data, not hex or base64.

### "encryption manifest not found"

Encryption is enabled for a local path, but the sidecar manifest is missing.
Migrate the plaintext database into an encrypted target before enabling
encrypted startup on that path.

### "cannot open encrypted database with wrong key"

The current provider cannot unwrap the DEK recorded in the manifest, or the
database file and manifest no longer match. Verify the configured key material
and restore from backup if the pair drifted.

### "aws kms key not found"

The configured KMS key ID or alias does not resolve in the selected region.
Verify the key identifier, region, and caller account.

### "aws kms denied Decrypt" / "aws kms denied ReEncrypt"

The active AWS identity can reach KMS but lacks permission for the requested
operation. Verify IAM policy, key policy, and any grant requirements.

### "aws kms network error"

Neovex could not reach KMS. Verify region, endpoint overrides, VPC routing,
and AWS credential-chain environment.

---

## Library Feature Flag

The shipped `neovex` CLI binary includes AWS KMS support. If you embed the
workspace crates directly and want to construct the KMS provider from Rust,
enable the `aws-kms` feature on `neovex`:

```bash
cargo build --release --features aws-kms
```
