#!/usr/bin/env bash
# Run the NKV0 Valkey external-mode conformance smoke lane against nimbus-kv.
#
# The runner pins Valkey source, spawns `nimbus kv` on loopback, injects AUTH into
# the temporary Valkey TCL client helpers, and runs the Nimbus NKV0 smoke slice
# under RESP2 and RESP3. It fails if either mode produces fewer than
# NIMBUS_KV_MIN_PASSING_ASSERTIONS passing behavioral tests, so an all-skipped
# or empty run cannot read green.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

VALKEY_REPO_URL="${NIMBUS_KV_VALKEY_REPO_URL:-https://github.com/valkey-io/valkey.git}"
VALKEY_TAG="${NIMBUS_KV_VALKEY_TAG:-9.1.0}"
VALKEY_REF="${NIMBUS_KV_VALKEY_REF:-c9e8005e9d0ec817e26c7db318861cb821409249}"
VALKEY_DIR="${NIMBUS_KV_VALKEY_DIR:-${REPO_ROOT}/target/nimbus-kv/valkey-${VALKEY_REF}}"
VALKEY_SLICE="${NIMBUS_KV_VALKEY_SLICE:-unit/type/nimbus_kv_smoke}"
MIN_PASSING_ASSERTIONS="${NIMBUS_KV_MIN_PASSING_ASSERTIONS:-1}"

HOST="${NIMBUS_KV_HOST:-127.0.0.1}"
PORT="${NIMBUS_KV_PORT:-$(python3 - <<'PY'
import socket
sock = socket.socket()
sock.bind(("127.0.0.1", 0))
print(sock.getsockname()[1])
sock.close()
PY
)}"
TENANT="${NIMBUS_KV_TENANT:-tenant-a}"
USERNAME="${NIMBUS_KV_USERNAME:-tenant-a}"
PASSWORD="${NIMBUS_KV_PASSWORD:-secret}"
SKIPFILE="${NIMBUS_KV_SKIPFILE:-${REPO_ROOT}/tests/nimbus-kv-skip.txt}"
NIMBUS_BIN="${REDISRS_SERVER_BIN:-${NIMBUS_KV_SERVER_BIN:-${REPO_ROOT}/target/debug/nimbus}}"

SERVER_PID=""
SERVER_LOG="$(mktemp -t nimbus-kv-conformance.XXXXXX.log)"
RUN_OUTPUT_DIR="$(mktemp -d -t nimbus-kv-conformance.XXXXXX)"
VALKEY_EXTERNAL_BIN_DIR="${RUN_OUTPUT_DIR}/valkey-bin"

