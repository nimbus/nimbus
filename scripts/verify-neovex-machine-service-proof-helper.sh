#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/neovex-machine-service-proof-verify.XXXXXX")"
tmp_dir="$(cd "${tmp_dir}" && pwd)"
trap 'rm -rf "${tmp_dir}"' EXIT

home_dir="${tmp_dir}/home"
runtime_root="${tmp_dir}/runtime-root"
bin_dir="${tmp_dir}/bin"
output_dir="${tmp_dir}/output"
project_dir="${tmp_dir}/project"
compose_file="${project_dir}/compose.yaml"
state_file="${tmp_dir}/service-state.txt"
quoted_state_file="$(printf '%q' "${state_file}")"

mkdir -p "${home_dir}" "${runtime_root}" "${bin_dir}" "${output_dir}" "${project_dir}"
printf 'up\n' > "${state_file}"
cat > "${compose_file}" <<'EOF'
name: demo-app
services:
  db:
    image: busybox:latest
    x_neovex:
      backend: container
EOF

cat > "${bin_dir}/neovex" <<EOF
#!/usr/bin/env bash
set -euo pipefail

state_file=${quoted_state_file}

if [[ "\${1:-}" == "machine" && "\${2:-}" == "status" ]]; then
  cat <<'OUT'
result: status
lifecycle: running
manager: ready
OUT
  exit 0
fi

if [[ "\${1:-}" != "service" ]]; then
  echo "unexpected args: \$*" >&2
  exit 64
fi

subcommand="\${2:-}"
shift 2

case "\${subcommand}" in
  config)
    cat <<'OUT'
project_name: demo-app
services:
  - db
OUT
    ;;
  up)
    printf 'up\n' > "\${state_file}"
    cat <<'OUT'
- action: started
  tenant_id: local-demo
  service_name: db
  sandbox_id: db-01aaa
  status: ready
OUT
    ;;
  list)
    if [[ "\$(cat "\${state_file}")" == "down" ]]; then
      echo "[]"
    else
      cat <<'OUT'
- tenant_id: local-demo
  service_name: db
  sandbox_id: db-01aaa
  status: ready
OUT
    fi
    ;;
  inspect)
    cat <<'OUT'
summary:
  tenant_id: local-demo
  service_name: db
  sandbox_id: db-01aaa
  status: ready
log_paths:
  ctr_log: /var/lib/neovex/service-sandboxes/container/state/containers/db-01aaa/ctr.log
OUT
    ;;
  ps)
    cat <<'OUT'
runtime_pid: 2002
conmon_pid: 1001
matching_processes:
  - "1001 /usr/bin/conmon"
  - "2002 /usr/bin/sleep 30"
OUT
    ;;
  logs)
    cat <<'OUT'
guest log line
OUT
    ;;
  down)
    printf 'down\n' > "\${state_file}"
    cat <<'OUT'
- action: stopped
  tenant_id: local-demo
  service_name: db
  sandbox_id: db-01aaa
  status: stopped
OUT
    ;;
  *)
    echo "unexpected service args: \$subcommand \$*" >&2
    exit 64
    ;;
esac
EOF

cat > "${bin_dir}/curl" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail

url="${@: -1}"

case "${url}" in
  http://localhost/healthz)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha1"}
OUT
    ;;
  http://localhost/v1/machine-api/capabilities)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"protocol_version":"v1alpha1","service_execution_ready":true,"service_execution_mode":"standard_containers","supported_service_backends":["container"],"supported_operations":["healthz","capabilities","service-sandboxes.image-start","service-sandboxes.list","service-sandboxes.inspect-current","service-sandboxes.logs","service-sandboxes.ps","service-sandboxes.stop"]}
OUT
    ;;
  http://localhost/v1/machine-api/service-sandboxes)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"sandboxes":[{"tenant_id":"local-demo","service_name":"db","sandbox_id":"db-01aaa","status":"ready"}]}
OUT
    ;;
  http://127.0.0.1:18080/healthz)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: text/plain

ok
OUT
    ;;
  *)
    echo "unexpected curl URL: ${url}" >&2
    exit 64
    ;;
esac
EOF

chmod +x "${bin_dir}/neovex" "${bin_dir}/curl"

bash "${repo_root}/scripts/collect-neovex-machine-service-proof.sh" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}" \
  --neovex "${bin_dir}/neovex" \
  --curl "${bin_dir}/curl" \
  --compose-file "${compose_file}" \
  --service db \
  --published-url http://127.0.0.1:18080/healthz \
  > "${output_dir}/stdout.txt"

summary_file="${output_dir}/summary.txt"

for expected_file in \
  "${output_dir}/machine-status.txt" \
  "${output_dir}/machine-api-health.txt" \
  "${output_dir}/machine-api-capabilities.txt" \
  "${output_dir}/service-config.txt" \
  "${output_dir}/service-up.txt" \
  "${output_dir}/machine-api-service-sandboxes.txt" \
  "${output_dir}/service-list.txt" \
  "${output_dir}/service-inspect.txt" \
  "${output_dir}/service-ps.txt" \
  "${output_dir}/service-logs.txt" \
  "${output_dir}/localhost-probe.txt" \
  "${output_dir}/service-down.txt" \
  "${output_dir}/service-list-after-down.txt"
do
  if [[ ! -f "${expected_file}" ]]; then
    echo "expected service-proof artifact missing: ${expected_file}" >&2
    exit 70
  fi
done

grep -F "service.name                       db" "${summary_file}" >/dev/null
grep -F "published.url                      http://127.0.0.1:18080/healthz" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_api_health[[:space:]]+ok path=${output_dir}/machine-api-health.txt cmd=${output_dir}/machine-api-health-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.service_up[[:space:]]+ok path=${output_dir}/service-up.txt cmd=${output_dir}/service-up-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_api_service_sandboxes[[:space:]]+ok path=${output_dir}/machine-api-service-sandboxes.txt cmd=${output_dir}/machine-api-service-sandboxes-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.localhost_probe[[:space:]]+ok path=${output_dir}/localhost-probe.txt cmd=${output_dir}/localhost-probe-command.txt$" "${summary_file}" >/dev/null
grep -F "result                             captured" "${summary_file}" >/dev/null

grep -F "manager: ready" "${output_dir}/machine-status.txt" >/dev/null
grep -F '"status":"ok"' "${output_dir}/machine-api-health.txt" >/dev/null
grep -F '"service_execution_ready":true' "${output_dir}/machine-api-capabilities.txt" >/dev/null
grep -F "project_name: demo-app" "${output_dir}/service-config.txt" >/dev/null
grep -F "action: started" "${output_dir}/service-up.txt" >/dev/null
grep -F '"sandbox_id":"db-01aaa"' "${output_dir}/machine-api-service-sandboxes.txt" >/dev/null
grep -F "service_name: db" "${output_dir}/service-list.txt" >/dev/null
grep -F "ctr.log" "${output_dir}/service-inspect.txt" >/dev/null
grep -F "runtime_pid: 2002" "${output_dir}/service-ps.txt" >/dev/null
grep -F "guest log line" "${output_dir}/service-logs.txt" >/dev/null
grep -F "HTTP/1.1 200 OK" "${output_dir}/localhost-probe.txt" >/dev/null
grep -F "action: stopped" "${output_dir}/service-down.txt" >/dev/null
grep -F "[]" "${output_dir}/service-list-after-down.txt" >/dev/null

echo "verified: neovex machine service proof helper captured deterministic forwarded-service evidence"
