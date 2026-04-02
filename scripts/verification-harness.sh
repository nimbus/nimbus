#!/usr/bin/env bash

set -euo pipefail

usage() {
  cat <<'EOF'
Usage:
  bash scripts/verification-harness.sh pr [storage|engine|server|all]
  bash scripts/verification-harness.sh nightly [storage|engine|server|all]
  bash scripts/verification-harness.sh repro <storage|engine|server> <pr|nightly> <case-id>

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
    nightly:storage) echo "verification_harness_nightly_generated_history_seed_corpus_matches_model" ;;
    nightly:engine) echo "verification_harness_nightly_generated_history_seed_corpus_matches_model" ;;
    nightly:server) echo "verification_harness_nightly_generated_history_seed_corpus_matches_model" ;;
    *)
      echo "unknown verification target: ${mode}:${surface}" >&2
      exit 1
      ;;
  esac
}

run_surface() {
  local mode="$1"
  local surface="$2"
  local package
  local test_name
  package="$(surface_package "$surface")"
  test_name="$(surface_test_name "$mode" "$surface")"
  cargo test -p "$package" "$test_name" -- --ignored --nocapture
}

run_mode() {
  local mode="$1"
  local surface="${2:-all}"
  if [[ "$surface" == "all" ]]; then
    run_surface "$mode" storage
    run_surface "$mode" engine
    run_surface "$mode" server
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
  package="$(surface_package "$surface")"
  test_name="$(surface_test_name "$mode" "$surface")"
  NEOVEX_VERIFY_CASE="$case_id" \
    cargo test -p "$package" "$test_name" -- --ignored --nocapture
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
