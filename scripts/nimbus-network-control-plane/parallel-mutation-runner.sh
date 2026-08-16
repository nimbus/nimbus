#!/usr/bin/env bash
# Bounded, deterministic process runner for independent verifier mutations.
# This file is sourced by concept-owned mutation suites.
# shellcheck shell=bash

mutation_worker_count() {
  requested="${NIMBUS_NETWORK_MUTATION_JOBS:-}"
  if [ -n "${requested}" ]; then
    case "${requested}" in
      *[!0-9]*)
        printf 'invalid NIMBUS_NETWORK_MUTATION_JOBS: %s\n' "${requested}" >&2
        return 2
        ;;
    esac
    normalized="$(printf '%s\n' "${requested}" | sed 's/^0*//')"
    if [ -z "${normalized}" ] || [ "${#normalized}" -gt 2 ] ||
      [ "${normalized}" -gt 64 ]; then
      printf 'invalid NIMBUS_NETWORK_MUTATION_JOBS: %s\n' "${requested}" >&2
      return 2
    fi
    printf '%s\n' "${normalized}"
    return 0
  fi

  detected="$(getconf _NPROCESSORS_ONLN 2>/dev/null || true)"
  case "${detected}" in
    '' | *[!0-9]* | 0)
      detected="$(sysctl -n hw.ncpu 2>/dev/null || true)"
      ;;
  esac
  case "${detected}" in
    '' | *[!0-9]* | 0) detected=2 ;;
  esac
  if [ "${detected}" -gt 8 ]; then
    detected=8
  fi
  printf '%s\n' "${detected}"
}

collect_mutation_batch() {
  output_root="$1"
  prefix="$2"
  first="$3"
  last="$4"

  index="${first}"
  while [ "${index}" -le "${last}" ]; do
    output_path="${output_root}/${prefix}-${index}.out"
    status_path="${output_root}/${prefix}-${index}.status"
    if [ -f "${output_path}" ]; then
      cat "${output_path}"
    fi
    if [ ! -f "${status_path}" ]; then
      printf 'SELFTEST FAIL %s mutation worker %d did not record a result\n' \
        "${prefix}" "${index}"
      PARALLEL_MUTATION_FAILED=$((PARALLEL_MUTATION_FAILED + 1))
      index=$((index + 1))
      continue
    fi

    status="$(cat "${status_path}")"
    case "${status}" in
      0) PARALLEL_MUTATION_PASSED=$((PARALLEL_MUTATION_PASSED + 1)) ;;
      1) PARALLEL_MUTATION_FAILED=$((PARALLEL_MUTATION_FAILED + 1)) ;;
      *)
        printf 'SELFTEST FAIL %s mutation worker %d recorded invalid status %s\n' \
          "${prefix}" "${index}" "${status}"
        PARALLEL_MUTATION_FAILED=$((PARALLEL_MUTATION_FAILED + 1))
        ;;
    esac
    index=$((index + 1))
  done
}

mutation_pid_is_owned() {
  candidate_pid="$1"
  for owned_pid in ${parallel_mutation_owned_pids}; do
    if [ "${owned_pid}" = "${candidate_pid}" ]; then
      return 0
    fi
  done
  return 1
}

terminate_mutation_batch() {
  parallel_mutation_owned_pids=""
  for worker_pid in ${batch_pids}; do
    if kill -0 "${worker_pid}" 2>/dev/null; then
      parallel_mutation_owned_pids="${parallel_mutation_owned_pids}${parallel_mutation_owned_pids:+ }${worker_pid}"
      kill -STOP "${worker_pid}" 2>/dev/null || true
    fi
  done

  discovered=1
  while [ "${discovered}" -ne 0 ]; do
    discovered=0
    for owned_pid in ${parallel_mutation_owned_pids}; do
      children="$(pgrep -P "${owned_pid}" 2>/dev/null || true)"
      for child_id in ${children}; do
        if ! mutation_pid_is_owned "${child_id}"; then
          parallel_mutation_owned_pids="${parallel_mutation_owned_pids}${parallel_mutation_owned_pids:+ }${child_id}"
          kill -STOP "${child_id}" 2>/dev/null || true
          discovered=1
        fi
      done
    done
  done
  for owned_pid in ${parallel_mutation_owned_pids}; do
    if kill -0 "${owned_pid}" 2>/dev/null; then
      kill -KILL "${owned_pid}" 2>/dev/null || true
    fi
  done
  for worker_pid in ${batch_pids}; do
    wait "${worker_pid}" 2>/dev/null || true
  done
  batch_pids=""
  active=0
}

