#!/usr/bin/env bash
set -euo pipefail

CGROUP_ROOT="${NIMBUS_EIH5_CGROUP_ROOT:-/sys/fs/cgroup}"
CGROUP_NAME="${NIMBUS_EIH5_CGROUP_NAME:-nimbus-eih5-memory-$$}"
CGROUP_PATH="${CGROUP_ROOT}/${CGROUP_NAME}"
MEMORY_MAX_BYTES="${NIMBUS_EIH5_MEMORY_MAX_BYTES:-33554432}"
MEMORY_HIGH_BYTES="${NIMBUS_EIH5_MEMORY_HIGH_BYTES:-max}"
ALLOC_CHUNK_BYTES="${NIMBUS_EIH5_ALLOC_CHUNK_BYTES:-4194304}"
PROOF_TIMEOUT_SECONDS="${NIMBUS_EIH5_TIMEOUT_SECONDS:-15}"

cleanup() {
  if [[ -d "${CGROUP_PATH}" ]]; then
    sudo -n rmdir "${CGROUP_PATH}" >/dev/null 2>&1 || true
  fi
}
trap cleanup EXIT

require_command() {
  local command_name="$1"
  if ! command -v "${command_name}" >/dev/null 2>&1; then
    echo "blocked: required command is missing: ${command_name}" >&2
    exit 2
  fi
}

require_command stat
require_command awk
require_command python3
require_command sudo
require_command timeout

if ! sudo -n true >/dev/null 2>&1; then
  echo "blocked: passwordless sudo is required to create a cgroup proof scope" >&2
  exit 2
fi

if [[ "$(stat -fc %T "${CGROUP_ROOT}")" != "cgroup2fs" ]]; then
  echo "blocked: ${CGROUP_ROOT} is not a cgroup v2 filesystem" >&2
  exit 2
fi

if ! grep -qw "memory" "${CGROUP_ROOT}/cgroup.controllers"; then
  echo "blocked: cgroup v2 memory controller is not available under ${CGROUP_ROOT}" >&2
  exit 2
fi

sudo -n mkdir "${CGROUP_PATH}"
sudo -n sh -c "printf '%s\n' '${MEMORY_MAX_BYTES}' > '${CGROUP_PATH}/memory.max'"
sudo -n sh -c "printf '%s\n' '${MEMORY_HIGH_BYTES}' > '${CGROUP_PATH}/memory.high'"
if [[ -e "${CGROUP_PATH}/memory.swap.max" ]]; then
  sudo -n sh -c "printf '%s\n' 0 > '${CGROUP_PATH}/memory.swap.max'"
fi

echo "host=$(hostname)"
echo "kernel=$(uname -r)"
echo "cgroup_root=${CGROUP_ROOT}"
echo "cgroup_path=${CGROUP_PATH}"
echo "memory.max=${MEMORY_MAX_BYTES}"
echo "memory.high=${MEMORY_HIGH_BYTES}"
if [[ -e "${CGROUP_PATH}/memory.swap.max" ]]; then
  echo "memory.swap.max=$(sudo -n cat "${CGROUP_PATH}/memory.swap.max")"
fi
echo "memory.events.before:"
sudo -n cat "${CGROUP_PATH}/memory.events"

set +e
timeout --kill-after=2s "${PROOF_TIMEOUT_SECONDS}s" sudo -n env \
  NIMBUS_EIH5_CGROUP_PATH="${CGROUP_PATH}" \
  NIMBUS_EIH5_ALLOC_CHUNK_BYTES="${ALLOC_CHUNK_BYTES}" \
  python3 - <<'PY'
import os
import time

cgroup_path = os.environ["NIMBUS_EIH5_CGROUP_PATH"]
chunk_bytes = int(os.environ["NIMBUS_EIH5_ALLOC_CHUNK_BYTES"])

with open(os.path.join(cgroup_path, "cgroup.procs"), "w", encoding="utf-8") as procs:
    procs.write(str(os.getpid()))

chunks = []
while True:
    chunk = bytearray(chunk_bytes)
    for index in range(0, len(chunk), 4096):
        chunk[index] = 1
    chunks.append(chunk)
    time.sleep(0.005)
PY
status=$?
set -e

echo "allocation_exit_status=${status}"
echo "memory.events.after:"
events_after="$(sudo -n cat "${CGROUP_PATH}/memory.events")"
printf '%s\n' "${events_after}"

oom_count="$(
  printf '%s\n' "${events_after}" |
    awk '$1 == "oom" { print $2 }'
)"
oom_kill_count="$(
  printf '%s\n' "${events_after}" |
    awk '$1 == "oom_kill" { print $2 }'
)"
oom_count="${oom_count:-0}"
oom_kill_count="${oom_kill_count:-0}"

if [[ "${oom_count}" != "0" || "${oom_kill_count}" != "0" ]]; then
  echo "result=pass"
  echo "reason=cgroup-v2-memory-limit-fired"
  exit 0
fi

echo "result=fail" >&2
echo "reason=cgroup-v2-memory-limit-did-not-fire" >&2
exit 3
