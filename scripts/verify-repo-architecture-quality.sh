#!/usr/bin/env bash
# Verifies the repository architecture-quality baseline and guardrails.

set -uo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
LEDGER="${REPO_ROOT}/docs/private/architecture/repo-architecture-quality-ledger.tsv"

issue_count=0
exclusion_patterns=()
large_file_paths=()
naming_exception_paths=()

record_issue() {
  printf 'repo-architecture-quality: %s\n' "$1" >&2
  issue_count=$((issue_count + 1))
}

load_ledger() {
  local kind
  local value
  local rest

  exclusion_patterns=()
  large_file_paths=()
  naming_exception_paths=()

  while IFS=$'\t' read -r kind value rest; do
    [[ -z "${kind}" || "${kind}" == \#* || "${kind}" == "kind" ]] && continue
    case "${kind}" in
      exclusion) exclusion_patterns+=("${value}") ;;
      large_file) large_file_paths+=("${value}") ;;
      naming_exception) naming_exception_paths+=("${value}") ;;
    esac
  done < "${LEDGER}"
}

ledger_values() {
  local kind="$1"
  local value
  local values=()

  case "${kind}" in
    exclusion) values=("${exclusion_patterns[@]}") ;;
    large_file) values=("${large_file_paths[@]}") ;;
    naming_exception) values=("${naming_exception_paths[@]}") ;;
    *) return 0 ;;
  esac

  for value in "${values[@]}"; do
    printf '%s\n' "${value}"
  done
}

ledger_has_value() {
  local kind="$1"
  local needle="$2"
  local value
  local values=()

  case "${kind}" in
    exclusion) values=("${exclusion_patterns[@]}") ;;
    large_file) values=("${large_file_paths[@]}") ;;
    naming_exception) values=("${naming_exception_paths[@]}") ;;
    *) return 1 ;;
  esac

  for value in "${values[@]}"; do
    [[ "${value}" == "${needle}" ]] && return 0
  done
  return 1
}

is_source_file() {
  case "$1" in
    *.rs|*.ts|*.tsx|*.js|*.jsx|*.mjs|*.cjs) return 0 ;;
    *) return 1 ;;
  esac
}

is_excluded() {
  local path="$1"
  local pattern

  for pattern in "${exclusion_patterns[@]}"; do
    [[ -z "${pattern}" ]] && continue
    case "${path}" in
      ${pattern}) return 0 ;;
    esac
  done

  return 1
}

line_count() {
  wc -l < "$1" | tr -d ' '
}

source_files() {
  find "${REPO_ROOT}/crates" "${REPO_ROOT}/packages" "${REPO_ROOT}/demos" \
    \( \
      -path '*/node_modules' -o \
      -path '*/target' -o \
      -path '*/dist' -o \
      -path '*/storybook-static' -o \
      -path '*/.nimbus' -o \
      -path '*/_generated' -o \
      -path '*/src/gen' -o \
      -path "${REPO_ROOT}/crates/nimbus-runtime/src/runtime/tests/node_compat_fixtures" -o \
      -path "${REPO_ROOT}/demos/convex/vendor" \
    \) -prune -o -type f -print
}

check_large_files() {
  local file
  local rel
  local lines
  local threshold_label

  printf '[1/4] owned-source size ledger\n'

  while IFS= read -r file; do
    rel="${file#${REPO_ROOT}/}"
    is_source_file "${rel}" || continue
    is_excluded "${rel}" && continue

    lines="$(line_count "${file}")"
    if [[ "${lines}" -lt 1500 ]]; then
      continue
    fi

    threshold_label="review"
    if [[ "${lines}" -ge 2000 ]]; then
      threshold_label="hard"
    fi

    printf '%s\t%s\t%s\n' "${threshold_label}" "${lines}" "${rel}"
    if ! ledger_has_value "large_file" "${rel}"; then
      record_issue "untracked large owned-source file (${lines} lines): ${rel}"
    fi
  done < <(source_files) || true

  while IFS= read -r rel; do
    [[ -z "${rel}" ]] && continue
    if [[ ! -f "${REPO_ROOT}/${rel}" ]]; then
      record_issue "large-file ledger path no longer exists: ${rel}"
    fi
  done < <(ledger_values "large_file") || true
}

check_naming_exceptions() {
  local file
  local rel
  local base

  printf '\n[2/4] helper/common naming ledger\n'

  while IFS= read -r file; do
    rel="${file#${REPO_ROOT}/}"
    is_source_file "${rel}" || continue
    is_excluded "${rel}" && continue
    base="$(basename "${rel}")"

    case "${base}" in
      helper.*|helpers.*|*helper*.rs|*helper*.ts|*helper*.tsx|*helper*.js|*helper*.jsx|*helper*.mjs|*helper*.cjs|common.rs|common.ts|common.tsx|common.js|common.jsx|common.mjs|common.cjs)
        printf '%s\n' "${rel}"
        if ! ledger_has_value "naming_exception" "${rel}"; then
          record_issue "untracked helper/common naming exception: ${rel}"
        fi
        ;;
    esac
  done < <(source_files) || true

  while IFS= read -r rel; do
    [[ -z "${rel}" ]] && continue
    if [[ ! -f "${REPO_ROOT}/${rel}" ]]; then
      record_issue "naming-exception ledger path no longer exists: ${rel}"
    fi
  done < <(ledger_values "naming_exception") || true
}

check_core_no_io() {
  printf '\n[3/4] nimbus-core zero-I/O invariant\n'

  local forbidden='(\bstd::fs\b|\bstd::net\b|\bstd::process\b|\btokio::|\breqwest\b|\bhyper\b|\baxum\b|\brusqlite\b|\bsqlx\b|\bmysql\b|\bpostgres\b|\bredb\b)'
  if rg -n "${forbidden}" "${REPO_ROOT}/crates/nimbus-core/src" "${REPO_ROOT}/crates/nimbus-core/Cargo.toml"; then
    record_issue "nimbus-core contains forbidden I/O import or dependency"
  else
    printf 'nimbus-core zero-I/O import scan: pass\n'
  fi
}

check_runtime_no_workspace_deps() {
  printf '\n[4/4] nimbus-runtime zero-workspace-dependency invariant\n'

  local cargo_toml="${REPO_ROOT}/crates/nimbus-runtime/Cargo.toml"
  if rg -n 'path\s*=\s*"\.\./|^nimbus-[A-Za-z0-9_-]+\s*=' "${cargo_toml}"; then
    record_issue "nimbus-runtime declares a workspace/local Nimbus dependency"
  else
    printf 'nimbus-runtime workspace dependency scan: pass\n'
  fi
}

if [[ ! -f "${LEDGER}" ]]; then
  record_issue "missing ledger: ${LEDGER#${REPO_ROOT}/}"
else
  load_ledger
  printf 'repo architecture quality gate\n'
  printf 'Repo: %s\n' "${REPO_ROOT}"
  printf 'Ledger: %s\n\n' "${LEDGER#${REPO_ROOT}/}"

  check_large_files
  check_naming_exceptions
  check_core_no_io
  check_runtime_no_workspace_deps
fi

if [[ "${issue_count}" -ne 0 ]]; then
  printf '\nrepo-architecture-quality: %s issue(s) detected\n' "${issue_count}" >&2
  exit 1
fi

printf '\nrepo-architecture-quality: pass\n'
