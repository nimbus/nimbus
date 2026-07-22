#!/usr/bin/env bash

set -euo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
engine_root="${repo_root}/crates/nimbus-engine/src"

fail() {
  printf 'mutation-committer-arm: %s\n' "$1" >&2
  exit 1
}

forbidden='CommitterPipelineMode|DrainingTo(Pipeline|Serial)|reconcile_mode|requested_mode_override|set_committer_pipeline_requested_for_testing|publisher_mode_transition|mode_(to|from)_u8'
if rg -n "${forbidden}" "${engine_root}"; then
  fail "live committer selection contains a transition or runtime override"
fi

for required_file in \
  crates/nimbus-engine/src/engine/mod.rs \
  crates/nimbus-engine/src/engine/mutations/journal.rs \
  crates/nimbus-engine/src/tenant/mutation/actor.rs \
  crates/nimbus-engine/src/tenant/mutation_facade.rs; do
  rg -q 'uses_ordered_publisher\(' "${repo_root}/${required_file}" \
    || fail "Family-A caller does not consult immutable selection: ${required_file}"
done

selection_count="$(rg -c 'let committer_arm = CommitterArm::for_persistence\(&store\)' "${engine_root}/tenant.rs" || true)"
[[ "${selection_count}" == "1" ]] \
  || fail "tenant construction must derive exactly one exhaustive production committer arm"
rg -q 'PublisherHandoff::new\(committer_arm,' "${engine_root}/tenant.rs" \
  || fail "tenant construction does not install the immutable committer arm"

arm_file="${engine_root}/tenant/mutation/arm.rs"
for adapter in Redb Sqlite LibsqlReplica Postgres MySql Memory; do
  rg -q "TenantPersistence::${adapter}\\(_\\) => Self::" "${arm_file}" \
    || fail "production selector does not exhaustively name ${adapter}"
done
rg -q 'CommitterArm::OrderedPublisher' "${arm_file}" \
  || fail "production selector does not choose the ordered publisher"
if rg -n 'has_process_local_sequence_authority' "${arm_file}" \
  || rg -n 'committer_arm.*has_process_local_sequence_authority|uses_ordered_publisher.*has_process_local_sequence_authority' \
    "${engine_root}/tenant.rs"; then
  fail "committer-arm selection is coupled to process-local window authority"
fi
if rg -n 'SerialJob|send_publisher_serial_job|send_serial_job' "${engine_root}"; then
  fail "publisher handoff still exposes obsolete serial-job vocabulary"
fi
rg -Fq '#[cfg(any(test, feature = "test-hooks"))]' "${arm_file}" \
  || fail "serial reference is not fenced behind test/test-hooks"
rg -q 'SerialReference' "${arm_file}" \
  || fail "test-only serial reference adapter is missing"

rg -q '`committer_arm` is the immutable construction-time mutation owner' \
  "${repo_root}/docs/operators/observability.md" \
  || fail "operator diagnostics do not document immutable committer ownership"
rg -q '`ordered-publisher` for every production persistence topology' \
  "${repo_root}/docs/operators/observability.md" \
  || fail "operator diagnostics do not document the all-topology production arm"

printf 'mutation-committer-arm: immutable selection verified\n'
