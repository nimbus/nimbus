#!/usr/bin/env bash
set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-check-vmm-host-verify.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

bin_dir="${tmp_dir}/bin"
runtime_root="${tmp_dir}/runtime"
probe_tmp="${tmp_dir}/probe-tmp"
contract_env="${tmp_dir}/linux-distribution-contract.env"
crun_log="${tmp_dir}/crun.log"

mkdir -p "${bin_dir}" "${runtime_root}/lib" "${probe_tmp}"

cat > "${contract_env}" <<'EOF'
NIMBUS_CRUN_VERSION=v1.30.1-nimbus.9
NIMBUS_CRUN_UPSTREAM_VERSION=1.30.1
NIMBUS_LIBKRUN_VERSION=v1.19.6-nimbus.9
NIMBUS_LIBKRUN_UPSTREAM_VERSION=1.19.6
EOF

printf 'nimbus-libkrun=v1.19.6-nimbus.9\n' > "${runtime_root}/NIMBUS_LIBKRUN_RELEASE.txt"
touch "${runtime_root}/lib/libkrun.so.1.19.6" "${runtime_root}/lib/libkrun.so.1.17.4" "${runtime_root}/lib/libkrunfw.so.5.3.0"
ln -s libkrunfw.so.5.3.0 "${runtime_root}/lib/libkrunfw.so.5"

# The fake crun reports the dynamic loader trace that STUB_* selects. It loads
# nothing unless the bundle asks for the krun handler. It refuses to run with
# LD_LIBRARY_PATH set because the probe must use the service search path. It
# refuses to run with a cgroup manager because a failed run leaks the cgroup.
cat > "${runtime_root}/crun" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf '%s\n' "$*" >> "${STUB_CRUN_LOG}"
case "${1:-}" in
  --version)
    printf 'crun version %s\ncommit: 0000000\n+SYSTEMD +SECCOMP +LIBKRUN\n' "${STUB_CRUN_VERSION}"
    exit 0
    ;;
  --cgroup-manager=disabled)
    [[ "${2:-}" == "--root" ]] || { echo "unexpected args for fake crun: $*" >&2; exit 64; }
    state_dir="$3"
    command_name="$4"
    ;;
  --root)
    echo "probe must disable the cgroup manager" >&2
    exit 70
    ;;
  *)
    echo "unexpected args for fake crun: $*" >&2
    exit 64
    ;;
esac

case "${command_name}" in
  run)
    bundle_dir="$6"
    if [[ -n "${LD_LIBRARY_PATH+set}" ]]; then
      echo "LD_LIBRARY_PATH leaked into the probe" >&2
      exit 70
    fi
    if [[ "${LD_DEBUG:-}" == "libs" ]] && grep -F '"run.oci.handler": "krun"' "${bundle_dir}/config.json" >/dev/null; then
      if [[ -n "${STUB_LIBKRUN_PATH}" ]]; then
        printf '     4242:\tcalling init: %s\n     4242:\t\n' "${STUB_LIBKRUN_PATH}" >&2
      fi
      if [[ -n "${STUB_LIBKRUNFW_PATH}" ]]; then
        printf '     4242:\tcalling init: %s\n' "${STUB_LIBKRUNFW_PATH}" >&2
      fi
    fi
    mkdir -p "${state_dir}"
    echo 'open `rootfs-missing`: No such file or directory' >&2
    exit 1
    ;;
  delete)
    exit 0
    ;;
esac
echo "unexpected crun command: ${command_name}" >&2
exit 64
EOF

cat > "${bin_dir}/readelf" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
printf ' 0x000000000000001d (RUNPATH)            Library runpath: [%s]\n' "${STUB_RUNPATH}"
EOF

cat > "${bin_dir}/nm" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
echo "0000000000001000 T krun_set_port_map_with_bind_address"
EOF

chmod +x "${runtime_root}/crun" "${bin_dir}/readelf" "${bin_dir}/nm"

failure_count() {
  local output_file="$1"
  local result_line=""

  result_line="$(grep -E '^result ' "${output_file}")"
  if [[ "${result_line}" == *"supported" && "${result_line}" != *"unsupported"* ]]; then
    echo 0
    return 0
  fi
  printf '%s\n' "${result_line}" | sed -n 's/.*unsupported (\([0-9]*\) failing checks).*/\1/p'
}

# Run check-vmm-host against the fake runtime. The good tuple is the default.
# Arguments after the output path are KEY=VALUE overrides, then check flags.
run_check() {
  local output_file="$1"
  shift
  local -a overrides=(
    STUB_CRUN_VERSION=1.30.1-nimbus.9
    STUB_RUNPATH="${runtime_root}/lib"
    STUB_LIBKRUN_PATH="${runtime_root}/lib/libkrun.so.1"
    STUB_LIBKRUNFW_PATH="${runtime_root}/lib/libkrunfw.so.5"
    STUB_LIBKRUN_FILE=libkrun.so.1.19.6
  )
  local -a flags=()
  local arg=""
  local libkrun_file=""

  for arg in "$@"; do
    if [[ "${arg}" == *=* ]]; then
      overrides+=("${arg}")
    else
      flags+=("${arg}")
    fi
  done
  for arg in "${overrides[@]}"; do
    if [[ "${arg}" == STUB_LIBKRUN_FILE=* ]]; then
      libkrun_file="${arg#STUB_LIBKRUN_FILE=}"
    fi
  done
  ln -sfn "${libkrun_file}" "${runtime_root}/lib/libkrun.so.1"
  : > "${crun_log}"

  env -u EXPECTED_NIMBUS_CRUN_VERSION -u EXPECTED_NIMBUS_LIBKRUN_VERSION \
    -u EXPECTED_NIMBUS_LIBKRUN_UPSTREAM_VERSION -u EXPECTED_CRUN_RUNPATH \
    PATH="${bin_dir}:${PATH}" \
    TMPDIR="${probe_tmp}" \
    LD_LIBRARY_PATH="${tmp_dir}/must-not-reach-crun" \
    NIMBUS_LINUX_DISTRIBUTION_CONTRACT_ENV="${contract_env}" \
    NIMBUS_PRIVATE_RUNTIME_ROOT="${runtime_root}" \
    STUB_CRUN_LOG="${crun_log}" \
    "${overrides[@]}" \
    bash "${repo_root}/scripts/check-vmm-host.sh" ${flags[@]+"${flags[@]}"} > "${output_file}" 2>&1 || true
}

