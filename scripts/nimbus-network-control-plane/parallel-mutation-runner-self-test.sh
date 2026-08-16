#!/usr/bin/env bash

set -u

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=scripts/nimbus-network-control-plane/parallel-mutation-runner.sh
. "${SCRIPT_DIR}/parallel-mutation-runner.sh"

temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-parallel-mutation-runner.XXXXXX")" || exit 1
unrelated_pid=""
cleanup() {
  if [ -n "${unrelated_pid}" ] && kill -0 "${unrelated_pid}" 2>/dev/null; then
    kill -TERM "${unrelated_pid}" 2>/dev/null || true
    wait "${unrelated_pid}" 2>/dev/null || true
  fi
  rm -rf "${temporary}"
}
trap cleanup EXIT
failures=0

coordinated_worker() {
  name="$1"
  : >"${temporary}/${name}.started"
  peer=left
  if [ "${name}" = left ]; then
    peer=right
  fi
  attempts=0
  while [ ! -f "${temporary}/${peer}.started" ] && [ "${attempts}" -lt 500 ]; do
    sleep 0.01
    attempts=$((attempts + 1))
  done
  if [ ! -f "${temporary}/${peer}.started" ]; then
    printf 'worker %s did not overlap its peer\n' "${name}"
    return 1
  fi
  printf '%s\n' "${name}"
}

NIMBUS_NETWORK_MUTATION_JOBS=2
export NIMBUS_NETWORK_MUTATION_JOBS
if ! run_parallel_mutation_cases "${temporary}/overlap" ordered coordinated_worker left right \
  >"${temporary}/overlap.out"; then
  printf 'SELFTEST FAIL parallel workers did not overlap\n'
  failures=$((failures + 1))
elif [ "$(cat "${temporary}/overlap.out")" != "$(printf 'left\nright')" ]; then
  printf 'SELFTEST FAIL parallel output did not retain input order\n'
  failures=$((failures + 1))
else
  printf 'SELFTEST PASS parallel workers overlap with deterministic output\n'
fi

failing_worker() {
  case "$1" in
    pass)
      printf 'pass\n'
      return 0
      ;;
    fail)
      printf 'fail\n'
      return 1
      ;;
  esac
  return 2
}

if run_parallel_mutation_cases "${temporary}/failure" failure failing_worker pass fail \
  >"${temporary}/failure.out"; then
  printf 'SELFTEST FAIL a failed worker did not fail the batch\n'
  failures=$((failures + 1))
elif [ "${PARALLEL_MUTATION_PASSED}" -ne 1 ] || [ "${PARALLEL_MUTATION_FAILED}" -ne 1 ]; then
  printf 'SELFTEST FAIL worker result accounting is not exact\n'
  failures=$((failures + 1))
else
  printf 'SELFTEST PASS worker failure is retained in exact result counts\n'
fi

NIMBUS_NETWORK_MUTATION_JOBS=invalid
export NIMBUS_NETWORK_MUTATION_JOBS
invalid_status=0
run_parallel_mutation_cases "${temporary}/invalid" invalid failing_worker pass \
  >"${temporary}/invalid.out" 2>&1 || invalid_status=$?
if [ "${invalid_status}" -ne 2 ]; then
  printf 'SELFTEST FAIL invalid worker count was accepted\n'
  failures=$((failures + 1))
elif ! grep -q '^invalid NIMBUS_NETWORK_MUTATION_JOBS: invalid$' "${temporary}/invalid.out"; then
  printf 'SELFTEST FAIL invalid worker count missed its diagnostic\n'
  failures=$((failures + 1))
else
  printf 'SELFTEST PASS invalid worker count fails closed\n'
fi

NIMBUS_NETWORK_MUTATION_JOBS=00
export NIMBUS_NETWORK_MUTATION_JOBS
leading_zero_status=0
run_parallel_mutation_cases "${temporary}/leading-zero" leading-zero failing_worker pass \
  >"${temporary}/leading-zero.out" 2>&1 || leading_zero_status=$?
