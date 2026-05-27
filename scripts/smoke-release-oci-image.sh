#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: smoke-release-oci-image.sh --image <ref> --expected-version <vX.Y.Z> [--runtime docker|podman]

Smoke-test a published Nimbus application OCI image:
- nimbus --version reports the release version
- the runtime image does not contain host workload-management tools
- a rotated state volume can start nimbus on 0.0.0.0 and answer /health
EOF
}

die() {
  printf '%s\n' "$*" >&2
  exit 1
}

image=""
expected_version=""
runtime="docker"

while [[ $# -gt 0 ]]; do
  case "$1" in
    --image)
      image="${2:-}"
      shift 2
      ;;
    --expected-version)
      expected_version="${2:-}"
      shift 2
      ;;
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

[[ -n "${image}" ]] || die "--image is required"
[[ -n "${expected_version}" ]] || die "--expected-version is required"
case "${runtime}" in
  docker|podman) ;;
  *) die "--runtime must be docker or podman" ;;
esac

command -v "${runtime}" >/dev/null 2>&1 || die "${runtime} is required"
command -v curl >/dev/null 2>&1 || die "curl is required"

health_file="$(mktemp "${TMPDIR:-/tmp}/nimbus-oci-health.XXXXXX.json")"
volume="nimbus-oci-smoke-$RANDOM-$$"
container="nimbus-oci-smoke-$RANDOM-$$"
host_port=""

cleanup() {
  "${runtime}" rm -f "${container}" >/dev/null 2>&1 || true
  "${runtime}" volume rm "${volume}" >/dev/null 2>&1 || true
  rm -f "${health_file}"
}
trap cleanup EXIT

version_without_v="${expected_version#v}"
version_output="$("${runtime}" run --rm "${image}" --version)"
if [[ "${version_output}" != *"${expected_version}"* && "${version_output}" != *"${version_without_v}"* ]]; then
  die "expected ${image} --version to mention ${expected_version} or ${version_without_v}, got: ${version_output}"
fi

"${runtime}" run --rm --entrypoint /bin/sh "${image}" -c '
  set -eu
  test -s /usr/local/share/doc/nimbus/LICENSE
  for tool in systemd systemctl rc-service supervisord systemd-run podman buildah conmon crun qemu-system-x86_64 qemu-system-aarch64; do
    if command -v "${tool}" >/dev/null 2>&1; then
      echo "unexpected tool in default Nimbus image: ${tool}" >&2
      exit 1
    fi
  done
'

"${runtime}" volume create "${volume}" >/dev/null
"${runtime}" run --rm -v "${volume}:/var/lib/nimbus" --entrypoint /bin/sh "${image}" -c '
  set -eu
  test "$(id -u)" = 10001
  test "$(id -g)" = 10001
  touch /var/lib/nimbus/.nimbus-write-test
  rm -f /var/lib/nimbus/.nimbus-write-test
'
"${runtime}" run --rm -v "${volume}:/var/lib/nimbus" "${image}" auth rotate-admin >/dev/null

"${runtime}" run \
  --detach \
  --name "${container}" \
  -p 127.0.0.1::8080 \
  -v "${volume}:/var/lib/nimbus" \
  "${image}" \
  start \
  --host 0.0.0.0 \
  --allow-network \
  --data-dir /var/lib/nimbus/data \
  --control-data-dir /var/lib/nimbus/control \
  >/dev/null

for _ in {1..60}; do
  port_line="$("${runtime}" port "${container}" 8080/tcp 2>/dev/null | head -n 1 || true)"
  if [[ -n "${port_line}" ]]; then
    host_port="$(printf '%s\n' "${port_line}" | sed -E 's/.*:([0-9]+)$/\1/')"
  fi
  if [[ -n "${host_port}" ]] && curl -fsS "http://127.0.0.1:${host_port}/health" > "${health_file}"; then
    grep -Eq '"ok"[[:space:]]*:[[:space:]]*true' "${health_file}" ||
      die "/health did not report ok=true"
    printf 'verified: %s reports %s and serves /health on container port 8080\n' "${image}" "${expected_version}"
    exit 0
  fi
  sleep 1
done

"${runtime}" logs "${container}" >&2 || true
die "timed out waiting for ${image} to answer /health"
