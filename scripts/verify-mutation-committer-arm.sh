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

selection_count="$(rg -c 'let uses_ordered_publisher = store\.has_process_local_sequence_authority\(\)' "${engine_root}/tenant.rs" || true)"
[[ "${selection_count}" == "1" ]] \
  || fail "tenant construction must derive exactly one production committer arm"
rg -q 'PublisherHandoff::new\(uses_ordered_publisher,' "${engine_root}/tenant.rs" \
  || fail "tenant construction does not install the immutable committer arm"

rg -q '`committer_arm` is the immutable construction-time mutation owner' \
  "${repo_root}/docs/operators/observability.md" \
  || fail "operator diagnostics do not document immutable committer ownership"

printf 'mutation-committer-arm: immutable selection verified\n'
