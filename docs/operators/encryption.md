---
title: Encryption at rest
description: Enable encryption for Nimbus-owned local data, choose a key provider, rotate keys, and recover encrypted databases.
sidebar:
  label: Encryption at rest
  order: 5
---

Nimbus encrypts the local database files it owns: embedded SQLite and redb
tenant databases, the redb control-plane database, and libSQL replica
caches. Encryption is disabled by default and is enabled with the
`--encryption-key-provider` flag on `nimbus start`. External databases
(Postgres, MySQL, a remote libSQL primary) are encrypted by that database,
not by Nimbus.

For the full flag ↔ environment variable ↔ config-key table, see the
[configuration reference](/reference/configuration/).

## Enable encryption

1. Generate a 32-byte key file and lock down its permissions. Store it
   outside the data directory:

   ```bash
   openssl rand -out /secure/path/master.key 32
   chmod 400 /secure/path/master.key
   ```

2. Start Nimbus with encryption enabled:

   ```bash
   nimbus start \
     --encryption-key-provider master-key-file \
     --encryption-master-key-file /secure/path/master.key
   ```

New tenant databases are created encrypted from then on. Nimbus generates a
random data-encryption key (DEK) per protected file and stores the wrapped
DEK in a sidecar manifest at `<file>.nimbus-enc`. Keep manifests with their
database files — a database without its manifest cannot be opened.

