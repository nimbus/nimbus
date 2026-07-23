#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
work_dir="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-ppsc-seed-farm-helper.XXXXXX")"
trap 'rm -rf "${work_dir}"' EXIT

fake_cargo="${work_dir}/cargo"
cat >"${fake_cargo}" <<'EOF'
#!/usr/bin/env bash
set -euo pipefail
expected_test="tests::ppsc::seed_farm::ppsc_seed_farm_executes_selected_redb_scenarios"
if [[ " $* " != *" ${expected_test} "* ]]; then
  echo "unexpected PPSC driver selection: $*" >&2
  exit 64
fi
for argument in "$@"; do
  if [[ "${argument}" == "--list" ]]; then
    if [[ "${FAKE_PPSC_SELECTED:-1}" == "1" ]]; then
      echo "${expected_test}: test"
    fi
    exit 0
  fi
done
exit "${FAKE_PPSC_RUN_EXIT:-0}"
EOF
chmod +x "${fake_cargo}"

run_fixture() {
  NIMBUS_PPSC_CARGO_BIN="${fake_cargo}" \
    NIMBUS_PPSC_FAILURE_DIR="${work_dir}/artifacts" \
    bash "${repo_root}/scripts/ppsc-seed-farm.sh"
}

FAKE_PPSC_SELECTED=1 FAKE_PPSC_RUN_EXIT=0 run_fixture

set +e
FAKE_PPSC_SELECTED=0 FAKE_PPSC_RUN_EXIT=0 run_fixture >/dev/null 2>&1
zero_status=$?
FAKE_PPSC_SELECTED=1 FAKE_PPSC_RUN_EXIT=37 run_fixture >/dev/null 2>&1
failure_status=$?
set -e

if [[ "${zero_status}" -ne 1 ]]; then
  echo "PPSC seed-farm zero-test guard returned ${zero_status}, expected 1" >&2
  exit 1
fi
if [[ "${failure_status}" -ne 37 ]]; then
  echo "PPSC seed-farm runner returned ${failure_status}, expected child status 37" >&2
  exit 1
fi

echo "PPSC seed-farm helper: zero-test rejection and child exit propagation passed"
