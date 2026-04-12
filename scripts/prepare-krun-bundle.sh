#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: prepare-krun-bundle.sh --bundle-dir <path> --rootfs <path> --host-port <port> --guest-port <port> [options]

Prepare an OCI bundle config for the neovex krun validation flow. By default,
the helper runs "<runtime> spec" inside the bundle directory, then updates
config.json so the krun handler and krun.port_map annotation are present in the
expected "host:guest" form.

options:
  --bundle-dir <path>      Directory containing config.json (created if needed)
  --rootfs <path>          Root filesystem path to place in config.json
  --host-port <port>       Host-side TSI port
  --guest-port <port>      Guest-side service port
  --runtime <path>         crun binary to use for "spec" generation (default: crun)
  --config-path <path>     Edit an existing config.json path directly
  --skip-spec              Do not run "<runtime> spec"; require an existing config
  --process-arg <value>    Replace process.args (repeatable). Default is busybox httpd
  --cwd <path>             Set process.cwd (default: /)
  -h, --help               Show this help

examples:
  bash scripts/prepare-krun-bundle.sh \
    --bundle-dir /tmp/neovex-krun-probe \
    --rootfs /var/lib/containers/storage/overlay/.../merged \
    --host-port 18080 \
    --guest-port 8080

  bash scripts/prepare-krun-bundle.sh \
    --config-path /tmp/neovex-krun-probe/config.json \
    --skip-spec \
    --rootfs /srv/rootfs \
    --host-port 15432 \
    --guest-port 5432 \
    --process-arg /usr/bin/postgres \
    --process-arg -D \
    --process-arg /var/lib/postgresql/data
EOF
}

require_command() {
  local command_name="$1"

  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "required command not found: ${command_name}" >&2
    exit 69
  fi
}

bundle_dir=""
config_path=""
rootfs_path=""
runtime_path="crun"
host_port=""
guest_port=""
process_cwd="/"
skip_spec=0
process_args=()

while [[ $# -gt 0 ]]; do
  case "$1" in
    --bundle-dir)
      bundle_dir="${2:-}"
      shift 2
      ;;
    --config-path)
      config_path="${2:-}"
      shift 2
      ;;
    --rootfs)
      rootfs_path="${2:-}"
      shift 2
      ;;
    --runtime)
      runtime_path="${2:-}"
      shift 2
      ;;
    --host-port)
      host_port="${2:-}"
      shift 2
      ;;
    --guest-port)
      guest_port="${2:-}"
      shift 2
      ;;
    --process-arg)
      process_args+=("${2:-}")
      shift 2
      ;;
    --cwd)
      process_cwd="${2:-}"
      shift 2
      ;;
    --skip-spec)
      skip_spec=1
      shift
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "unknown argument: $1" >&2
      usage >&2
      exit 64
      ;;
  esac
done

if [[ -z "${rootfs_path}" || -z "${host_port}" || -z "${guest_port}" ]]; then
  usage >&2
  exit 64
fi

if [[ -z "${bundle_dir}" && -z "${config_path}" ]]; then
  usage >&2
  exit 64
fi

if [[ -n "${bundle_dir}" && -z "${config_path}" ]]; then
  config_path="${bundle_dir}/config.json"
fi

if [[ -z "${bundle_dir}" && -n "${config_path}" ]]; then
  bundle_dir="$(cd "$(dirname "${config_path}")" && pwd)"
fi

if [[ "${#process_args[@]}" -eq 0 ]]; then
  process_args=("/bin/busybox" "httpd" "-f" "-p" "${guest_port}")
fi

if [[ "${skip_spec}" -eq 0 ]]; then
  require_command "${runtime_path}"
  mkdir -p "${bundle_dir}"
  (
    cd "${bundle_dir}"
    "${runtime_path}" spec
  )
fi

if [[ ! -f "${config_path}" ]]; then
  echo "config.json not found: ${config_path}" >&2
  exit 66
fi

require_command python3

python3 - "${config_path}" "${rootfs_path}" "${host_port}" "${guest_port}" "${process_cwd}" "${process_args[@]}" <<'PY'
import json
from pathlib import Path
import sys

config_path = Path(sys.argv[1])
rootfs_path = sys.argv[2]
host_port = sys.argv[3]
guest_port = sys.argv[4]
process_cwd = sys.argv[5]
process_args = sys.argv[6:]

with config_path.open("r", encoding="utf-8") as fh:
    config = json.load(fh)

config.setdefault("root", {})
config["root"]["path"] = rootfs_path
config["root"]["readonly"] = False

config.setdefault("process", {})
config["process"]["cwd"] = process_cwd
config["process"]["args"] = process_args

annotations = config.setdefault("annotations", {})
annotations["run.oci.handler"] = "krun"
annotations["krun.port_map"] = f"{host_port}:{guest_port}"

with config_path.open("w", encoding="utf-8") as fh:
    json.dump(config, fh, indent=2)
    fh.write("\n")
PY

echo "bundle.dir=${bundle_dir}"
echo "bundle.config=${config_path}"
echo "bundle.rootfs=${rootfs_path}"
echo "bundle.port_map=${host_port}:${guest_port}"
printf 'bundle.process.args='
printf '%q ' "${process_args[@]}"
printf '\n'