If you enable encryption on a server that already has plaintext database
files, startup fails for those files with a "sidecar manifest is missing"
error. Migrate them first — see
[Migrate existing plaintext data](#migrate-existing-plaintext-data).

## What gets encrypted

| Local file | Cipher |
| --- | --- |
| Embedded SQLite tenant databases | SQLCipher |
| Embedded redb tenant databases | AES-256-GCM-SIV (per page) |
| Control-plane redb database (`nimbus-control.db`) | AES-256-GCM-SIV (per page) |
| libSQL replica cache files | SQLCipher |

With `--tenant-provider postgres` or `mysql`, only the local control-plane
database is encryptable by Nimbus; tenant data encryption belongs to the
external database.

## Choose a key provider

Exactly one provider is active at a time. Flags belonging to a provider you
have not selected are rejected at startup, and encryption flags without
`--encryption-key-provider` are also rejected.

### master-key-file (recommended)

One operator-managed 32-byte key file. Nimbus derives a unique wrapping key
per protected file from it with HKDF-SHA256, so the single key protects many
databases without reusing DEKs:

```bash
nimbus start \
  --encryption-key-provider master-key-file \
  --encryption-master-key-file /secure/path/master.key
```

The file must contain exactly 32 bytes of raw key material — not hex, not
base64.

### key-dir

One 32-byte wrapping-key file per protected subject, for deployments that
want explicit per-tenant or per-role key custody:

```bash
nimbus start \
  --encryption-key-provider key-dir \
  --encryption-key-dir /secure/path/keys/
```

Key files are named by hex-encoding the subject descriptor recorded in the
manifest, with a `.key` extension. For example, the descriptor
`db:sqlite:tenant:demo:demo.sqlite3` maps to
`64623a73716c6974653a74656e616e743a64656d6f3a64656d6f2e73716c69746533.key`.
Each file must contain exactly 32 raw bytes.

### aws-kms

Envelope encryption with AWS KMS-managed wrapping keys. The per-file
manifest contract is unchanged — KMS replaces only the wrapping step:

```bash
nimbus start \
  --encryption-key-provider aws-kms \
  --encryption-aws-kms-key-id alias/nimbus-production \
  --encryption-aws-region us-east-1
```

`--encryption-aws-kms-key-id` is required; region and
`--encryption-aws-endpoint-url` (for VPC endpoints or local KMS emulators)
are optional, falling back to the standard AWS credential and region chain.
Nimbus calls `GenerateDataKey` to create DEKs, `Decrypt` to reopen
databases, and `ReEncrypt` during key rotation, and binds manifest metadata
into the KMS `EncryptionContext` — so grant the server identity those KMS
permissions on the selected key. A tampered manifest or wrong key surfaces
as a decrypt failure, not silent corruption.

## Configure the admin commands

The `nimbus encryption` admin commands (`status`, `migrate`, `export`,
`rotate-kek`, `rotate-dek`) do not take server flags. They read the same
persistence and encryption settings from environment variables
(`NIMBUS_ENCRYPTION_*`, `NIMBUS_DATA_DIR`, …) or from the JSON config file
named by `NIMBUS_CONFIG`. Set those before running any admin command:

```bash
export NIMBUS_ENCRYPTION_KEY_PROVIDER=master-key-file
export NIMBUS_ENCRYPTION_MASTER_KEY_FILE=/secure/path/master.key
```

Every command below assumes this environment is in place. The commands
refuse to run when encryption is not configured.

## Check status

From the CLI:

```bash
nimbus encryption status            # human-readable
nimbus encryption status --format json
```

This reports the enabled state, the key provider, and which local file
families are covered under the current provider configuration.

On a running server, the same posture is available over HTTP. The endpoint
is admin-only — send the local admin token (see the
[self-host quickstart](/get-started/self-host/) for where the token lives):

```bash
curl -s http://localhost:8080/debug/encryption/status \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

```json
{
  "enabled": true,
  "encrypted_families": ["embedded_sqlite", "control_plane_redb"],
  "descriptor": {
    "status": "enabled",
    "provider": "master_key_file",
    "path": "/secure/path/master.key"
  }
}
```

The response never contains key material.

## Migrate existing plaintext data

Stop the server before migrating files it has open. Then convert each
plaintext database into an encrypted copy:

```bash
nimbus encryption migrate \
  --source /data/tenant-a.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a
```

- `--provider` is `sqlite`, `redb`, or `libsql-cache`.
- `--target` is optional; the default appends `.encrypted` to the source
  name. The target (and its manifest) must not already exist.
- `--tenant-id` names the owning tenant. For redb it may be omitted only
  for `nimbus-control.db`.
- Migration validates the encrypted copy afterwards unless you pass
  `--skip-validation`, and publishes the target only after the copy
  succeeds.
- Pass `--retire-source` to delete the plaintext source (and its SQLite
  sidecar files) after a successful migration.

libSQL replica caches are not migrated this way: restart the server with
encryption enabled and the cache rebuilds from the remote primary under the
new key.

## Rotate keys

### Rotate the key-encryption key (KEK)

KEK rotation rewraps the manifest only — database pages are not rewritten.
Run it with the current provider configured in the environment, and name the
replacement provider on the command:

```bash
NIMBUS_ENCRYPTION_KEY_PROVIDER=master-key-file \
NIMBUS_ENCRYPTION_MASTER_KEY_FILE=/secure/path/old.key \
nimbus encryption rotate-kek \
  --path /data/tenant-a.sqlite3 \
  --new-master-key-file /secure/path/new.key
```

- `--path` is the protected database file; pass `--all` with a directory
  path to rotate every `.nimbus-enc` manifest in that directory.
- The replacement provider is inferred when you pass exactly one of
  `--new-master-key-file`, `--new-key-dir`, or `--new-aws-kms-key-id`;
  set `--new-key-provider` explicitly otherwise.
- To rotate onto AWS KMS:

  ```bash
  nimbus encryption rotate-kek \
    --path /data/tenant-a.sqlite3 \
    --new-key-provider aws-kms \
    --new-aws-kms-key-id alias/nimbus-production \
    --new-aws-region us-east-1
  ```

After rotation, use the new key configuration for all subsequent starts and
admin commands.

### Rotate the data-encryption key (DEK)

DEK rotation replaces the per-file key and is provider-specific:

```bash
nimbus encryption rotate-dek \
  --path /data/tenant-a.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a
```

- **sqlite** — checkpoints WAL state, backs up the database artifact set
  to `.bak` copies, rekeys in place with SQLCipher's `PRAGMA rekey`, then
  updates the manifest. On failure the backups are restored automatically.
- **redb** — re-encrypts every page into a staged file under fresh nonces,
  then commits the staged database and manifest together. An interrupted
  rotation is recovered automatically on the next rotate or server start.
- **libsql-cache** — rotates the manifest to a fresh DEK and deletes the
  local cache files; stop any running instance first, and the next start
  rebuilds the cache from the remote primary under the new key.

Pass `--skip-backup` to skip the `.bak` copies if you have your own backup
in place.

## Recover

To export an encrypted database back to plaintext (disaster recovery, or
moving data somewhere that needs plaintext):

```bash
nimbus encryption export \
  --source /data/tenant-a.sqlite3 \
  --target /recovery/tenant-a.sqlite3 \
  --provider sqlite \
  --tenant-id tenant-a
```

`--target` is required and must not exist. libSQL caches cannot be
exported — rebuild them from the remote primary instead.

Backup rules:

- Back up the `.nimbus-enc` manifest sidecars together with the database
  files. KMS access alone cannot recover a database whose manifest is lost.
- Back up the key material itself — the master key file or key directory —
  or, for `aws-kms`, preserve access to the KMS key.
- Encrypted backups without the keys are unrecoverable. Test restores
  before you depend on them. See
  [backup and restore](/operators/backup-restore/).

## Troubleshoot

- **Key file missing or unreadable** — the configured master key or
  per-subject key file path does not exist or is not readable. Startup
  fails before serving traffic.
- **Wrong key size** — key files must be exactly 32 bytes of raw binary
  data, not hex or base64.
- **"sidecar manifest is missing"** — encryption is enabled for an
  existing plaintext file. Migrate it first
  (see [above](#migrate-existing-plaintext-data)).
- **Cannot open an encrypted database** — the configured provider cannot
  unwrap the DEK in the manifest, or the file and manifest no longer
  match. Verify the key material and restore the file/manifest pair from
  backup.
- **AWS KMS errors** — key-not-found means the key ID or alias does not
  resolve in the selected region; permission-denied means the caller lacks
  the KMS operation on the key; network errors point at region, endpoint
  override, VPC routing, or the AWS credential chain.
