#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/neovex-machine-cli-proof-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
cleanup() {
  local status="$1"
  if [[ "${status}" -eq 0 ]]; then
    rm -rf "${tmp_dir}"
  else
    echo "debug: preserved helper tmp dir at ${tmp_dir}" >&2
  fi
}
trap 'cleanup "$?"' EXIT

expected_neovex_version="$(
  awk -F'"' '/^version = / { print $2; exit }' "${repo_root}/Cargo.toml"
)"

if [[ -z "${expected_neovex_version}" ]]; then
  echo "failed to resolve expected neovex version from workspace Cargo.toml" >&2
  exit 70
fi

proof_root="${tmp_dir}/proof-root"
output_dir="${tmp_dir}/output"
bin_dir="${tmp_dir}/bin"
guest_binary_path="${tmp_dir}/guest-neovex"
script_log="${tmp_dir}/fake-script-calls.txt"
image_reference="docker://quay.io/podman/machine-os@sha256:deadbeef"

mkdir -p "${proof_root}" "${output_dir}" "${bin_dir}"
printf 'fake guest linux binary\n' > "${guest_binary_path}"

cat > "${bin_dir}/neovex" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

call_log="${XDG_STATE_HOME}/fake-neovex-calls.txt"
mkdir -p "$(dirname "${call_log}")"
printf '%s\n' "$*" >> "${call_log}"

expected_version="__EXPECTED_NEOVEX_VERSION__"

if [[ "${1:-}" == "--version" ]]; then
  printf 'neovex %s\n' "${expected_version}"
  exit 0
fi

if [[ "${1:-}" != "machine" ]]; then
  echo "unexpected args: $*" >&2
  exit 64
fi

subcommand="${2:-}"
shift 2

case "${subcommand}" in
  start)
    image="<default>"
    machine_name="default"
    while [[ $# -gt 0 ]]; do
      case "${1}" in
        --image)
          image="${2:?missing image reference}"
          shift 2
          ;;
        *)
          machine_name="${1}"
          shift
          ;;
      esac
    done
    printf '==> Starting machine "%s"\n' "${machine_name}" >&2
    printf '==> Pulling machine image %s\n' "${image}" >&2
    printf 'Machine "%s" initialized and started successfully\n' "${machine_name}"
    ;;
  status)
    format="table"
    machine_name="default"
    while [[ $# -gt 0 ]]; do
      case "${1}" in
        --format)
          format="${2:?missing output format}"
          shift 2
          ;;
        *)
          machine_name="${1}"
          shift
          ;;
      esac
    done
    case "${format}" in
      table)
        cat <<OUT
NAME               LIFECYCLE      MANAGER           PROVIDER   CPUS  MEMORY(MiB)  DISK(GiB) API
${machine_name}            running        ready             krunkit       2         2048         20 reachable
OUT
        ;;
      json)
        cat <<OUT
{"result":"status","name":"${machine_name}","lifecycle":"running","manager":"ready","provider":"krunkit"}
OUT
        ;;
      yaml)
        cat <<OUT
result: status
name: ${machine_name}
lifecycle: running
manager: ready
provider: krunkit
OUT
        ;;
      *)
        echo "unexpected status format: ${format}" >&2
        exit 64
        ;;
    esac
    ;;
  stop)
    printf 'Machine "%s" stopped successfully\n' "${1:-default}"
    ;;
  rm)
    printf 'Machine "%s" removed successfully\n' "${1:-default}"
    ;;
  *)
    echo "unexpected machine subcommand: ${subcommand}" >&2
    exit 64
    ;;
esac
EOF

cat > "${bin_dir}/script" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

script_log="__SCRIPT_LOG__"
printf '%s\n' "$*" >> "${script_log}"

if [[ "${1:-}" == "-q" ]]; then
  shift
fi

output_path="${1:?missing output path}"
shift

"$@" > "${output_path}" 2>&1
EOF

python3 - <<'PY' "${bin_dir}/neovex" "${expected_neovex_version}" "${bin_dir}/script" "${script_log}"
from pathlib import Path
import sys

neovex_path = Path(sys.argv[1])
expected_version = sys.argv[2]
script_path = Path(sys.argv[3])
script_log = sys.argv[4]

neovex_path.write_text(
    neovex_path.read_text().replace("__EXPECTED_NEOVEX_VERSION__", expected_version)
)
script_path.write_text(
    script_path.read_text().replace("__SCRIPT_LOG__", script_log)
)
PY

chmod +x "${bin_dir}/neovex" "${bin_dir}/script"

