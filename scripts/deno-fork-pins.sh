#!/usr/bin/env bash
# Canonical extraction helpers for the Deno/rusty_v8 pins consumed by Nimbus.
# Source this file; do not duplicate release tags or peeled SHAs in verifiers.
# shellcheck disable=SC2034 # The loader intentionally populates caller globals.

deno_fork_patch_field() {
  local crate="$1"
  local field="$2"
  awk -v crate="${crate}" -v field="${field}" '
    $0 ~ "^" crate " = " {
      marker = field " = \""
      start = index($0, marker)
      if (start == 0) {
        next
      }
      value = substr($0, start + length(marker))
      sub(/\".*/, "", value)
      print value
      found = 1
      exit
    }
    END {
      if (!found) {
        exit 1
      }
    }
  ' Cargo.toml
}

deno_fork_lock_field() {
  local crate="$1"
  local field="$2"
  awk -v crate="${crate}" -v field="${field}" '
    function emit() {
      if (!found && name == crate) {
        if (field == "version") {
          print version
        } else if (field == "source") {
          print source
        }
        found = 1
        exit
      }
    }
    $0 == "[[package]]" {
      emit()
      name = ""
      version = ""
      source = ""
      next
    }
    /^name = / {
      name = $3
      gsub(/"/, "", name)
      next
    }
    /^version = / {
      version = $3
      gsub(/"/, "", version)
      next
    }
    /^source = / {
      source = $0
      sub(/^source = "/, "", source)
      sub(/"$/, "", source)
      next
    }
    END {
      emit()
      if (!found) {
        exit 1
      }
    }
  ' Cargo.lock
}

deno_fork_source_repo() {
  local source="$1"
  source="${source#git+}"
  printf '%s\n' "${source%%\?*}"
}

deno_fork_source_tag() {
  local source="$1"
  source="${source#*\?tag=}"
  printf '%s\n' "${source%%#*}"
}

deno_fork_source_sha() {
  local source="$1"
  printf '%s\n' "${source##*#}"
}

deno_fork_load_consumed_pins() {
  DENO_FORK_PATCH_TAG="$(deno_fork_patch_field deno_core tag)"
  DENO_FORK_LOCK_SOURCE="$(deno_fork_lock_field deno_core source)"
  DENO_FORK_REPO="$(deno_fork_source_repo "${DENO_FORK_LOCK_SOURCE}")"
  DENO_FORK_LOCK_TAG="$(deno_fork_source_tag "${DENO_FORK_LOCK_SOURCE}")"
  DENO_FORK_SHA="$(deno_fork_source_sha "${DENO_FORK_LOCK_SOURCE}")"

  RUSTY_V8_PATCH_TAG="$(deno_fork_patch_field v8 tag)"
  RUSTY_V8_LOCK_SOURCE="$(deno_fork_lock_field v8 source)"
  RUSTY_V8_REPO="$(deno_fork_source_repo "${RUSTY_V8_LOCK_SOURCE}")"
  RUSTY_V8_LOCK_TAG="$(deno_fork_source_tag "${RUSTY_V8_LOCK_SOURCE}")"
  RUSTY_V8_SHA="$(deno_fork_source_sha "${RUSTY_V8_LOCK_SOURCE}")"
  RUSTY_V8_VERSION="$(deno_fork_lock_field v8 version)"
}
