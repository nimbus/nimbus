#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
proof_script="${repo_root}/scripts/verify-elle-serializability.sh"
tmp_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-elle-proof-helper.XXXXXX")"
trap 'rm -rf "${tmp_dir}"' EXIT

readonly archive_sha="7bb21b1c68580cd63816abee7655c68023b837bcca91eac9025674e4fe1ff12c"
readonly jar_sha="c9ba9b9fd32640e73d632cb5f15069c162ba6528a67f27a878767187c59f539a"
command_log="${tmp_dir}/commands.log"

fail() {
  printf 'Elle proof helper failed: %s\n' "$*" >&2
  exit 1
}

assert_contains() {
  local path="$1"
  local expected="$2"
  grep -F -- "${expected}" "${path}" >/dev/null \
    || fail "${path} did not contain: ${expected}"
}

assert_not_contains() {
  local path="$1"
  local unexpected="$2"
  if grep -F -- "${unexpected}" "${path}" >/dev/null 2>&1; then
    fail "${path} unexpectedly contained: ${unexpected}"
  fi
}

fake_sha="${tmp_dir}/sha256"
fake_curl="${tmp_dir}/curl"
fake_unzip="${tmp_dir}/unzip"
fake_java="${tmp_dir}/java"
fake_cargo="${tmp_dir}/cargo"

cat >"${fake_sha}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_ELLE_COMMAND_LOG:?}"
path="${1:?}"
printf 'sha %s\n' "${path}" >>"${FAKE_ELLE_COMMAND_LOG}"
case "${path}" in
  *.zip|*.zip.download.*)
    printf '%s  %s\n' \
      7bb21b1c68580cd63816abee7655c68023b837bcca91eac9025674e4fe1ff12c \
      "${path}"
    ;;
  *.jar)
    if [[ "${FAKE_ELLE_BAD_JAR_HASH:-0}" == "1" ]]; then
      printf '%s  %s\n' \
        0000000000000000000000000000000000000000000000000000000000000000 \
        "${path}"
    else
      printf '%s  %s\n' \
        c9ba9b9fd32640e73d632cb5f15069c162ba6528a67f27a878767187c59f539a \
        "${path}"
    fi
    ;;
  *)
    exit 64
    ;;
esac
EOF

cat >"${fake_curl}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_ELLE_COMMAND_LOG:?}"
printf 'curl' >>"${FAKE_ELLE_COMMAND_LOG}"
printf ' %q' "$@" >>"${FAKE_ELLE_COMMAND_LOG}"
printf '\n' >>"${FAKE_ELLE_COMMAND_LOG}"
[[ "${FAKE_ELLE_CURL_EXIT:-0}" == "0" ]] || exit "${FAKE_ELLE_CURL_EXIT}"
output=""
while [[ "$#" -gt 0 ]]; do
  if [[ "$1" == "--output" ]]; then
    output="${2:?}"
    break
  fi
  shift
done
[[ -n "${output}" ]] || exit 65
printf 'fake archive\n' >"${output}"
EOF

cat >"${fake_unzip}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_ELLE_COMMAND_LOG:?}"
printf 'unzip' >>"${FAKE_ELLE_COMMAND_LOG}"
printf ' %q' "$@" >>"${FAKE_ELLE_COMMAND_LOG}"
printf '\n' >>"${FAKE_ELLE_COMMAND_LOG}"
destination=""
entry=""
while [[ "$#" -gt 0 ]]; do
  case "$1" in
    -q)
      shift
      ;;
    -d)
      destination="${2:?}"
      shift 2
      ;;
    *.jar)
      entry="$1"
      shift
      ;;
    *)
      shift
      ;;
  esac
done
[[ -n "${destination}" && -n "${entry}" ]] || exit 66
mkdir -p "${destination}/$(dirname "${entry}")"
printf 'fake jar\n' >"${destination}/${entry}"
EOF

cat >"${fake_java}" <<'EOF'
#!/usr/bin/env bash
exit "${FAKE_ELLE_JAVA_EXIT:-0}"
EOF

cat >"${fake_cargo}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
: "${FAKE_ELLE_COMMAND_LOG:?}"
printf 'cargo jar=%s java=%s' \
  "${NIMBUS_ELLE_CLI_JAR:-}" \
  "${NIMBUS_ELLE_JAVA_BIN:-}" >>"${FAKE_ELLE_COMMAND_LOG}"
printf ' %q' "$@" >>"${FAKE_ELLE_COMMAND_LOG}"
printf '\n' >>"${FAKE_ELLE_COMMAND_LOG}"
exit "${FAKE_ELLE_CARGO_EXIT:-0}"
EOF

chmod +x "${fake_sha}" "${fake_curl}" "${fake_unzip}" "${fake_java}" "${fake_cargo}"