if [ "${leading_zero_status}" -ne 2 ]; then
  printf 'SELFTEST FAIL zero worker count with leading zeroes was accepted\n'
  failures=$((failures + 1))
elif ! grep -q '^invalid NIMBUS_NETWORK_MUTATION_JOBS: 00$' \
  "${temporary}/leading-zero.out"; then
  printf 'SELFTEST FAIL leading-zero worker count missed its diagnostic\n'
  failures=$((failures + 1))
else
  printf 'SELFTEST PASS leading-zero worker count fails closed\n'
fi

NIMBUS_NETWORK_MUTATION_JOBS=1
export NIMBUS_NETWORK_MUTATION_JOBS
trap 'printf "" >/dev/null' TERM
sleep 5 &
unrelated_pid=$!
if ! run_parallel_mutation_cases "${temporary}/unrelated" unrelated failing_worker pass \
  >"${temporary}/unrelated.out"; then
  printf 'SELFTEST FAIL runner failed while an unrelated background job existed\n'
  failures=$((failures + 1))
elif ! kill -0 "${unrelated_pid}" 2>/dev/null; then
  printf 'SELFTEST FAIL runner waited for an unrelated background job\n'
  failures=$((failures + 1))
else
  printf 'SELFTEST PASS runner waits only for its owned workers\n'
fi
kill -TERM "${unrelated_pid}" 2>/dev/null || true
wait "${unrelated_pid}" 2>/dev/null || true
unrelated_pid=""
trap -p TERM >"${temporary}/restored-term-trap"
if ! grep -q 'printf "" >/dev/null' "${temporary}/restored-term-trap"; then
  printf 'SELFTEST FAIL runner did not restore the caller TERM trap\n'
  failures=$((failures + 1))
else
  printf 'SELFTEST PASS runner restores caller signal traps\n'
fi
trap - TERM

blocking_worker() {
  worker_name="$1"
  sleep 30 &
  child_pid=$!
  worker_process="$(ps -o ppid= -p "${child_pid}" | tr -d ' ')"
  printf '%s %s\n' "${worker_process}" "${child_pid}" >"${temporary}/${worker_name}.pids"
  wait "${child_pid}"
}

(
  NIMBUS_NETWORK_MUTATION_JOBS=2
  export NIMBUS_NETWORK_MUTATION_JOBS
  run_parallel_mutation_cases \
    "${temporary}/interrupted" interrupted blocking_worker left right
) >"${temporary}/interrupted.out" 2>&1 &
runner_pid=$!
attempts=0
while { [ ! -f "${temporary}/left.pids" ] || [ ! -f "${temporary}/right.pids" ]; } &&
  [ "${attempts}" -lt 500 ]; do
  sleep 0.01
  attempts=$((attempts + 1))
done
interrupted_status=0
if [ ! -f "${temporary}/left.pids" ] || [ ! -f "${temporary}/right.pids" ]; then
  printf 'SELFTEST FAIL interrupt fixture did not start both workers\n'
  failures=$((failures + 1))
  kill -TERM "${runner_pid}" 2>/dev/null || true
  wait "${runner_pid}" 2>/dev/null || true
else
  kill -TERM "${runner_pid}"
  wait "${runner_pid}" || interrupted_status=$?
  leaked_pids=""
  while read -r worker_process child_process; do
    for owned_pid in "${worker_process}" "${child_process}"; do
      if kill -0 "${owned_pid}" 2>/dev/null; then
        leaked_pids="${leaked_pids}${leaked_pids:+ }${owned_pid}"
        kill -KILL "${owned_pid}" 2>/dev/null || true
      fi
    done
  done < <(cat "${temporary}/left.pids" "${temporary}/right.pids")
  if [ "${interrupted_status}" -ne 143 ]; then
    printf 'SELFTEST FAIL interrupted runner returned %d instead of 143\n' \
      "${interrupted_status}"
    failures=$((failures + 1))
  elif [ -n "${leaked_pids}" ]; then
    printf 'SELFTEST FAIL interrupted runner leaked owned processes: %s\n' "${leaked_pids}"
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS interruption terminates only runner-owned process trees\n'
  fi
