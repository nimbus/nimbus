#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-release-oci-image-build-helper.sh [--runtime docker]

Build a local fixture Nimbus OCI image from Containerfile and verify the
container metadata/runtime posture. This proves the image definition consumes
the release archive layout and produces the expected application-container
shape; it does not replace the published GHCR smoke proof for a real release.
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

runtime="docker"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --runtime)
      runtime="${2:-}"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      die "unknown argument: $1"
      ;;
  esac
done

[[ "${runtime}" == "docker" ]] || die "--runtime currently supports docker"
command -v "${runtime}" >/dev/null 2>&1 || die "${runtime} is required"
command -v jq >/dev/null 2>&1 || die "jq is required"
command -v tar >/dev/null 2>&1 || die "tar is required"

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
containerfile="${repo_root}/Containerfile"
[[ -f "${containerfile}" ]] || die "Containerfile is required"

tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-oci-build-helper.XXXXXX")"
image_tag="nimbus-release-oci-fixture:$RANDOM-$$"
volume="nimbus-oci-fixture-$RANDOM-$$"
container="nimbus-oci-fixture-$RANDOM-$$"

cleanup() {
  "${runtime}" rm -f "${container}" >/dev/null 2>&1 || true
  "${runtime}" volume rm "${volume}" >/dev/null 2>&1 || true
  "${runtime}" image rm -f "${image_tag}" >/dev/null 2>&1 || true
  rm -rf "${tmp_dir}"
}
trap cleanup EXIT

payload_dir="${tmp_dir}/payload"
context_dir="${tmp_dir}/context"
mkdir -p "${payload_dir}" "${context_dir}"

cat >"${payload_dir}/nimbus" <<'EOF'
#!/bin/sh
set -eu

case "${1:-}" in
  --version)
    echo "nimbus v9.9.9-fixture"
    ;;
  auth)
    if [ "${2:-}" = "rotate-admin" ]; then
      mkdir -p "${HOME:-/var/lib/nimbus}/control"
      echo fixture > "${HOME:-/var/lib/nimbus}/control/admin-token"
      echo "rotated fixture admin token"
    else
      echo "unexpected auth command: $*" >&2
      exit 64
    fi
    ;;
  start)
    echo "nimbus fixture stdout"
    echo "nimbus fixture stderr" >&2
    while :; do
      sleep 60
    done
    ;;
  *)
    echo "unexpected fixture command: $*" >&2
    exit 64
    ;;
esac
EOF

chmod 0755 "${payload_dir}/nimbus"
printf '# Nimbus fixture\n' >"${payload_dir}/README.md"
printf 'Nimbus fixture license\n' >"${payload_dir}/LICENSE"

COPYFILE_DISABLE=1 tar --no-xattrs --no-mac-metadata \
  -C "${payload_dir}" \
  -czf "${context_dir}/nimbus-fixture.tar.gz" \
  nimbus README.md LICENSE
cp "${containerfile}" "${context_dir}/Containerfile"

"${runtime}" build \
  --file "${context_dir}/Containerfile" \
  --build-arg NIMBUS_ARCHIVE=nimbus-fixture.tar.gz \
  --build-arg NIMBUS_VERSION=v9.9.9-fixture \
  --build-arg VCS_REF=fixture-vcs-ref \
  --build-arg BUILD_DATE=2026-05-26T00:00:00Z \
  --build-arg SOURCE_REPOSITORY=https://github.com/nimbus/nimbus \
  --tag "${image_tag}" \
  "${context_dir}" \
  >/dev/null

inspect_json="${tmp_dir}/inspect.json"
"${runtime}" image inspect "${image_tag}" >"${inspect_json}"

jq -e '.[0].Config.Entrypoint == ["nimbus"]' "${inspect_json}" >/dev/null ||
  die "image entrypoint is not [\"nimbus\"]"
jq -e '.[0].Config.Cmd == ["start","--host","0.0.0.0","--allow-network","--data-dir","/var/lib/nimbus/data","--control-data-dir","/var/lib/nimbus/control"]' "${inspect_json}" >/dev/null ||
  die "image default command is not the Nimbus foreground start command"
jq -e '.[0].Config.User == "10001:10001"' "${inspect_json}" >/dev/null ||
  die "image runtime user is not 10001:10001"
jq -e '.[0].Config.WorkingDir == "/var/lib/nimbus"' "${inspect_json}" >/dev/null ||
  die "image working directory is not /var/lib/nimbus"
jq -e '.[0].Config.ExposedPorts["8080/tcp"] != null' "${inspect_json}" >/dev/null ||
  die "image does not expose 8080/tcp"
jq -e '.[0].Config.Volumes["/var/lib/nimbus"] != null' "${inspect_json}" >/dev/null ||
  die "image does not declare /var/lib/nimbus as a volume"
jq -e '.[0].Config.Healthcheck.Test == ["CMD","curl","-fsS","http://127.0.0.1:8080/health"]' "${inspect_json}" >/dev/null ||
  die "image healthcheck does not probe /health with curl"
jq -e '.[0].Config.Labels["org.opencontainers.image.title"] == "Nimbus"' "${inspect_json}" >/dev/null ||
  die "missing OCI title label"
jq -e '.[0].Config.Labels["org.opencontainers.image.source"] == "https://github.com/nimbus/nimbus"' "${inspect_json}" >/dev/null ||
  die "missing OCI source label"
jq -e '.[0].Config.Labels["org.opencontainers.image.version"] == "v9.9.9-fixture"' "${inspect_json}" >/dev/null ||
  die "missing OCI version label"
jq -e '.[0].Config.Labels["org.opencontainers.image.revision"] == "fixture-vcs-ref"' "${inspect_json}" >/dev/null ||
  die "missing OCI revision label"
jq -e '.[0].Config.Labels["org.opencontainers.image.licenses"] == "LicenseRef-Nimbus-Community"' "${inspect_json}" >/dev/null ||
  die "missing OCI license label"

version_output="$("${runtime}" run --rm "${image_tag}" --version)"
[[ "${version_output}" == *"v9.9.9-fixture"* ]] ||
  die "fixture image did not route --version through the nimbus entrypoint"

"${runtime}" run --rm --entrypoint /bin/sh "${image_tag}" -c '
  set -eu
  grep -F "Nimbus fixture license" /usr/local/share/doc/nimbus/LICENSE >/dev/null
  for tool in systemd systemctl rc-service supervisord systemd-run podman buildah conmon crun qemu-system-x86_64 qemu-system-aarch64; do
    if command -v "${tool}" >/dev/null 2>&1; then
      echo "unexpected tool in default Nimbus image: ${tool}" >&2
      exit 1
    fi
  done
'

"${runtime}" volume create "${volume}" >/dev/null
"${runtime}" run --rm -v "${volume}:/var/lib/nimbus" --entrypoint /bin/sh "${image_tag}" -c '
  set -eu
  test "$(id -u)" = 10001
  test "$(id -g)" = 10001
  touch /var/lib/nimbus/.nimbus-write-test
  rm -f /var/lib/nimbus/.nimbus-write-test
'

"${runtime}" run --detach --name "${container}" "${image_tag}" >/dev/null
sleep 1
logs="$("${runtime}" logs "${container}" 2>&1)"
[[ "${logs}" == *"nimbus fixture stdout"* ]] ||
  die "default command did not emit stdout through container logs"
[[ "${logs}" == *"nimbus fixture stderr"* ]] ||
  die "default command did not emit stderr through container logs"

printf 'verified: fixture OCI image builds from release archive layout and has expected metadata, license, user, volume, logs, and forbidden-tool posture\n'