bash "${repo_root}/scripts/collect-neovex-machine-cli-proof.sh" \
  --machine proofbox \
  --root "${proof_root}" \
  --output-dir "${output_dir}" \
  --neovex "${bin_dir}/neovex" \
  --image "${image_reference}" \
  --guest-binary "${guest_binary_path}" \
  --script "${bin_dir}/script" \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"
call_log="${proof_root}/state/fake-neovex-calls.txt"
machine_start_command_file="${output_dir}/machine-start-command.txt"
machine_status_command_file="${output_dir}/machine-status-command.txt"
machine_status_json_command_file="${output_dir}/machine-status-json-command.txt"
machine_status_yaml_command_file="${output_dir}/machine-status-yaml-command.txt"

for expected_file in \
  "${output_dir}/host-neovex-version.txt" \
  "${output_dir}/machine-start-pty.txt" \
  "${output_dir}/machine-status.txt" \
  "${output_dir}/machine-status.json" \
  "${output_dir}/machine-status.yaml" \
  "${output_dir}/machine-stop.txt" \
  "${output_dir}/machine-rm.txt" \
  "${machine_start_command_file}" \
  "${machine_status_command_file}" \
  "${machine_status_json_command_file}" \
  "${machine_status_yaml_command_file}" \
  "${summary_file}" \
  "${call_log}" \
  "${script_log}"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected CLI-proof artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "proof.root                         ${proof_root}" "${summary_file}" >/dev/null
grep -F "output.dir                         ${output_dir}" "${summary_file}" >/dev/null
grep -F "machine.name                       proofbox" "${summary_file}" >/dev/null
grep -F "neovex.bin                         ${bin_dir}/neovex" "${summary_file}" >/dev/null
grep -F "script.bin                         ${bin_dir}/script" "${summary_file}" >/dev/null
grep -F "machine.image                      ${image_reference}" "${summary_file}" >/dev/null
grep -F "guest.binary.override              ${guest_binary_path}" "${summary_file}" >/dev/null
grep -F "cleanup.keep_machine               0" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_start[[:space:]]+ok path=${output_dir}/machine-start-pty.txt cmd=${output_dir}/machine-start-command.txt mode=pty$" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_status_json[[:space:]]+ok path=${output_dir}/machine-status.json cmd=${output_dir}/machine-status-json-command.txt$" "${summary_file}" >/dev/null
grep -E "^cleanup\\.machine_rm[[:space:]]+ok path=${output_dir}/machine-rm.txt cmd=${output_dir}/machine-rm-command.txt$" "${summary_file}" >/dev/null
grep -F "result                             captured" "${summary_file}" >/dev/null

grep -F "neovex ${expected_neovex_version}" "${output_dir}/host-neovex-version.txt" >/dev/null
grep -F '==> Starting machine "proofbox"' "${output_dir}/machine-start-pty.txt" >/dev/null
grep -F 'Machine "proofbox" initialized and started successfully' "${output_dir}/machine-start-pty.txt" >/dev/null
grep -F "NAME               LIFECYCLE" "${output_dir}/machine-status.txt" >/dev/null
grep -F '"name":"proofbox"' "${output_dir}/machine-status.json" >/dev/null
grep -F "provider: krunkit" "${output_dir}/machine-status.yaml" >/dev/null
grep -F 'Machine "proofbox" stopped successfully' "${output_dir}/machine-stop.txt" >/dev/null
grep -F 'Machine "proofbox" removed successfully' "${output_dir}/machine-rm.txt" >/dev/null

grep -F "NEOVEX_MACHINE_GUEST_BINARY=${guest_binary_path}" "${machine_start_command_file}" >/dev/null
grep -F -- "--image ${image_reference}" "${machine_start_command_file}" >/dev/null
grep -F "machine status proofbox" "${machine_status_command_file}" >/dev/null
grep -F "machine status --format json proofbox" "${machine_status_json_command_file}" >/dev/null
grep -F "machine status --format yaml proofbox" "${machine_status_yaml_command_file}" >/dev/null

grep -F "machine start --image ${image_reference} proofbox" "${call_log}" >/dev/null
grep -F "machine status proofbox" "${call_log}" >/dev/null
grep -F "machine status --format json proofbox" "${call_log}" >/dev/null
grep -F "machine status --format yaml proofbox" "${call_log}" >/dev/null
grep -F "machine stop proofbox" "${call_log}" >/dev/null
grep -F "machine rm proofbox" "${call_log}" >/dev/null

grep -F -- "-q ${output_dir}/machine-start-pty.txt" "${script_log}" >/dev/null

echo "verified: neovex machine CLI proof helper captures deterministic isolated-root local-binary evidence"
