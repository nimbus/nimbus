#!/usr/bin/env bash
# Verifies the storage-metadata-retention plan's observable contract.
#
# SMR0 intentionally lands this verifier while it is red. Later tasks make
# conditions green at their owning seams. Keep each condition narrow enough
# that a source rename cannot claim behavior that tests and proof do not show.

set -uo pipefail

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${repo_root}"

passed=0
failed=0

pass() {
  passed=$((passed + 1))
  printf 'PASS: %s\n' "$1"
}

fail() {
  failed=$((failed + 1))
  printf 'FAIL: %s\n' "$1"
}

contains() {
  local pattern="$1"
  shift
  rg -q --glob '*.rs' --glob '*.md' --glob '*.sh' "${pattern}" "$@" 2>/dev/null
}

contains_all() {
  local target="$1"
  shift
  local pattern
  for pattern in "$@"; do
    if ! rg -q "${pattern}" "${target}" 2>/dev/null; then
      return 1
    fi
  done
}

if contains_all crates/nimbus-storage/src/retention.rs \
  'enum RetentionGcResource' \
  'struct RetentionGcWatermarks' \
  'pin_protects_resource'; then
  pass 'resource-specific watermarks and participant routing exist'
else
  fail 'resource-specific watermarks and participant routing exist'
fi

if contains_all crates/nimbus-storage/src/retention.rs \
  'compact_retained_versions' \
  'prune_redb_document_versions_before' \
  'prune_redb_index_versions_before' \
  && contains 'compact_retained_versions' \
    crates/nimbus-storage/src/sqlite.rs \
    crates/nimbus-storage/src/sql/store_core.rs; then
  pass 'MVCC compaction exists on embedded and SQL storage seams'
else
  fail 'MVCC compaction exists on embedded and SQL storage seams'
fi

if contains 'retention_gc_preserves_document_anchor_and_respects_pins' \
    crates/nimbus-storage/src/tests \
  && contains_all crates/nimbus-storage/src/retention.rs \
    'latest_anchor_by_document' \
    'visible_until' \
    'prune_before'; then
  pass 'existing tests preserve document anchors and closed index intervals'
else
  fail 'existing tests preserve document anchors and closed index intervals'
fi

if contains 'RetentionExpired' \
    crates/nimbus-storage/src/changefeed.rs \
    crates/nimbus-storage/src/store/journal_snapshot.rs \
  && contains 'point_in_time_archive_rejects_expired_retention_target' \
    crates/nimbus-storage/src/store/journal_snapshot/tests.rs; then
  pass 'trimmed cursor and PITR errors already have a typed classification'
else
  fail 'trimmed cursor and PITR errors already have a typed classification'
fi

if contains_all crates/nimbus-storage/src/diagnostics.rs \
  'retention_gc' \
  'retention_pins' \
  'safe_prune_before'; then
  pass 'storage diagnostics expose current MVCC watermarks and pins'
else
  fail 'storage diagnostics expose current MVCC watermarks and pins'
fi

if contains 'impl_retention_floor_accessors!\(PostgresTenantStore\)' \
    crates/nimbus-storage/src/retention.rs \
  && contains 'impl_retention_floor_accessors!\(MySqlTenantStore\)' \
    crates/nimbus-storage/src/retention.rs \
  && contains 'impl_retention_floor_accessors!\(LibsqlReplicaTenantStore\)' \
    crates/nimbus-storage/src/retention.rs; then
  pass 'all provider stores expose the current retention-floor seam'
else
  fail 'all provider stores expose the current retention-floor seam'
fi

if contains_all crates/nimbus-storage/src/retention/read_safety.rs \
  'fn validate_retention_after_page' \
  'HistoricalReadErrorKind::RetentionExpired' \
  'is behind the retention floor' \
  && contains 'validate_retention_after_page' \
    crates/nimbus-storage/src/store/journal_stream.rs \
    crates/nimbus-storage/src/sqlite/read.rs \
    crates/nimbus-storage/src/postgres/backend.rs \
    crates/nimbus-storage/src/mysql/read.rs \
    crates/nimbus-storage/src/libsql; then
  pass 'journal pages perform an optimistic retention-floor check'
else
  fail 'journal pages perform an optimistic retention-floor check'
fi

if contains_all crates/nimbus-storage/src/retention.rs \
  'document_version_window_sequences' \
  'index_version_window_sequences' \
  'cdc_window_sequences' \
  'pitr_window_sequences' \
  && contains 'MetadataRetentionProfile' crates/nimbus-engine/src; then
  pass 'four durable windows and a shipped Engine profile are explicit'
else
  fail 'four durable windows and a shipped Engine profile are explicit'