restore_mutation_trap() {
  saved_trap_path="$1"
  signal_name="$2"
  if [ -s "${saved_trap_path}" ]; then
    saved_trap="$(<"${saved_trap_path}")"
    eval "${saved_trap}"
  else
    trap - "${signal_name}"
  fi
  rm -f "${saved_trap_path}"
}

wait_for_mutation_batch() {
  for worker_pid in ${batch_pids}; do
    wait "${worker_pid}" || true
    if [ -n "${parallel_mutation_signal}" ]; then
      return
    fi
  done
}

run_parallel_mutation_cases() {
  if [ "$#" -lt 4 ]; then
    printf 'run_parallel_mutation_cases requires output-root, prefix, worker, and cases\n' >&2
    return 2
  fi

  output_root="$1"
  prefix="$2"
  worker="$3"
  shift 3

  PARALLEL_MUTATION_PASSED=0
  PARALLEL_MUTATION_FAILED=0
  if ! command -v pgrep >/dev/null 2>&1; then
    printf 'parallel mutation runner requires pgrep for owned-process cleanup\n' >&2
    return 2
  fi
  worker_count="$(mutation_worker_count)" || return $?
  if ! mkdir -p "${output_root}"; then
    printf 'unable to create parallel mutation output root: %s\n' "${output_root}" >&2
    return 2
  fi

  saved_int_trap="${output_root}/.${prefix}-saved-int-trap"
  saved_term_trap="${output_root}/.${prefix}-saved-term-trap"
  saved_hup_trap="${output_root}/.${prefix}-saved-hup-trap"
  trap -p INT >"${saved_int_trap}"
  trap -p TERM >"${saved_term_trap}"
  trap -p HUP >"${saved_hup_trap}"
  parallel_mutation_signal=""
  batch_pids=""
  trap 'parallel_mutation_signal=INT' INT
  trap 'parallel_mutation_signal=TERM' TERM
  trap 'parallel_mutation_signal=HUP' HUP

  index=0
  batch_first=1
  active=0
  for mutation_case in "$@"; do
    if [ -n "${parallel_mutation_signal}" ]; then
      terminate_mutation_batch
      break
    fi
    index=$((index + 1))
    output_path="${output_root}/${prefix}-${index}.out"
    status_path="${output_root}/${prefix}-${index}.status"
    (
      set +e
      "${worker}" "${mutation_case}"
      worker_status=$?
      printf '%d\n' "${worker_status}" >"${status_path}"
      exit 0
    ) >"${output_path}" 2>&1 &
    batch_pids="${batch_pids}${batch_pids:+ }$!"
    active=$((active + 1))
    if [ -n "${parallel_mutation_signal}" ]; then
      terminate_mutation_batch
      break
    fi

    if [ "${active}" -eq "${worker_count}" ]; then
      wait_for_mutation_batch
      if [ -n "${parallel_mutation_signal}" ]; then
        terminate_mutation_batch
        break
      fi
      collect_mutation_batch "${output_root}" "${prefix}" "${batch_first}" "${index}"
      if [ -n "${parallel_mutation_signal}" ]; then
        terminate_mutation_batch
        break
      fi
      batch_first=$((index + 1))
      active=0
      batch_pids=""
    fi
  done

  if [ -n "${parallel_mutation_signal}" ]; then
    terminate_mutation_batch
  elif [ "${active}" -ne 0 ]; then
    wait_for_mutation_batch
    if [ -n "${parallel_mutation_signal}" ]; then
      terminate_mutation_batch
    else
      collect_mutation_batch "${output_root}" "${prefix}" "${batch_first}" "${index}"
    fi
  fi

  restore_mutation_trap "${saved_int_trap}" INT
  restore_mutation_trap "${saved_term_trap}" TERM
  restore_mutation_trap "${saved_hup_trap}" HUP

  case "${parallel_mutation_signal}" in
    INT) return 130 ;;
    TERM) return 143 ;;
    HUP) return 129 ;;
  esac

  [ "${PARALLEL_MUTATION_FAILED}" -eq 0 ]
}
