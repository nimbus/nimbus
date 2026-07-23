#!/usr/bin/env bash

set -euo pipefail

readonly ELLE_CLI_VERSION="0.1.9"
readonly ELLE_CORE_VERSION="0.2.4"
readonly ELLE_CLI_ARCHIVE="elle-cli-bin-${ELLE_CLI_VERSION}.zip"
readonly ELLE_CLI_URL="https://github.com/ligurio/elle-cli/releases/download/${ELLE_CLI_VERSION}/${ELLE_CLI_ARCHIVE}"
readonly ELLE_CLI_ARCHIVE_SHA256="7bb21b1c68580cd63816abee7655c68023b837bcca91eac9025674e4fe1ff12c"
readonly ELLE_CLI_JAR_RELATIVE="target/elle-cli-${ELLE_CLI_VERSION}-standalone.jar"
readonly ELLE_CLI_JAR_SHA256="c9ba9b9fd32640e73d632cb5f15069c162ba6528a67f27a878767187c59f539a"
readonly ELLE_TEST_FILTER='package(nimbus-engine) and test(/^engine::execution_units::tests::elle::elle_serializable_check_passes$/)'
readonly UNVERIFIED_EXIT=69

die() {
  printf 'Elle proof error: %s\n' "$*" >&2
  exit 1
}

unverified() {
  printf 'UNVERIFIED: %s\n' "$*" >&2
  exit "${UNVERIFIED_EXIT}"
}

require_command() {
  local command_name="$1"
  local purpose="$2"
  command -v "${command_name}" >/dev/null 2>&1 \
    || unverified "${purpose} requires executable ${command_name}"
}

if [[ -n "${NIMBUS_ELLE_SHA256_BIN:-}" ]]; then
  require_command "${NIMBUS_ELLE_SHA256_BIN}" "Elle checksum verification"
  sha256_command=("${NIMBUS_ELLE_SHA256_BIN}")
elif command -v sha256sum >/dev/null 2>&1; then
  sha256_command=(sha256sum)
elif command -v shasum >/dev/null 2>&1; then
  sha256_command=(shasum -a 256)
else
  unverified "Elle checksum verification requires sha256sum or shasum"
fi

sha256_file() {
  local path="$1"
  local output
  output="$("${sha256_command[@]}" "${path}")" \
    || die "checksum command failed for ${path}"
  read -r digest _ <<<"${output}"
  [[ "${digest}" =~ ^[0-9a-fA-F]{64}$ ]] \
    || die "checksum command returned an invalid digest for ${path}: ${digest}"
  printf '%s\n' "${digest}" | tr '[:upper:]' '[:lower:]'
}

verify_sha256() {
  local path="$1"
  local expected="$2"
  local label="$3"
  local actual
  actual="$(sha256_file "${path}")"
  [[ "${actual}" == "${expected}" ]] \
    || die "${label} checksum mismatch for ${path}: expected ${expected}, got ${actual}"
}

cache_root="${NIMBUS_ELLE_CACHE_DIR:-${XDG_CACHE_HOME:-${HOME}/.cache}/nimbus-test-tools/elle-cli/${ELLE_CLI_VERSION}}"
archive_path="${cache_root}/${ELLE_CLI_ARCHIVE}"
cached_jar="${cache_root}/${ELLE_CLI_JAR_RELATIVE}"

download_archive() {
  local curl_bin="${NIMBUS_ELLE_CURL_BIN:-curl}"
  local temporary
  local status
  require_command "${curl_bin}" "downloading pinned Elle CLI ${ELLE_CLI_VERSION}"
  mkdir -p "${cache_root}"
  temporary="$(mktemp "${archive_path}.download.XXXXXX")"
  set +e
  "${curl_bin}" \
    --fail \
    --location \
    --silent \
    --show-error \
    --connect-timeout 15 \
    --max-time 180 \
    --retry 2 \
    --retry-all-errors \
    --output "${temporary}" \
    "${ELLE_CLI_URL}"
  status=$?
  set -e
  if [[ "${status}" -ne 0 ]]; then
    rm -f "${temporary}"
    unverified "pinned Elle CLI ${ELLE_CLI_VERSION} download failed with exit ${status}: ${ELLE_CLI_URL}"
  fi
  verify_sha256 "${temporary}" "${ELLE_CLI_ARCHIVE_SHA256}" "Elle release archive"
  mv "${temporary}" "${archive_path}"
}