fi

(
  NIMBUS_NETWORK_MUTATION_JOBS=1
  export NIMBUS_NETWORK_MUTATION_JOBS
  run_parallel_mutation_cases \
    "${temporary}/cancel-before-next" cancel-before-next blocking_worker serial-left serial-right
) >"${temporary}/cancel-before-next.out" 2>&1 &
serial_runner_pid=$!
attempts=0
while [ ! -f "${temporary}/serial-left.pids" ] && [ "${attempts}" -lt 500 ]; do
  sleep 0.01
  attempts=$((attempts + 1))
done
serial_status=0
if [ ! -f "${temporary}/serial-left.pids" ]; then
  printf 'SELFTEST FAIL serial cancellation fixture did not start its first worker\n'
  failures=$((failures + 1))
  kill -TERM "${serial_runner_pid}" 2>/dev/null || true
  wait "${serial_runner_pid}" 2>/dev/null || true
else
  kill -TERM "${serial_runner_pid}"
  wait "${serial_runner_pid}" || serial_status=$?
  if [ "${serial_status}" -ne 143 ]; then
    printf 'SELFTEST FAIL serial cancellation returned %d instead of 143\n' \
      "${serial_status}"
    failures=$((failures + 1))
  elif [ -f "${temporary}/serial-right.pids" ]; then
    printf 'SELFTEST FAIL runner launched another worker after cancellation\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS cancellation prevents later worker launches\n'
  fi
fi

(
  NIMBUS_NETWORK_MUTATION_JOBS=2
  export NIMBUS_NETWORK_MUTATION_JOBS
  run_parallel_mutation_cases \
    "${temporary}/partial-batch" partial-batch blocking_worker partial-left
) >"${temporary}/partial-batch.out" 2>&1 &
partial_runner_pid=$!
attempts=0
while [ ! -f "${temporary}/partial-left.pids" ] && [ "${attempts}" -lt 500 ]; do
  sleep 0.01
  attempts=$((attempts + 1))
done
partial_status=0
if [ ! -f "${temporary}/partial-left.pids" ]; then
  printf 'SELFTEST FAIL partial-batch cancellation fixture did not start its worker\n'
  failures=$((failures + 1))
  kill -TERM "${partial_runner_pid}" 2>/dev/null || true
  wait "${partial_runner_pid}" 2>/dev/null || true
else
  kill -TERM "${partial_runner_pid}"
  wait "${partial_runner_pid}" || partial_status=$?
  partial_leaks=""
  while read -r worker_process child_process; do
    for owned_pid in "${worker_process}" "${child_process}"; do
      if kill -0 "${owned_pid}" 2>/dev/null; then
        partial_leaks="${partial_leaks}${partial_leaks:+ }${owned_pid}"
        kill -KILL "${owned_pid}" 2>/dev/null || true
      fi
    done
  done <"${temporary}/partial-left.pids"
  if [ "${partial_status}" -ne 143 ]; then
    printf 'SELFTEST FAIL partial-batch cancellation returned %d instead of 143\n' \
      "${partial_status}"
    failures=$((failures + 1))
  elif [ -n "${partial_leaks}" ]; then
    printf 'SELFTEST FAIL partial-batch cancellation leaked owned processes: %s\n' \
      "${partial_leaks}"
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS cancellation terminates a final partial batch\n'
  fi
fi

if [ "${failures}" -ne 0 ]; then
  printf 'parallel mutation runner self-test: %d failed\n' "${failures}"
  exit 1
fi
printf 'parallel mutation runner self-test: 9 passed, 0 failed\n'
