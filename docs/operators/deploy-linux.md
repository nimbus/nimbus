---
title: Deploy to a Linux server
description: Take a fresh Linux server to a running, persistent, systemd-managed Nimbus — install, dedicated user, unit file, admin token, first request.
sidebar:
  label: Deploy to Linux
  order: 2
---

This tutorial takes a fresh Linux server to a Nimbus instance that survives
reboots, runs as its own system user, and answers authenticated requests.
It takes about fifteen minutes. If you just want to try Nimbus on your own
machine, the [self-host quickstart](/get-started/self-host/) is shorter —
this page is for putting a server on real infrastructure.

You need:

- A fresh Linux server — Debian, Ubuntu, Fedora, RHEL, or a derivative — on
  x86_64 or ARM64.
- A login with `sudo`.
- `curl` and `jq` installed (`sudo apt-get install -y curl jq` or
  `sudo dnf install -y curl jq`).

Every step below is run on the server.

## 1. Install Nimbus

```bash
curl -fsSL https://github.com/nimbus/nimbus/releases/latest/download/install.sh | sh
```

On Linux the install script:

- detects your distribution and architecture (x86_64 and ARM64 are
  supported),
- installs the system packages Nimbus's sandboxing stack depends on via
  `apt` or `dnf`,
- downloads the latest release, verifies its SHA-256 checksum (and its
  GitHub build attestation when the `gh` CLI is available),
- installs the `nimbus` binary to `/usr/local/bin/nimbus`, and
- installs the paired runtime components (`nimbus-libkrun`, `nimbus-crun`)
  under `/usr/libexec/nimbus`.

It may warn that `/dev/kvm` is missing or inaccessible — that only affects
hardware-isolated sandbox workloads, not the server itself, so it is safe to
continue with this tutorial.

Confirm the binary is on your `PATH`:

```bash
nimbus --version
```

## 2. Create a dedicated system user

Run Nimbus as its own unprivileged user rather than as root or your login
account:

```bash
sudo useradd --system --create-home --home-dir /var/lib/nimbus \
  --shell /usr/sbin/nologin nimbus
sudo chmod 700 /var/lib/nimbus
```

The home directory matters: Nimbus resolves its admin-token, state, and log
paths from the service user's home directory (it honors `XDG_DATA_HOME` and
`XDG_STATE_HOME` when set, and falls back to `~/.local/share` and
`~/.local/state`). With the home above, the admin token will live at
`/var/lib/nimbus/.local/share/nimbus/auth/token`.

## 3. Choose a data directory

Nimbus persists to an embedded SQLite backend by default. Without
`--data-dir` it writes to `./data` relative to the process working
directory, so a production deployment should always pass the flag
explicitly. Create the directory now:

```bash
sudo -u nimbus mkdir -p /var/lib/nimbus/data
```

Other storage backends (Postgres, MySQL, libSQL, redb) are selected with
`--tenant-provider` — see [storage backends](/operators/storage-backends/).
This tutorial stays on the SQLite default.

## 4. Write a systemd unit

Nimbus does not install a service file — you write one now. Create
`/etc/systemd/system/nimbus.service`:

```bash
sudo tee /etc/systemd/system/nimbus.service > /dev/null <<'EOF'
[Unit]
Description=Nimbus server
After=network.target

[Service]
Type=simple
User=nimbus
Group=nimbus
Environment=HOME=/var/lib/nimbus
WorkingDirectory=/var/lib/nimbus
ExecStart=/usr/local/bin/nimbus start --host 127.0.0.1 --port 8080 --data-dir /var/lib/nimbus/data
Restart=on-failure
NoNewPrivileges=true

[Install]
WantedBy=multi-user.target
EOF
```

Three things this unit gets right:

- **`Environment=HOME=/var/lib/nimbus`** pins the home directory the
  service sees, so the admin token always lands at the path from step 2
  regardless of how systemd populates the environment.
- **`--host 127.0.0.1 --port 8080`** are Nimbus's defaults, written out
  explicitly. The server binds to loopback only — `nimbus start` refuses a
  non-loopback host unless you opt in with `--allow-network`. Keeping the
  bind on loopback and putting a reverse proxy in front is the recommended
  shape; the [hardening guide](/operators/hardening/) covers both paths.
- **`nimbus start` runs in the foreground**, which is exactly what
  `Type=simple` expects.

This is a deliberately minimal unit. The
[hardening guide](/operators/hardening/) adds the systemd sandboxing
directives you should layer on once the service is running.

## 5. Start and enable the service

```bash
sudo systemctl daemon-reload
sudo systemctl enable --now nimbus
sudo systemctl status nimbus
```

The status output should show `active (running)`. The journal shows the
listen line:

```bash
sudo journalctl -u nimbus | grep listening
```

Confirm the server answers — the `/health` endpoint needs no
authentication:

```bash
curl -s http://127.0.0.1:8080/health
```

```json
{"ok":true}
```

## 6. Retrieve the local admin token

On first boot the server mints a local admin token and stores it as a JSON
file readable only by the service user (mode `0600`):

```text
/var/lib/nimbus/.local/share/nimbus/auth/token
```

Every native API request must present this token; requests without it get a
`401`. Read it into a shell variable:

```bash
NIMBUS_TOKEN=$(sudo jq -r .token /var/lib/nimbus/.local/share/nimbus/auth/token)
```

The token value starts with `nimbus_at_`. The file also records the token's
`generation`, `issuedAt` timestamp, and `scope`.

## 7. Make your first authenticated request

List tenants — empty on a fresh server:

```bash
curl -s http://127.0.0.1:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

```json
{"tenants":[]}
```

The token is accepted either as `Authorization: Bearer <token>` or as an
`X-Nimbus-Admin-Token: <token>` header.

Your server is up, persistent, and authenticated. To create a tenant and
start reading and writing documents, continue with steps 4–6 of the
[self-host quickstart](/get-started/self-host/) — the same commands work
against this server.

## Next steps

- [Security hardening](/operators/hardening/) — TLS termination, systemd
  sandboxing, token rotation, and what to keep off the public internet.
- [Storage backends](/operators/storage-backends/) — move from embedded
  SQLite to Postgres, MySQL, libSQL, or redb.
- [Encryption at rest](/operators/encryption/) — key providers and
  rotation.
- [Backup and restore](/operators/backup-restore/) and
  [updates](/operators/updates/) — day-2 operations.
- [Reference](/reference/) — every `nimbus start` flag and the native API.
