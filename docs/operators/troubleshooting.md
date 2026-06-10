---
title: Troubleshooting
description: Symptom-to-fix entries for common Nimbus self-hosting failures, matched to the exact error messages the server emits.
sidebar:
  label: "Troubleshooting"
  order: 12
---

Each entry below quotes the real error text the server or CLI emits, so you
can match what you see in logs or terminal output. Native API errors arrive
as a JSON envelope with a `code` and `message` field, for example
`{"error": {"code": "auth.unauthorized", "message": "..."}}` — see the
[error reference](/reference/native/errors/) for the envelope shape.

## `nimbus start` exits with "Address already in use"

```text
Address already in use (os error 48)   # macOS
Address already in use (os error 98)   # Linux
```

**Cause:** another process is already listening on the port (default
`8080`) — often a previous Nimbus server that is still running.

**Fix:** find the listener and either stop it or move this server to a
different port:

```bash
lsof -iTCP:8080 -sTCP:LISTEN
nimbus start --port 8081
```

## `nimbus start` refuses a non-loopback host

```text
refusing to bind on non-loopback host `0.0.0.0` without --allow-network.
```

or, if the admin token is older than 30 days:

```text
refusing to bind on non-loopback host `0.0.0.0` with a stale local admin token.
```

**Cause:** the local admin token grants full control of the server, so
binding on a public interface is opt-in, and Nimbus additionally requires
the token to have been rotated within the last 30 days before a public
bind.

**Fix:** rotate the token, then re-run with the explicit opt-in:

```bash
nimbus auth rotate-admin
nimbus start --host 0.0.0.0 --allow-network
```

If you did not intend to expose the server, bind on `127.0.0.1`, `::1`, or
`localhost` instead.

## Admin API returns 401

```text
local admin access requires Authorization: Bearer <token> or X-Nimbus-Admin-Token
```

**Cause:** the request reached an admin route (`/api/tenants`,
`/debug/...`, and similar) without a valid local admin token.

**Fix:** read the token from the token file and send it on every request:

```bash
# Linux
export NIMBUS_TOKEN=$(jq -r .token ~/.local/share/nimbus/auth/token)
# macOS
export NIMBUS_TOKEN=$(jq -r .token "$HOME/Library/Application Support/nimbus/auth/token")

curl -s http://127.0.0.1:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

On Windows the file is `%LOCALAPPDATA%\nimbus\auth\token.json`. The header
can be either `Authorization: Bearer <token>` or `X-Nimbus-Admin-Token`.

## Requests fail with `auth.token_revoked` after a rotation

```text
auth.token_revoked
```

**Cause:** the admin token was rotated (`nimbus token rotate`,
`nimbus auth rotate-admin`, or the 401 above prompting a rotation), and a
client is still presenting the previous-generation token.

**Fix:** re-read the token file and update any scripts, CI secrets, or
shells that cached the old value. A related `auth.session_expired` message
means a console session outlived its lifetime — sign in again.

## Write rejected with "schema validation error"

```text
schema validation error: missing required field: author
schema validation error: field 'count' expected type Number, got string
```

**Cause:** the target table has a schema, and the document being written
is missing a required field or has a field whose type does not match.
Tables without a schema accept any document; setting a schema adds these
constraints.

**Fix:** correct the document to match the table schema, or update the
schema if the new shape is intended.

## "storage error [busy]" on the SQLite backend

```text
storage error [busy]: database is locked
```

**Cause:** another process is holding a write lock on a tenant database
file — typically a second Nimbus server pointed at the same data
directory, or an external `sqlite3` shell sitting in a write transaction.
Nimbus waits up to 5 seconds for the lock before giving up.

**Fix:** run exactly one server per data directory, and close any external
tools that have the database open for writing. Related kinds in the same
format are `storage error [unavailable]` (the database file cannot be
opened — check the path and filesystem permissions) and
`storage error [corruption]` (the file is damaged or is not a database —
restore from [backup](/operators/backup-restore/)).

## "encryption is enabled ... but the sidecar manifest is missing"

```text
encryption is enabled for data/demo.sqlite3, but the sidecar manifest is missing; migrate the plaintext database before enabling encryption
```

**Cause:** one of two things. Either you enabled
`--encryption-key-provider` on a server whose data directory already
contains plaintext databases, or you restored a backup that includes the
database file but not its `<file>.nimbus-enc` manifest sidecar.

**Fix:** for the first case, migrate the plaintext database:

```bash
nimbus encryption migrate --source data/demo.sqlite3 --provider sqlite --tenant-id demo
```

For the second case, restore the `.nimbus-enc` sidecar from backup. The
manifest holds the wrapped data-encryption key; without it the encrypted
database cannot be opened.

## "invalid encryption key or database is not encrypted"

```text
permission denied: invalid encryption key or database is not encrypted
```

A related variant when the provider type changed:

```text
encryption manifest for data/demo.sqlite3 was wrapped by 'master-key-file' but current config uses 'key-dir'
```

**Cause:** the key material the server loaded does not match what the
database (or its manifest) was encrypted with — usually a restore that
brought back the wrong master key file, or a config change to a different
key provider.

**Fix:** point the server back at the original key material
(`--encryption-master-key-file` / `--encryption-key-dir` / the original
KMS key) and the original key-provider type. Key startup failures have
their own messages you can match:

```text
failed to read master key from /etc/nimbus/master.key: No such file or directory (os error 2)
master key at /etc/nimbus/master.key has invalid size 44 (expected 32 bytes)
```

The master key file must contain exactly 32 raw bytes (not hex or base64
text).

## Server fails to start with "failed to read license file"

```text
failed to read license file /etc/nimbus/license.json: No such file or directory (os error 2)
failed to parse license file /etc/nimbus/license.json: expected value at line 1 column 1
```

**Cause:** `--license-file` or `NIMBUS_LICENSE_FILE` points at a missing
or malformed file. A configured-but-broken license aborts startup.

**Fix:** correct the path or file contents. Removing the flag and the
environment variable starts the server with community defaults. Once
running, inspect the active license and usage with the admin token:

```bash
curl -s http://127.0.0.1:8080/debug/license/status \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