fi

if contains 'MaterializedRetentionCheckpoint' crates/nimbus-storage/src \
  && contains 'checkpoint.*MaterializedPosition|MaterializedPosition.*checkpoint' \
    crates/nimbus-storage/src; then
  pass 'a materialized retention checkpoint binds the retained replay base'
else
  fail 'a materialized retention checkpoint binds the retained replay base'
fi

if contains_all crates/nimbus-storage/src/retention.rs \
  'desired_floor' \
  'confirmed_floor' \
  'physical_floor'; then
  pass 'desired, confirmed, and physical floors are distinct state'
else
  fail 'desired, confirmed, and physical floors are distinct state'
fi

if contains 'compact_retained_history' crates/nimbus-storage/src \
  && contains 'journal_records_pruned' crates/nimbus-storage/src; then
  pass 'one maintenance contract checkpoints and prunes journal plus MVCC history'
else
  fail 'one maintenance contract checkpoints and prunes journal plus MVCC history'
fi

if contains 'restores_from_retained_checkpoint|nonzero.*base.*snapshot' \
    crates/nimbus-storage/src/store/journal_snapshot/tests.rs \
  && ! contains 'validate_materialized_journal_replay_base_is_empty\(&archive.base_snapshot\)' \
    crates/nimbus-storage/src/store/journal_snapshot.rs; then
  pass 'PITR export and import accept a validated nonzero retained base'
else
  fail 'PITR export and import accept a validated nonzero retained base'
fi

if contains 'retention_checkpoint.*restart|restart.*retention_checkpoint' \
    crates/nimbus-storage/src/tests \
    crates/nimbus-storage/src/store \
    crates/nimbus-storage/src/sqlite \
  && contains 'checkpoint.*fault|fault.*checkpoint' \
    crates/nimbus-storage/src/tests \
    crates/nimbus-storage/src/store \
    crates/nimbus-storage/src/sqlite; then
  pass 'memory, redb, and SQLite prove restart and checkpoint fault atomicity'
else
  fail 'memory, redb, and SQLite prove restart and checkpoint fault atomicity'
fi

if contains 'fenced_compact_retained_history' crates/nimbus-storage/src \
  && contains 'stale.*lease.*retention|retention.*stale.*lease' \
    crates/nimbus-storage/src/postgres \
    crates/nimbus-storage/src/mysql \
    crates/nimbus-storage/src/libsql \
    crates/nimbus-storage/src/tests; then
  pass 'provider retention finalization is lease-fenced and tested'
else
  fail 'provider retention finalization is lease-fenced and tested'
fi

if contains 'metadata_retention' crates/nimbus-engine/src \
  && contains 'prepare_retained_history' crates/nimbus-engine/src \
  && contains 'finalize_retained_history' crates/nimbus-engine/src \
  && contains 'retain_all' crates/nimbus-engine/src; then
  pass 'the Engine lifecycle prepares off-route, finalizes in order, and exposes retain-all explicitly'
else
  fail 'the Engine lifecycle prepares off-route, finalizes in order, and exposes retain-all explicitly'
fi

if contains 'validate_retention_after_page|post_read_retention' \
    crates/nimbus-storage/src \
    crates/nimbus-engine/src \
  && contains 'concurrent.*prune.*page|page.*concurrent.*prune' \
    crates/nimbus-storage/src \
    crates/nimbus-engine/src; then
  pass 'paged consumers revalidate after reads and cover concurrent pruning'
else
  fail 'paged consumers revalidate after reads and cover concurrent pruning'
fi

if contains 'retention_.*(duration|failure|pruned|lag)' \
    crates/nimbus-engine/src \
    crates/nimbus-storage/src \
  && contains 'confirmed_floor' \
    crates/nimbus-engine/src \
    crates/nimbus-storage/src/diagnostics.rs; then
  pass 'bounded metrics and diagnostics expose lifecycle outcomes and floor lag'
else
  fail 'bounded metrics and diagnostics expose lifecycle outcomes and floor lag'
fi

closeout_proof='docs/private/plans/proof/storage-metadata-retention/smr5-closeout.md'
if [[ -f "${closeout_proof}" ]] \
  && contains_all "${closeout_proof}" \
    'Summary: 18 passed, 0 failed' \
    'make ci' \
    'Nimbus autoreview' \
    'SAFE'; then
  pass 'closeout proof records a green verifier, repository gate, review, and SAFE verdict'
else
  fail 'closeout proof records a green verifier, repository gate, review, and SAFE verdict'
fi

printf '\nSummary: %d passed, %d failed\n' "${passed}" "${failed}"

if ((failed > 0)); then
  exit 1
fi