cleanup() {
  if [ -n "${SERVER_PID}" ] && kill -0 "${SERVER_PID}" 2>/dev/null; then
    kill "${SERVER_PID}" 2>/dev/null || true
    wait "${SERVER_PID}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

require_tool() {
  if ! command -v "$1" >/dev/null 2>&1; then
    printf 'missing required tool: %s\n' "$1" >&2
    exit 1
  fi
}

ensure_nimbus_binary() {
  if [ -x "${NIMBUS_BIN}" ]; then
    return
  fi
  printf 'nimbus binary not found at %s; building nimbus-bin\n' "${NIMBUS_BIN}"
  cargo build -p nimbus-bin --bin nimbus
  if [ ! -x "${NIMBUS_BIN}" ]; then
    printf 'built nimbus-bin, but %s is still missing or not executable\n' "${NIMBUS_BIN}" >&2
    exit 1
  fi
}

ensure_valkey_checkout() {
  if [ ! -d "${VALKEY_DIR}/.git" ]; then
    mkdir -p "$(dirname "${VALKEY_DIR}")"
    git clone --depth 1 --branch "${VALKEY_TAG}" "${VALKEY_REPO_URL}" "${VALKEY_DIR}"
  fi

  local actual
  actual="$(git -C "${VALKEY_DIR}" rev-parse HEAD)"
  if [ "${actual}" != "${VALKEY_REF}" ]; then
    printf 'Valkey checkout mismatch: expected %s from tag %s, got %s in %s\n' \
      "${VALKEY_REF}" "${VALKEY_TAG}" "${actual}" "${VALKEY_DIR}" >&2
    exit 1
  fi
}

patch_valkey_auth_client() {
  python3 - "${VALKEY_DIR}/tests/support/valkey.tcl" <<'PY'
from pathlib import Path
import sys

path = Path(sys.argv[1])
text = path.read_text()
marker = "NIMBUS_KV_AUTH_PATCH_BEGIN"
if marker in text:
    raise SystemExit(0)

needle = "    ::valkey::valkey_reset_state $id\n"
auth_block = r'''
    # NIMBUS_KV_AUTH_PATCH_BEGIN
    if {[info exists ::env(NIMBUS_KV_PASSWORD)] && [string length $::env(NIMBUS_KV_PASSWORD)] > 0} {
        set auth_args [list AUTH]
        if {[info exists ::env(NIMBUS_KV_USERNAME)] && [string length $::env(NIMBUS_KV_USERNAME)] > 0} {
            lappend auth_args $::env(NIMBUS_KV_USERNAME)
        }
        lappend auth_args $::env(NIMBUS_KV_PASSWORD)
        set authcmd "*[llength $auth_args]\r\n"
        foreach part $auth_args {
            append authcmd "\$[string length $part]\r\n$part\r\n"
        }
        ::valkey::valkey_write $fd $authcmd
        flush $fd
        set auth_reply [::valkey::valkey_read_reply $id $fd]
        if {$auth_reply ne "OK"} {
            error "Nimbus KV AUTH failed: $auth_reply"
        }
    }
    # NIMBUS_KV_AUTH_PATCH_END
'''
if needle not in text:
    raise SystemExit(f"could not find insertion point in {path}")
path.write_text(text.replace(needle, needle + auth_block, 1))
PY
}

write_valkey_smoke_slice() {
  cat > "${VALKEY_DIR}/tests/unit/type/nimbus_kv_smoke.tcl" <<'TCL'
start_server {tags {"nimbus-kv"}} {
    test {NKV0 smoke GET SET DEL EXPIRE TTL INCR through external Nimbus KV} {
        if {$::force_resp3} {
            set hello [r HELLO 3 AUTH $::env(NIMBUS_KV_USERNAME) $::env(NIMBUS_KV_PASSWORD)]
            assert_match {*proto*} $hello
            assert_match {*3*} $hello
        }

        assert_equal OK [r set nkv0:string value]
        assert_equal value [r get nkv0:string]
        assert_equal 1 [r del nkv0:string]

        assert_equal OK [r set nkv0:counter 41]
        assert_equal 42 [r incr nkv0:counter]
        assert_equal 1 [r expire nkv0:counter 60]
        set ttl [r ttl nkv0:counter]
        assert {$ttl > 0}
        assert_equal 1 [r del nkv0:counter]
    }
}
TCL
}

prepare_valkey_external_bin_dir() {
  mkdir -p "${VALKEY_EXTERNAL_BIN_DIR}"
  cat > "${VALKEY_EXTERNAL_BIN_DIR}/valkey-server" <<'SH'
#!/usr/bin/env sh
echo "nimbus-kv external-mode conformance must not spawn valkey-server" >&2
exit 127
SH
  chmod +x "${VALKEY_EXTERNAL_BIN_DIR}/valkey-server"
}

wait_for_ready() {
  python3 - "${HOST}" "${PORT}" "${USERNAME}" "${PASSWORD}" "${SERVER_PID}" <<'PY'
import os
import socket
import sys
import time

host, port, username, password, pid = sys.argv[1], int(sys.argv[2]), sys.argv[3], sys.argv[4], int(sys.argv[5])

def encode(parts):
    out = f"*{len(parts)}\r\n".encode()
    for part in parts:
        data = part.encode()
        out += f"${len(data)}\r\n".encode() + data + b"\r\n"
    return out

deadline = time.monotonic() + 15
last_error = None
while time.monotonic() < deadline:
    try:
        os.kill(pid, 0)
    except OSError as exc:
        raise SystemExit(f"nimbus-kv process exited before readiness: {exc}")
    try:
        with socket.create_connection((host, port), timeout=0.5) as sock:
            sock.sendall(encode(["AUTH", username, password]))
            auth = sock.recv(4096)
            sock.sendall(encode(["PING"]))
            pong = sock.recv(4096)
            if b"+OK" in auth and b"PONG" in pong:
                raise SystemExit(0)
            last_error = f"AUTH={auth!r} PING={pong!r}"
    except Exception as exc:
        last_error = str(exc)
    time.sleep(0.1)

raise SystemExit(f"nimbus-kv did not become ready: {last_error}")
PY
}

run_valkey_mode() {
  local mode="$1"
  local out="${RUN_OUTPUT_DIR}/valkey-${mode}.out"

  printf '\n== Valkey external-mode %s slice %s ==\n' "${mode}" "${VALKEY_SLICE}"
  if [ "${mode}" = "RESP3" ]; then
    (
    cd "${VALKEY_DIR}"
    NIMBUS_KV_USERNAME="${USERNAME}" \
      NIMBUS_KV_PASSWORD="${PASSWORD}" \
      VALKEY_BIN_DIR="${VALKEY_EXTERNAL_BIN_DIR}" \
      ./runtest \
          --host "${HOST}" \
          --port "${PORT}" \
          --single "${VALKEY_SLICE}" \
          --skipfile "${SKIPFILE}" \
          --singledb \
          --clients 1 \
          --force-resp3
    ) 2>&1 | tee "${out}"
  else
    (
    cd "${VALKEY_DIR}"
    NIMBUS_KV_USERNAME="${USERNAME}" \
      NIMBUS_KV_PASSWORD="${PASSWORD}" \
      VALKEY_BIN_DIR="${VALKEY_EXTERNAL_BIN_DIR}" \
      ./runtest \
          --host "${HOST}" \
          --port "${PORT}" \
          --single "${VALKEY_SLICE}" \
          --skipfile "${SKIPFILE}" \
          --singledb \
          --clients 1
    ) 2>&1 | tee "${out}"
  fi

  local passed
  passed="$(python3 - "${out}" <<'PY'
import re
import sys
text = open(sys.argv[1], encoding="utf-8", errors="replace").read()
text = re.sub(r"\x1b\[[0-9;]*m", "", text)
print(len(re.findall(r"\[ok\]:", text)))
PY
)"
  if [ "${passed}" -lt "${MIN_PASSING_ASSERTIONS}" ]; then
    printf '%s produced %s passing behavioral assertions; minimum is %s. Refusing all-skipped green.\n' \
      "${mode}" "${passed}" "${MIN_PASSING_ASSERTIONS}" >&2
    exit 1
  fi
  printf '%s passing behavioral assertions: %s (minimum %s)\n' \
    "${mode}" "${passed}" "${MIN_PASSING_ASSERTIONS}"
}