## "No Convex or Cloud Functions surface found"

```text
No Convex or Cloud Functions surface found in /srv/myapp.

Create a `convex/` or `nimbus/` source directory, a `firebase.json`, or a Functions Framework `package.json` and place your app functions there.
```

**Cause:** `nimbus start --app-dir <dir>` (or `nimbus dev`) was pointed at
a directory that contains none of the recognized app layouts.

**Fix:** point `--app-dir` at the directory that contains your `convex/`
or `nimbus/` functions source (the project root, not the functions folder
itself), or scaffold one with `nimbus init`.

## Codegen fails during startup or `nimbus codegen`

```text
in-binary codegen failed for /srv/myapp: <error>. Set NIMBUS_CODEGEN_RUNNER=external-node for the diagnostic external Node.js runner.
```

**Cause:** generating app artifacts from your functions source failed —
most often a syntax or import error in the app code. Codegen runs inside
the Nimbus binary by default; no Node.js installation is required for it.

**Fix:** fix the underlying error in the app source. To cross-check with
the external Node.js runner as a diagnostic:

```bash
NIMBUS_CODEGEN_RUNNER=external-node nimbus codegen --app /srv/myapp
```

If that path reports
`failed to start Node.js for 'nimbus codegen --app /srv/myapp'`, the
external runner needs a Node.js installation on PATH — but the default
in-binary runner does not.

## "runtime bundle integrity check failed"

```text
runtime bundle integrity check failed: <bundle path> (expected <sha256>, got <sha256>)
```

**Cause:** Nimbus re-hashes runtime bundles before every invocation, and
the bundle file on disk no longer matches the hash recorded at
deploy time — something modified or partially overwrote the generated
artifacts.

**Fix:** regenerate and redeploy the artifacts (`nimbus codegen` followed
by `nimbus deploy`, or restart `nimbus dev`), and make sure nothing else
writes into the generated output directory.

## "unsupported tenant provider"

```text
invalid input: unsupported tenant provider 'postgresql'; expected sqlite, libsql-replica, redb, postgres, or mysql
```

**Cause:** a typo in `--tenant-provider`, `NIMBUS_TENANT_PROVIDER`, or the
config file.

**Fix:** use one of the exact names listed in the message. The same
pattern applies to the encryption flag:
`unsupported encryption key provider ...; expected master-key-file,
key-dir, or aws-kms`.
