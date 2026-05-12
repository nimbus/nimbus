#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_base="${TMPDIR:-/tmp}"
tmp_base="${tmp_base%/}"
tmp_dir="$(mktemp -d "${tmp_base}/nimbus-machine-service-proof-verify.XXXXXX")"
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
inspect_count_file="${tmp_dir}/inspect-count.txt"
quoted_inspect_count_file="$(printf '%q' "${inspect_count_file}")"
probe_count_file="${tmp_dir}/probe-count.txt"
quoted_probe_count_file="$(printf '%q' "${probe_count_file}")"

mkdir -p "${home_dir}" "${runtime_root}" "${bin_dir}" "${output_dir}" "${project_dir}"
printf 'up\n' > "${state_file}"
printf '0\n' > "${inspect_count_file}"
printf '0\n' > "${probe_count_file}"
cat > "${compose_file}" <<'EOF'
name: demo-app
services:
  db:
    build:
      context: .
      dockerfile: Dockerfile
    ports:
      - "18080:8080"
    x_nimbus:
      backend: container
EOF

cat > "${project_dir}/Dockerfile" <<'EOF'
FROM scratch
COPY healthz /healthz
EOF

printf 'ok\n' > "${project_dir}/healthz"

cat > "${bin_dir}/nimbus" <<EOF
#!/usr/bin/env bash
set -euo pipefail

state_file=${quoted_state_file}
inspect_count_file=${quoted_inspect_count_file}

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
source_file: __COMPOSE_FILE__
project_name: demo-app
services:
  db:
    backend: container
    source:
      kind: build
      image_name: nimbus-demo-app-db
      dockerfile_path: __DOCKERFILE_PATH__
      context_path: __CONTEXT_PATH__
    process: {}
    ports:
    - name: default
      protocol: tcp
      host_address: 127.0.0.1
      host_port: 18080
      guest_port: 8080
    resources: {}
    restart:
      policy: never
    x_nimbus:
      backend: container
OUT
    ;;
  up)
    printf 'up\n' > "\${state_file}"
    printf '0\n' > "\${inspect_count_file}"
    cat <<'OUT'
- action: started
  tenant_id: local-demo
  service_name: db
  sandbox_id: db-01aaa
  status: starting
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
    inspect_count="\$(cat "\${inspect_count_file}")"
    inspect_count="\$((inspect_count + 1))"
    printf '%s\n' "\${inspect_count}" > "\${inspect_count_file}"
    if [[ "\${inspect_count}" -lt 3 ]]; then
      cat <<'OUT'
{
  "summary": {
    "tenant_id": "local-demo",
    "service_name": "db",
    "sandbox_id": "db-01aaa",
    "status": "starting"
  },
  "log_paths": {
    "ctr_log": "/var/lib/nimbus/service-sandboxes/container/state/containers/db-01aaa/ctr.log"
  }
}
OUT
    else
      cat <<'OUT'
{
  "summary": {
    "tenant_id": "local-demo",
    "service_name": "db",
    "sandbox_id": "db-01aaa",
    "status": "ready"
  },
  "log_paths": {
    "ctr_log": "/var/lib/nimbus/service-sandboxes/container/state/containers/db-01aaa/ctr.log"
  }
}
OUT
    fi
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
probe_count_file=__PROBE_COUNT_FILE__

case "${url}" in
  http://localhost/healthz)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"status":"ok","role":"guest-machine-api","protocol_version":"v1alpha2"}
OUT
    ;;
  http://localhost/v1/machine-api/capabilities)
    cat <<'OUT'
HTTP/1.1 200 OK
content-type: application/json

{"protocol_version":"v1alpha2","service_execution_ready":true,"service_execution_mode":"standard_containers","supported_service_backends":["container"],"supported_operations":["healthz","capabilities","service-sandboxes.list","service-sandboxes.inspect","service-sandboxes.inspect-current","service-sandboxes.logs","service-sandboxes.ps","service-sandboxes.image-start","service-sandboxes.stop","service-sandboxes.build-start"],"binary_statuses":[{"name":"conmon","present":true,"resolved_path":"/usr/bin/conmon","required_for_operations":["service-sandboxes.image-start","service-sandboxes.build-start"]},{"name":"crun","present":true,"resolved_path":"/usr/bin/crun","required_for_operations":["service-sandboxes.image-start","service-sandboxes.build-start"]},{"name":"netavark","present":true,"resolved_path":"/usr/libexec/podman/netavark","required_for_operations":["service-sandboxes.image-start","service-sandboxes.build-start"]},{"name":"aardvark-dns","present":true,"resolved_path":"/usr/libexec/podman/aardvark-dns","required_for_operations":["service-sandboxes.image-start","service-sandboxes.build-start"]}],"operation_statuses":[{"name":"service-sandboxes.list","available":true,"blockers":[]},{"name":"service-sandboxes.inspect","available":true,"blockers":[]},{"name":"service-sandboxes.inspect-current","available":true,"blockers":[]},{"name":"service-sandboxes.logs","available":true,"blockers":[]},{"name":"service-sandboxes.ps","available":true,"blockers":[]},{"name":"service-sandboxes.image-start","available":true,"blockers":[]},{"name":"service-sandboxes.stop","available":true,"blockers":[]},{"name":"service-sandboxes.build-start","available":true,"blockers":[]}],"service_execution_blockers":[]}
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
    probe_count="$(cat "${probe_count_file}")"
    probe_count="$((probe_count + 1))"
    printf '%s\n' "${probe_count}" > "${probe_count_file}"
    if [[ "${probe_count}" -lt 2 ]]; then
      echo "connection refused" >&2
      exit 7
    fi
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

