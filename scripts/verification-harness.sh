#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/verification-harness.sh required [storage|engine|server|runtime|all] [N/M]
  bash scripts/verification-harness.sh nightly [storage|engine|server|runtime|all] [N/M]
  bash scripts/verification-harness.sh repro <storage|engine|server|runtime> <required|nightly> <case-id>

The optional N/M shard argument restricts the corpus to the Nth shard of M
total shards (1 <= N <= M). It is forwarded via NIMBUS_HARNESS_SHARD so the
corpus filter runs at the test-harness level rather than truncating cases at
the test name.

Examples:
  bash scripts/verification-harness.sh required
  bash scripts/verification-harness.sh nightly engine
  bash scripts/verification-harness.sh required server 1/2
  bash scripts/verification-harness.sh repro server nightly adversarial-long-tail-131
EOF
}

validate_shard_spec() {
  local spec="$1"
  if [[ ! "$spec" =~ ^[0-9]+/[0-9]+$ ]]; then
    echo "shard spec must be of the form N/M (got '$spec')" >&2
    exit 1
  fi
  local n="${spec%/*}"
  local m="${spec#*/}"
  if (( n < 1 || m < 1 || n > m )); then
    echo "shard spec must satisfy 1 <= N <= M (got '$spec')" >&2
    exit 1
  fi
}

surface_package() {
  case "$1" in
    storage) echo "nimbus-storage" ;;
    engine) echo "nimbus-engine" ;;
    server) echo "nimbus-server" ;;
    runtime) echo "nimbus-runtime" ;;
    *)
      echo "unknown surface: $1" >&2
      exit 1
      ;;
  esac
}

surface_test_name() {
  local mode="$1"
  local surface="$2"
  case "${mode}:${surface}" in
    required:storage) echo "verification_harness_required_generated_history_seed_corpus_matches_model" ;;
    required:engine) echo "verification_harness_required_generated_history_seed_corpus_matches_model" ;;
    required:server) echo "verification_harness_required_generated_history_seed_corpus_matches_model" ;;
    required:runtime) echo "verification_harness_required_runtime_liveness_and_integrity_cases" ;;
    nightly:storage) echo "verification_harness_nightly_generated_history_seed_corpus_matches_model" ;;
    nightly:engine) echo "verification_harness_nightly_generated_history_seed_corpus_matches_model" ;;
    nightly:server) echo "verification_harness_nightly_generated_history_seed_corpus_matches_model" ;;
    nightly:runtime) echo "verification_harness_nightly_runtime_liveness_and_integrity_cases" ;;
    *)
      echo "unknown verification target: ${mode}:${surface}" >&2
      exit 1
      ;;
  esac
}

surface_additional_test_name() {
  local mode="$1"
  local surface="$2"
  case "${mode}:${surface}" in
    required:server) echo "verification_harness_required_transport_liveness_campaigns" ;;
    nightly:server) echo "verification_harness_nightly_transport_liveness_campaigns" ;;
    *) echo "" ;;
  esac
}

server_transport_test_name() {
  local mode="$1"
  case "$mode" in
    required) echo "verification_harness_required_transport_liveness_campaigns" ;;
    nightly) echo "verification_harness_nightly_transport_liveness_campaigns" ;;
    *)
      echo "unknown verification mode for server transport harness: $mode" >&2
      exit 1
      ;;
  esac
}

repro_test_name() {
  local surface="$1"
  local mode="$2"
  local case_id="$3"
  if [[ "$surface" == "server" ]]; then
    case "$case_id" in
      websocket-disconnect-cleanup|websocket-auth-change-resubscribe|scheduled-job-history-failure-publication|runtime-tenant-fairness-http-rejection|runtime-tenant-fairness-websocket-rejection)
        server_transport_test_name "$mode"
        return
        ;;
    esac
  fi
  surface_test_name "$mode" "$surface"
}

run_surface_filter() {
  local mode="$1"
  local surface="$2"
  local test_name="$3"
  local package
  local selected
  local list_output
  local cargo_args
  local key_suffix=""
  package="$(surface_package "$surface")"
  if ! list_output="$(cargo test -p "$package" "$test_name" -- --ignored --list 2>&1)"; then
    echo "verification harness ${mode}/${surface} failed while listing tests for filter ${test_name}" >&2
    printf '%s\n' "$list_output" >&2
    exit 1
  fi
  selected="$(printf '%s\n' "$list_output" | awk '/: test$/{count++} END{print count+0}')"
  if [[ "$selected" -eq 0 ]]; then
    echo "verification harness ${mode}/${surface} matched zero tests for filter ${test_name}" >&2
    exit 1
  fi
  cargo_args=(cargo test -p "$package" "$test_name" -- --ignored --nocapture)
  if [[ "$surface" == "server" ]]; then
    # The server harness corpus boots multiple ephemeral HTTP fixtures; keep
    # the dedicated ignored corpus lane single-threaded so socket-binding
    # failures cannot hide the actual deterministic campaign result.
    cargo_args+=(--test-threads=1)
  fi
  if [[ -n "${NIMBUS_HARNESS_SHARD:-}" ]]; then
    key_suffix="-shard-${NIMBUS_HARNESS_SHARD//\//-of-}"
  fi
  bash "${SCRIPT_DIR}/single-flight.sh" \
    --key "verify-harness-${mode}-${surface}${key_suffix}" \
    -- "${cargo_args[@]}"
}

run_surface() {
  local mode="$1"
  local surface="$2"
  local primary_test_name
  local additional_test_name
  primary_test_name="$(surface_test_name "$mode" "$surface")"
  run_surface_filter "$mode" "$surface" "$primary_test_name"
  additional_test_name="$(surface_additional_test_name "$mode" "$surface")"
  if [[ -n "$additional_test_name" ]]; then
    run_surface_filter "$mode" "$surface" "$additional_test_name"
  fi
}

run_mode() {
  local mode="$1"
  local surface="${2:-all}"
  if [[ "$surface" == "all" ]]; then
    run_surface "$mode" storage
    run_surface "$mode" engine
    run_surface "$mode" server
    run_surface "$mode" runtime
    return
  fi
  run_surface "$mode" "$surface"
}

run_repro() {
  local surface="$1"
  local mode="$2"
  local case_id="$3"
  local package
  local test_name
  local cargo_args
  package="$(surface_package "$surface")"
  test_name="$(repro_test_name "$surface" "$mode" "$case_id")"
  cargo_args=(cargo test -p "$package" "$test_name" -- --ignored --nocapture)
  if [[ "$surface" == "server" ]]; then
    cargo_args+=(--test-threads=1)
  fi
  NIMBUS_VERIFY_CASE="$case_id" \
    bash "${SCRIPT_DIR}/single-flight.sh" \
      --key "verify-harness-repro-${surface}-${mode}-${case_id}" \
      -- "${cargo_args[@]}"
}

main() {
  local command="${1:-}"
  case "$command" in
    required|nightly)
      if [[ $# -ge 3 && -n "${3:-}" ]]; then
        validate_shard_spec "$3"
        export NIMBUS_HARNESS_SHARD="$3"
      fi
      run_mode "$command" "${2:-all}"
      ;;
    repro)
      if [[ $# -ne 4 ]]; then
        usage >&2
        exit 1
      fi
      run_repro "$2" "$3" "$4"
      ;;
    *)
      usage >&2
      exit 1
      ;;
  esac
}

main "$@"