require_tool git
require_tool python3
require_tool tclsh

if [ ! -f "${SKIPFILE}" ]; then
  printf 'missing skipfile: %s\n' "${SKIPFILE}" >&2
  exit 1
fi

ensure_nimbus_binary
ensure_valkey_checkout
patch_valkey_auth_client
write_valkey_smoke_slice
prepare_valkey_external_bin_dir

printf 'Pinned Valkey checkout: %s (%s)\n' "${VALKEY_REF}" "${VALKEY_DIR}"
printf 'Starting nimbus-kv: %s kv --bind %s:%s --tenant %s --username %s --password <redacted> --no-disk\n' \
  "${NIMBUS_BIN}" "${HOST}" "${PORT}" "${TENANT}" "${USERNAME}"
"${NIMBUS_BIN}" kv \
  --bind "${HOST}:${PORT}" \
  --tenant "${TENANT}" \
  --username "${USERNAME}" \
  --password "${PASSWORD}" \
  --no-disk >"${SERVER_LOG}" 2>&1 &
SERVER_PID="$!"

wait_for_ready
printf 'nimbus-kv ready on %s:%s (log %s)\n' "${HOST}" "${PORT}" "${SERVER_LOG}"

run_valkey_mode RESP2
run_valkey_mode RESP3

printf '\nNimbus KV Valkey conformance smoke passed: RESP2+RESP3, Valkey %s, slice %s, skipfile %s\n' \
  "${VALKEY_REF}" "${VALKEY_SLICE}" "${SKIPFILE}"