sed "s|__PROBE_COUNT_FILE__|${quoted_probe_count_file}|g" \
  "${bin_dir}/curl" > "${bin_dir}/curl.rendered"
mv "${bin_dir}/curl.rendered" "${bin_dir}/curl"

sed -e "s|__COMPOSE_FILE__|${compose_file}|g" \
    -e "s|__DOCKERFILE_PATH__|${project_dir}/Dockerfile|g" \
    -e "s|__CONTEXT_PATH__|${project_dir}|g" \
  "${bin_dir}/nimbus" > "${bin_dir}/nimbus.rendered"
mv "${bin_dir}/nimbus.rendered" "${bin_dir}/nimbus"

chmod +x "${bin_dir}/nimbus" "${bin_dir}/curl"

bash "${repo_root}/scripts/collect-nimbus-machine-service-proof.sh" \
  --home "${home_dir}" \
  --runtime-root "${runtime_root}" \
  --output-dir "${output_dir}" \
  --nimbus "${bin_dir}/nimbus" \
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
grep -E "^capture\\.service_ready[[:space:]]+ok attempts=3 path=${output_dir}/service-inspect.txt cmd=${output_dir}/service-inspect-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.machine_api_service_sandboxes[[:space:]]+ok path=${output_dir}/machine-api-service-sandboxes.txt cmd=${output_dir}/machine-api-service-sandboxes-command.txt$" "${summary_file}" >/dev/null
grep -E "^capture\\.localhost_probe[[:space:]]+ok attempts=2 path=${output_dir}/localhost-probe.txt cmd=${output_dir}/localhost-probe-command.txt$" "${summary_file}" >/dev/null
grep -F "result                             captured" "${summary_file}" >/dev/null

grep -F "manager: ready" "${output_dir}/machine-status.txt" >/dev/null
grep -F '"status":"ok"' "${output_dir}/machine-api-health.txt" >/dev/null
grep -F '"protocol_version":"v1alpha2"' "${output_dir}/machine-api-health.txt" >/dev/null
grep -F '"service_execution_ready":true' "${output_dir}/machine-api-capabilities.txt" >/dev/null
grep -F '"service-sandboxes.build-start"' "${output_dir}/machine-api-capabilities.txt" >/dev/null
grep -F "project_name: demo-app" "${output_dir}/service-config.txt" >/dev/null
grep -F "kind: build" "${output_dir}/service-config.txt" >/dev/null
grep -F "dockerfile_path: ${project_dir}/Dockerfile" "${output_dir}/service-config.txt" >/dev/null
grep -F "context_path: ${project_dir}" "${output_dir}/service-config.txt" >/dev/null
grep -F "action: started" "${output_dir}/service-up.txt" >/dev/null
grep -F '"sandbox_id":"db-01aaa"' "${output_dir}/machine-api-service-sandboxes.txt" >/dev/null
grep -F "service_name: db" "${output_dir}/service-list.txt" >/dev/null
grep -F '"status": "ready"' "${output_dir}/service-inspect.txt" >/dev/null
grep -F "ctr.log" "${output_dir}/service-inspect.txt" >/dev/null
grep -F "runtime_pid: 2002" "${output_dir}/service-ps.txt" >/dev/null
grep -F "guest log line" "${output_dir}/service-logs.txt" >/dev/null
grep -F "HTTP/1.1 200 OK" "${output_dir}/localhost-probe.txt" >/dev/null
grep -F "action: stopped" "${output_dir}/service-down.txt" >/dev/null
grep -F "[]" "${output_dir}/service-list-after-down.txt" >/dev/null

echo "verified: nimbus machine service proof helper captured deterministic forwarded-service evidence"