expect_line() {
  local output_file="$1"
  local expected="$2"

  if ! grep -F -- "${expected}" "${output_file}" >/dev/null; then
    echo "expected line not found: ${expected}" >&2
    cat "${output_file}" >&2
    exit 1
  fi
}

expect_delta() {
  local output_file="$1"
  local expected_delta="$2"
  local actual=""

  actual="$(failure_count "${output_file}")"
  if [[ "${actual}" != "$((baseline_failures + expected_delta))" ]]; then
    echo "expected ${expected_delta} more failing checks than the baseline ${baseline_failures}, got ${actual}" >&2
    cat "${output_file}" >&2
    exit 1
  fi
}

good_output="${tmp_dir}/good.txt"
run_check "${good_output}"
expect_line "${good_output}" "nimbus.crun.version    present expected=v1.30.1-nimbus.9"
expect_line "${good_output}" "nimbus.crun.runpath    present expected=${runtime_root}/lib"
expect_line "${good_output}" "nimbus.libkrun.loaded  present path=${runtime_root}/lib/libkrun.so.1 file=libkrun.so.1.19.6"
expect_line "${good_output}" "nimbus.libkrunfw.loaded present path=${runtime_root}/lib/libkrunfw.so.5"
grep -E '^--cgroup-manager=disabled --root .* run --bundle .* nimbus-check-vmm-host-[0-9]+$' "${crun_log}" >/dev/null
grep -E '^--cgroup-manager=disabled --root .* delete -f nimbus-check-vmm-host-[0-9]+$' "${crun_log}" >/dev/null
if compgen -G "${probe_tmp}/nimbus-check-vmm-host.*" >/dev/null; then
  echo "probe left its bundle directory in TMPDIR" >&2
  exit 1
fi
baseline_failures="$(failure_count "${good_output}")"

stale_output="${tmp_dir}/stale.txt"
run_check "${stale_output}" \
  STUB_LIBKRUN_PATH=/usr/local/lib64/libkrun.so.1 \
  STUB_LIBKRUNFW_PATH=/usr/local/lib64/libkrunfw.so.5
expect_line "${stale_output}" "nimbus.libkrun.loaded  mismatch path=/usr/local/lib64/libkrun.so.1 expected=${runtime_root}/lib/libkrun.so.1"
expect_line "${stale_output}" "nimbus.libkrunfw.loaded mismatch path=/usr/local/lib64/libkrunfw.so.5 expected=${runtime_root}/lib/libkrunfw.so.5"
expect_delta "${stale_output}" 2

pending_output="${tmp_dir}/pending.txt"
run_check "${pending_output}" \
  STUB_LIBKRUN_PATH=/usr/local/lib64/libkrun.so.1 \
  STUB_LIBKRUNFW_PATH=/usr/local/lib64/libkrunfw.so.5 \
  --allow-pending-private-runtime
expect_line "${pending_output}" "nimbus.libkrun.loaded  mismatch path=/usr/local/lib64/libkrun.so.1"
expect_delta "${pending_output}" 0

no_load_output="${tmp_dir}/no-load.txt"
run_check "${no_load_output}" STUB_LIBKRUN_PATH= STUB_LIBKRUNFW_PATH=
expect_line "${no_load_output}" "nimbus.libkrun.loaded  missing expected=${runtime_root}/lib/libkrun.so.1 (crun did not load libkrun.so.1: open \`rootfs-missing\`: No such file or directory)"
expect_line "${no_load_output}" "nimbus.libkrunfw.loaded missing expected=${runtime_root}/lib/libkrunfw.so.5"
expect_delta "${no_load_output}" 2

crun_version_output="${tmp_dir}/crun-version.txt"
run_check "${crun_version_output}" STUB_CRUN_VERSION=1.30.1-nimbus.1
expect_line "${crun_version_output}" "nimbus.crun.version    mismatch path=${runtime_root}/crun actual=crun version 1.30.1-nimbus.1 expected=v1.30.1-nimbus.9"
expect_delta "${crun_version_output}" 1

libkrun_version_output="${tmp_dir}/libkrun-version.txt"
run_check "${libkrun_version_output}" STUB_LIBKRUN_FILE=libkrun.so.1.17.4
expect_line "${libkrun_version_output}" "nimbus.libkrun.loaded  mismatch path=${runtime_root}/lib/libkrun.so.1 file=libkrun.so.1.17.4 expected_file=libkrun.so.1.19.6"
expect_delta "${libkrun_version_output}" 1

runpath_output="${tmp_dir}/runpath.txt"
# shellcheck disable=SC2016 # $ORIGIN is a literal ELF loader token.
run_check "${runpath_output}" 'STUB_RUNPATH=$ORIGIN/lib'
# shellcheck disable=SC2016 # $ORIGIN is a literal ELF loader token.
expect_line "${runpath_output}" 'nimbus.crun.runpath    mismatch actual=$ORIGIN/lib'
expect_delta "${runpath_output}" 1

echo "check-vmm-host helper verification passed"
