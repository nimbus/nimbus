#!/usr/bin/env bash

set -euo pipefail

fail() {
  printf '::error::%s\n' "$1" >&2
  exit 1
}

require_env() {
  local name="$1"
  if [[ -z "${!name:-}" ]]; then
    fail "${name} must be set by the prebuilt rusty_v8 consumption step"
  fi
}

if [[ "${V8_FROM_SOURCE+x}" == "x" ]]; then
  fail "V8_FROM_SOURCE must be unset when consuming prebuilt rusty_v8 artifacts"
fi

require_env RUSTY_V8_ARCHIVE
require_env RUSTY_V8_MIRROR
require_env RUSTY_V8_SRC_BINDING_PATH

[[ -d "${RUSTY_V8_MIRROR}" ]] || fail "RUSTY_V8_MIRROR is not a directory: ${RUSTY_V8_MIRROR}"
[[ -f "${RUSTY_V8_ARCHIVE}" ]] || fail "RUSTY_V8_ARCHIVE is not a file: ${RUSTY_V8_ARCHIVE}"
[[ -s "${RUSTY_V8_ARCHIVE}" ]] || fail "RUSTY_V8_ARCHIVE is empty: ${RUSTY_V8_ARCHIVE}"
[[ -f "${RUSTY_V8_SRC_BINDING_PATH}" ]] || fail "RUSTY_V8_SRC_BINDING_PATH is not a file: ${RUSTY_V8_SRC_BINDING_PATH}"
[[ -s "${RUSTY_V8_SRC_BINDING_PATH}" ]] || fail "RUSTY_V8_SRC_BINDING_PATH is empty: ${RUSTY_V8_SRC_BINDING_PATH}"

# Expected variant is a required argument: archive AND binding must belong to
# the SAME variant, because rusty_v8's build.rs does not validate an explicit
# RUSTY_V8_SRC_BINDING_PATH against the enabled feature set — a mismatched
# pairing would compile with wrong bindings silently.
variant="${1:-}"
case "${variant}" in
  release|ptrcomp)
    ;;
  *)
    fail "usage: $0 <release|ptrcomp> — expected variant argument required"
    ;;
esac

archive_base="$(basename -- "${RUSTY_V8_ARCHIVE}")"
binding_base="$(basename -- "${RUSTY_V8_SRC_BINDING_PATH}")"
if [[ "${variant}" == "release" ]]; then
  case "${archive_base}" in
    librusty_v8_ptrcomp_simdutf_release_*.a)
      fail "variant=release but RUSTY_V8_ARCHIVE is the ptrcomp artifact: ${archive_base}" ;;
    librusty_v8_simdutf_release_*.a) ;;
    *) fail "RUSTY_V8_ARCHIVE does not look like a published release artifact: ${archive_base}" ;;
  esac
  case "${binding_base}" in
    src_binding_simdutf_release_*.rs) ;;
    *) fail "variant=release but binding is not the release binding: ${binding_base}" ;;
  esac
else
  case "${archive_base}" in
    librusty_v8_ptrcomp_simdutf_release_*.a) ;;
    *) fail "variant=ptrcomp but RUSTY_V8_ARCHIVE is not the ptrcomp artifact: ${archive_base}" ;;
  esac
  case "${binding_base}" in
    src_binding_ptrcomp_simdutf_release_*.rs) ;;
    *) fail "variant=ptrcomp but binding is not the ptrcomp binding: ${binding_base}" ;;
  esac
fi



# Target-suffix equality (review LOW): archive and binding must be for the
# SAME target triple, not merely the same variant.
archive_target="${archive_base##*release_}"; archive_target="${archive_target%.a}"
binding_target="${binding_base##*release_}"; binding_target="${binding_target%.rs}"
if [[ "${archive_target}" != "${binding_target}" ]]; then
  fail "archive target ${archive_target} != binding target ${binding_target}"
fi

printf 'prebuilt rusty_v8 env is valid\n'