base_env=(
  "NIMBUS_ELLE_SHA256_BIN=${fake_sha}"
  "NIMBUS_ELLE_CURL_BIN=${fake_curl}"
  "NIMBUS_ELLE_UNZIP_BIN=${fake_unzip}"
  "NIMBUS_ELLE_JAVA_BIN=${fake_java}"
  "NIMBUS_ELLE_CARGO_BIN=${fake_cargo}"
  "FAKE_ELLE_COMMAND_LOG=${command_log}"
)

run_case() {
  local expected_status="$1"
  local output="$2"
  shift 2
  local status
  set +e
  env "${base_env[@]}" "$@" bash "${proof_script}" >"${output}" 2>&1
  status=$?
  set -e
  [[ "${status}" -eq "${expected_status}" ]] \
    || fail "expected exit ${expected_status}, got ${status}; output: $(cat "${output}")"
}

explicit_jar="${tmp_dir}/explicit.jar"
printf 'fake jar\n' >"${explicit_jar}"
: >"${command_log}"
run_case 0 "${tmp_dir}/explicit.out" "NIMBUS_ELLE_CLI_JAR=${explicit_jar}"
assert_contains "${tmp_dir}/explicit.out" "Elle CLI version: 0.1.9"
assert_contains "${tmp_dir}/explicit.out" "Embedded Elle core version: 0.2.4"
assert_contains "${command_log}" "sha ${explicit_jar}"
assert_contains "${command_log}" "cargo jar=${explicit_jar} java=${fake_java} nextest run"
assert_contains "${command_log}" "--run-ignored only --no-tests fail"
assert_contains "${command_log}" "engine::execution_units::tests::elle::elle_serializable_check_passes"

cache_dir="${tmp_dir}/cache"
: >"${command_log}"
run_case 0 "${tmp_dir}/download.out" "NIMBUS_ELLE_CACHE_DIR=${cache_dir}"
assert_contains "${command_log}" "curl --fail --location --silent --show-error --connect-timeout 15 --max-time 180 --retry 2 --retry-all-errors"
assert_contains "${command_log}" "https://github.com/ligurio/elle-cli/releases/download/0.1.9/elle-cli-bin-0.1.9.zip"
assert_contains "${command_log}" "unzip -q"
assert_contains "${command_log}" "sha ${cache_dir}/elle-cli-bin-0.1.9.zip"
assert_contains "${command_log}" "sha ${cache_dir}/target/elle-cli-0.1.9-standalone.jar"

: >"${command_log}"
run_case 0 "${tmp_dir}/reuse.out" "NIMBUS_ELLE_CACHE_DIR=${cache_dir}"
assert_not_contains "${command_log}" "curl "
assert_not_contains "${command_log}" "unzip "
assert_contains "${command_log}" "cargo jar=${cache_dir}/target/elle-cli-0.1.9-standalone.jar"

archive_file="${tmp_dir}/nimbus-tests.tar.zst"
printf 'fake nextest archive\n' >"${archive_file}"
: >"${command_log}"
run_case 0 \
  "${tmp_dir}/archive-adapter.out" \
  "NIMBUS_ELLE_CLI_JAR=${explicit_jar}" \
  "NIMBUS_ELLE_ARCHIVE_FILE=${archive_file}" \
  "NIMBUS_CARGO_NEXTEST_BIN=${fake_cargo}" \
  "GITHUB_WORKSPACE=/tmp/nimbus-workspace"
assert_contains "${command_log}" "nextest run --archive-file ${archive_file}"
assert_contains "${command_log}" "--workspace-remap /tmp/nimbus-workspace --package nimbus-engine"

: >"${command_log}"
run_case 37 \
  "${tmp_dir}/test-failure.out" \
  "NIMBUS_ELLE_CLI_JAR=${explicit_jar}" \
  "FAKE_ELLE_CARGO_EXIT=37"

run_case 69 \
  "${tmp_dir}/missing-java.out" \
  "NIMBUS_ELLE_CLI_JAR=${explicit_jar}" \
  "NIMBUS_ELLE_JAVA_BIN=${tmp_dir}/missing-java"
assert_contains "${tmp_dir}/missing-java.out" "UNVERIFIED:"

run_case 69 \
  "${tmp_dir}/download-failure.out" \
  "NIMBUS_ELLE_CACHE_DIR=${tmp_dir}/failed-download-cache" \
  "FAKE_ELLE_CURL_EXIT=28"
assert_contains "${tmp_dir}/download-failure.out" "UNVERIFIED:"
assert_contains "${tmp_dir}/download-failure.out" "download failed with exit 28"

run_case 1 \
  "${tmp_dir}/integrity-failure.out" \
  "NIMBUS_ELLE_CLI_JAR=${explicit_jar}" \
  "FAKE_ELLE_BAD_JAR_HASH=1"
assert_contains "${tmp_dir}/integrity-failure.out" "checksum mismatch"

printf 'Elle proof helper: selection/checksums/download bounds/UNVERIFIED/exit propagation passed\n'
