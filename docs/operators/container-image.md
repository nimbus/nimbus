---
title: Container image
description: Run Nimbus from the official OCI image — pull, rotate the admin token, mount the state volume, and pin by digest with Docker, Compose, Podman, or Kubernetes.
sidebar:
  label: Container image
  order: 3
---

Every tagged Nimbus release publishes an application image to GitHub
Container Registry:

```text
ghcr.io/nimbus/nimbus:<version>
```

The image runs `nimbus` directly in the foreground — no init system, no
nested container manager. Your orchestrator owns restarts, networking,
probes, secrets, and volume lifecycle.

## Image contract

| Field | Value |
| --- | --- |
| Image | `ghcr.io/nimbus/nimbus:<version>` (multi-arch: linux/amd64, linux/arm64) |
| Per-arch tags | `<version>-amd64`, `<version>-arm64` |
| Entrypoint | `nimbus` |
| Default command | `start --host 0.0.0.0 --allow-network --data-dir /var/lib/nimbus/data --control-data-dir /var/lib/nimbus/control` |
| User | UID/GID `10001:10001` (non-root) |
| State volume | `/var/lib/nimbus` |
| HTTP/WebSocket port | `8080` |
| Health probe | `GET /health` (no auth required) |
| Logs | stdout/stderr |
| License | source-available `LicenseRef-Nimbus-Community`; text at `/usr/local/share/doc/nimbus/LICENSE` |

The `latest` tag only moves on stable releases. Do not use `latest` in
production — pin the version, and pin the digest for rollouts (below).

## 1. Rotate the admin token

The container binds `0.0.0.0`, and Nimbus refuses a non-loopback bind
unless the local admin token has been rotated within the last 30 days.
Rotate it once against the state volume before the first run:

```bash
docker volume create nimbus-data
docker run --rm -v nimbus-data:/var/lib/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z auth rotate-admin
```

The token lives inside the volume at
`/var/lib/nimbus/.local/share/nimbus/auth/token` and survives container
recreation. Repeat the rotation when the 30-day freshness window lapses,
then restart the container.

## 2. Run the server

```bash
docker run --rm -p 127.0.0.1:8080:8080 \
  -v nimbus-data:/var/lib/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z
```

Confirm it answers:

```bash
curl -s http://127.0.0.1:8080/health
```

```json
{"ok":true}
```

To call the authenticated API, read the token out of the volume and send
it as `Authorization: Bearer` (or `X-Nimbus-Admin-Token`):

```bash
NIMBUS_TOKEN=$(docker run --rm -v nimbus-data:/var/lib/nimbus \
  --entrypoint cat ghcr.io/nimbus/nimbus:vX.Y.Z \
  /var/lib/nimbus/.local/share/nimbus/auth/token | jq -r .token)

curl -s http://127.0.0.1:8080/api/tenants \
  -H "Authorization: Bearer $NIMBUS_TOKEN"
```

## 3. Pin by digest for production

Each release attaches `nimbus_oci_image.txt` (the image digest),
`nimbus_oci_sbom.json`, `nimbus_oci_vulns.sarif.json`, and
`nimbus_oci_attestation.json` to the
[GitHub Release](https://github.com/nimbus/nimbus/releases). Pull by
digest:

```bash
docker pull ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest>
```

Verify the build provenance of the published image with the GitHub CLI:

```bash
gh attestation verify \
  oci://ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> \
  --repo nimbus/nimbus --bundle-from-oci
```

## Docker Compose

```yaml
services:
  nimbus:
    image: ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest>
    ports:
      - "127.0.0.1:8080:8080"
    volumes:
      - nimbus-data:/var/lib/nimbus
    healthcheck:
      test: ["CMD", "curl", "-fsS", "http://127.0.0.1:8080/health"]
      interval: 30s
      timeout: 5s
      retries: 3
    restart: unless-stopped

volumes:
  nimbus-data:
```

Rotate the admin token against the same volume first (step 1).

## Podman with systemd

For a host-managed Podman deployment, let `nimbus node install
--container` generate the Quadlet unit instead of writing one by hand:

```bash
podman volume create nimbus-data
podman run --rm -v nimbus-data:/var/lib/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> auth rotate-admin

nimbus node install --container \
  --image ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> \
  --user --enable --now
```

See [Node lifecycle](/operators/node-lifecycle/) for the generated
artifacts, dry-run review, and diagnostics.

## Kubernetes

```yaml
apiVersion: apps/v1
kind: Deployment
metadata:
  name: nimbus
spec:
  selector:
    matchLabels:
      app: nimbus
  template:
    metadata:
      labels:
        app: nimbus
    spec:
      securityContext:
        runAsNonRoot: true
        runAsUser: 10001
        runAsGroup: 10001
        fsGroup: 10001
      containers:
        - name: nimbus
          image: ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest>
          ports:
            - containerPort: 8080
          volumeMounts:
            - name: state
              mountPath: /var/lib/nimbus
          readinessProbe:
            httpGet:
              path: /health
              port: 8080
          livenessProbe:
            httpGet:
              path: /health
              port: 8080
      volumes:
        - name: state
          persistentVolumeClaim:
            claimName: nimbus-state
```

Run `nimbus auth rotate-admin` against the same persistent volume before
exposing a non-loopback endpoint — as a one-time init Job or an
operator-controlled maintenance task.

## Configuration

The default command persists to the embedded SQLite backend under the
`/var/lib/nimbus` volume. To use an external storage backend, set the
provider environment variables on the container — for example:

```bash
docker run --rm -p 127.0.0.1:8080:8080 \
  -v nimbus-data:/var/lib/nimbus \
  -e NIMBUS_TENANT_PROVIDER=postgres \
  -e NIMBUS_POSTGRES_URL=postgres://nimbus:secret@db:5432/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z
```

See [storage backends](/operators/storage-backends/) for the provider
matrix and [configuration reference](/reference/configuration/) for every
flag and environment variable. The data and control directories are fixed
by the image's default command; override the container command to change
them.

## Next steps

- [Node lifecycle](/operators/node-lifecycle/) — systemd and Quadlet
  service management for a Nimbus node.
- [Updates](/operators/updates/) — moving a containerized deployment to a
  new release.
- [Hardening](/operators/hardening/) — TLS, reverse proxies, and what to
  keep off the public internet.
