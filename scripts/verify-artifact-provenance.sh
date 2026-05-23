#!/usr/bin/env bash
# Runs the artifact provenance verification conformance gate.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

printf 'artifact provenance verification gate\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

printf '[1/5] verifier adapters, Cosign, SLSA, SBOM, offline, and executable artifact fixtures\n'
cargo test -p nimbus-server artifact_provenance -- --nocapture

printf '\n[2/5] runtime invocation provenance gate fixture\n'
cargo test -p nimbus-server runtime_invocation_options_admit_bundle_provenance -- --nocapture

printf '\n[3/5] tenant image admission fixtures\n'
cargo test -p nimbus-server image_admission -- --nocapture

printf '\n[4/5] operator SBOM policy hook fixture\n'
cargo test -p nimbus-server operator_image_policy_sbom_required -- --nocapture

printf '\n[5/5] production Compose image admission fixtures\n'
cargo test -p nimbus-bin production_compose_admission -- --nocapture

printf '\nartifact provenance verification gate: pass\n'