prepare_cached_jar() {
  local unzip_bin="${NIMBUS_ELLE_UNZIP_BIN:-unzip}"
  local extract_dir

  if [[ ! -f "${archive_path}" ]] \
    || [[ "$(sha256_file "${archive_path}")" != "${ELLE_CLI_ARCHIVE_SHA256}" ]]; then
    rm -f "${archive_path}"
    download_archive
  fi
  verify_sha256 "${archive_path}" "${ELLE_CLI_ARCHIVE_SHA256}" "Elle release archive"

  if [[ -f "${cached_jar}" ]] \
    && [[ "$(sha256_file "${cached_jar}")" == "${ELLE_CLI_JAR_SHA256}" ]]; then
    printf '%s\n' "${cached_jar}"
    return
  fi

  require_command "${unzip_bin}" "extracting pinned Elle CLI ${ELLE_CLI_VERSION}"
  rm -f "${cached_jar}"
  extract_dir="$(mktemp -d "${cache_root}/extract.XXXXXX")"
  if ! "${unzip_bin}" -q "${archive_path}" "${ELLE_CLI_JAR_RELATIVE}" -d "${extract_dir}"; then
    rm -rf "${extract_dir}"
    die "failed to extract ${ELLE_CLI_JAR_RELATIVE} from checksum-verified ${archive_path}"
  fi
  verify_sha256 \
    "${extract_dir}/${ELLE_CLI_JAR_RELATIVE}" \
    "${ELLE_CLI_JAR_SHA256}" \
    "Elle standalone jar"
  mkdir -p "$(dirname "${cached_jar}")"
  mv "${extract_dir}/${ELLE_CLI_JAR_RELATIVE}" "${cached_jar}"
  rm -rf "${extract_dir}"
  verify_sha256 "${cached_jar}" "${ELLE_CLI_JAR_SHA256}" "Elle standalone jar"
  printf '%s\n' "${cached_jar}"
}

if [[ -n "${NIMBUS_ELLE_CLI_JAR:-}" ]]; then
  elle_jar="${NIMBUS_ELLE_CLI_JAR}"
  [[ -f "${elle_jar}" ]] || die "explicit NIMBUS_ELLE_CLI_JAR does not exist: ${elle_jar}"
  verify_sha256 "${elle_jar}" "${ELLE_CLI_JAR_SHA256}" "Elle standalone jar"
else
  elle_jar="$(prepare_cached_jar)"
fi

java_bin="${NIMBUS_ELLE_JAVA_BIN:-java}"
require_command "${java_bin}" "pinned Elle CLI execution"
if ! "${java_bin}" -version >/dev/null 2>&1; then
  unverified "Java runtime ${java_bin} is present but could not start"
fi

printf 'Elle CLI version: %s\n' "${ELLE_CLI_VERSION}"
printf 'Embedded Elle core version: %s\n' "${ELLE_CORE_VERSION}"
printf 'Elle release archive SHA-256: %s\n' "${ELLE_CLI_ARCHIVE_SHA256}"
printf 'Elle standalone jar SHA-256: %s\n' "${ELLE_CLI_JAR_SHA256}"
printf 'Elle standalone jar: %s\n' "${elle_jar}"

if [[ -n "${NIMBUS_ELLE_ARCHIVE_FILE:-}" ]]; then
  nextest_bin="${NIMBUS_CARGO_NEXTEST_BIN:-cargo-nextest}"
  require_command "${nextest_bin}" "archived Elle proof execution"
  exec env \
    NIMBUS_ELLE_CLI_JAR="${elle_jar}" \
    NIMBUS_ELLE_JAVA_BIN="${java_bin}" \
    "${nextest_bin}" nextest run \
      --archive-file "${NIMBUS_ELLE_ARCHIVE_FILE}" \
      --workspace-remap "${GITHUB_WORKSPACE:-${PWD}}" \
      --package nimbus-engine \
      --profile ci-pr \
      --run-ignored only \
      --no-tests fail \
      -E "${ELLE_TEST_FILTER}"
fi

cargo_bin="${NIMBUS_ELLE_CARGO_BIN:-cargo}"
require_command "${cargo_bin}" "local Elle proof execution"
exec env \
  NIMBUS_ELLE_CLI_JAR="${elle_jar}" \
  NIMBUS_ELLE_JAVA_BIN="${java_bin}" \
  "${cargo_bin}" nextest run \
    --package nimbus-engine \
    --profile ci-pr \
    --run-ignored only \
    --no-tests fail \
    -E "${ELLE_TEST_FILTER}"
