#!/usr/bin/env bash

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

usage() {
  cat <<'EOF'
Usage:
  bash scripts/verification-harness.sh pr [storage|engine|server|runtime|all]
  bash scripts/verification-harness.sh nightly [storage|engine|server|runtime|all]
  bash scripts/verification-harness.sh repro <storage|engine|server|runtime> <pr|nightly> <case-id>

Examples:
  bash scripts/verification-harness.sh pr
  bash scripts/verification-harness.sh nightly engine
  bash scripts/verification-harness.sh repro server nightly adversarial-long-tail-131
EOF
}

surface_package() {
  case "$1" in
    storage) echo "neovex-storage" ;;
    engine) echo "neovex-engine" ;;
    server) echo "neovex-server" ;;
    runtime) echo "neovex-runtime" ;;
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
    pr:storage) echo "verification_harness_pr_generated_history_seed_corpus_matches_model" ;;
    pr:engine) echo "verification_harness_pr_generated_history_seed_corpus_matches_model" ;;
    pr:server) echo "verification_harness_pr_generated_history_seed_corpus_matches_model" ;;
    pr:runtime) echo "verification_harness_pr_runtime_liveness_and_integrity_cases" ;;
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
    pr:server) echo "verification_harness_pr_transport_liveness_campaigns" ;;
    nightly:server) echo "verification_harness_nightly_transport_liveness_campaigns" ;;
    *) echo "" ;;
  esac
}

server_transport_test_name() {
  local mode="$1"
  case "$mode" in
    pr) echo "verification_harness_pr_transport_liveness_campaigns" ;;
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
  local cargo_args
  package="$(surface_package "$surface")"
  selected="$(
    cargo test -p "$package" "$test_name" -- --ignored --list 2>/dev/null |
      awk '/: test$/{count++} END{print count+0}'
  )"
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
  bash "${SCRIPT_DIR}/single-flight.sh" \
    --key "verify-harness-${mode}-${surface}" \
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
  NEOVEX_VERIFY_CASE="$case_id" \
    bash "${SCRIPT_DIR}/single-flight.sh" \
      --key "verify-harness-repro-${surface}-${mode}-${case_id}" \
      -- "${cargo_args[@]}"
}

main() {
  local command="${1:-}"
  case "$command" in
    pr|nightly)
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
