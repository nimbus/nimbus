# Nimbus Container Image

Every tagged Nimbus release publishes a normal application OCI image:

```text
ghcr.io/nimbus/nimbus:<version>
```

The image runs `nimbus` directly in the foreground. It does not run an init
system or a nested container/runtime manager. Systemd-managed node installs and
the `ghcr.io/nimbus/machine-os:<version>` bootable machine image are separate
deployment surfaces.

## Contract

| Field | Value |
| --- | --- |
| Image | `ghcr.io/nimbus/nimbus:<version>` |
| Per-arch tags | `ghcr.io/nimbus/nimbus:<version>-amd64`, `ghcr.io/nimbus/nimbus:<version>-arm64` |
| Entrypoint | `["nimbus"]` |
| Default command | `start --host 0.0.0.0 --allow-network --data-dir /var/lib/nimbus/data --control-data-dir /var/lib/nimbus/control` |
| User | UID/GID `10001:10001` |
| State volume | `/var/lib/nimbus` |
| HTTP port | `8080` |
| Health probe | `GET /health` |
| Logs | stdout/stderr |
| License | `LicenseRef-Nimbus-Community`; text at `/usr/local/share/doc/nimbus/LICENSE` |

The release workflow builds the image from the release-produced Linux archives,
not from a fresh Cargo build inside the image job. The release also uploads
`nimbus_oci_image.txt`, `nimbus_oci_sbom.json`, `nimbus_oci_vulns.sarif.json`,
and `nimbus_oci_attestation.json` alongside the binary archives, top-level
`LICENSE` asset, and checksums.

## Running Locally

Nimbus protects non-loopback binds with an explicit admin-token freshness gate.
For a container volume, rotate that token once before exposing port `8080`:

```bash
docker volume create nimbus-data
docker run --rm -v nimbus-data:/var/lib/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z auth rotate-admin

docker run --rm -p 127.0.0.1:8080:8080 \
  -v nimbus-data:/var/lib/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z
```

Pin production rollouts by digest from `nimbus_oci_image.txt`:

```bash
docker pull ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest>
```

The release workflow only moves `latest` for stable tags. Do not use `latest`
for production rollouts; pin the version and digest instead.

## Compose

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

Use the orchestrator for restart policy, networking, probes, secrets, and
volume lifecycle. Do not install a service manager inside this image.

## Podman

For a host-managed Podman deployment, keep systemd on the host by using Quadlet
or an equivalent service-manager artifact. The container still runs `nimbus`
directly:

```ini
# ~/.config/containers/systemd/nimbus.container
[Unit]
Description=Nimbus application server
After=network-online.target
Wants=network-online.target

[Container]
Image=ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest>
ContainerName=nimbus
PublishPort=127.0.0.1:8080:8080
Volume=nimbus-data:/var/lib/nimbus
HealthCmd=curl -fsS http://127.0.0.1:8080/health
HealthInterval=30s

[Service]
Restart=on-failure

[Install]
WantedBy=default.target
```

Rotate the admin token once against the same named volume before enabling the
unit:

```bash
podman volume create nimbus-data
podman run --rm -v nimbus-data:/var/lib/nimbus \
  ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> auth rotate-admin

systemctl --user daemon-reload
systemctl --user enable --now nimbus.service
```

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

Initialize the state volume with `nimbus auth rotate-admin` before exposing a
non-loopback endpoint. In Kubernetes, run that once as an init job or an
operator-controlled maintenance task against the same persistent volume.

## Verification

For a published tag, run the live verifier to download the GitHub Release
bundle, validate the release archives, top-level `LICENSE`, checksum coverage,
`nimbus_oci_*` evidence, GitHub/Sigstore attestations for every checksummed
release asset plus the checksum manifest, registry-pushed GitHub/Sigstore
attestation, and the published image smoke test:

```bash
make verify-release-oci-image-live TAG=vX.Y.Z OUTPUT_DIR=/tmp/nimbus-vX.Y.Z-oci-proof
```

Release consumers can verify the digest and GitHub/Sigstore provenance with:

```bash
gh attestation verify \
  oci://ghcr.io/nimbus/nimbus:vX.Y.Z@sha256:<digest> \
  --repo nimbus/nimbus \
  --bundle-from-oci \
  --signer-workflow github.com/nimbus/nimbus/.github/workflows/release.yml \
  --predicate-type https://slsa.dev/provenance/v1 \
  --source-ref refs/tags/vX.Y.Z \
  --deny-self-hosted-runners
```

The release report also records the BuildKit SBOM inspection command, the
Trivy SARIF scan artifact, the release-evidence verifier command, and the
exact smoke-test command used by the release workflow. The downloaded
release bundle can be checked with `scripts/verify-release-oci-image-assets.sh`;
that verifier rejects evidence unless the attestation names the expected image
digest, its verified certificate names the `nimbus/nimbus` repository, release
workflow, and tag ref, includes verified timestamp evidence, proves a
GitHub-hosted runner identity, and `nimbus_oci_sbom.json` binds the SBOM to the
expected image, tag, ref, and digest with SPDX payloads for both linux/amd64
and linux/arm64. For a downloaded final GitHub Release bundle, run the verifier
with `--require-license --checksums <downloaded>/checksums-sha256.txt` so the
top-level `LICENSE` asset is also present and checksummed.
