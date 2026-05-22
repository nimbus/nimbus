#!/usr/bin/env bash
# Runs the tenant-isolation conformance gate for runtime/sandbox/storage seams.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

printf 'tenant isolation conformance gate\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

printf '[1/2] server/runtime/sandbox/storage conformance scenarios\n'
cargo test -p nimbus-server tenant_isolation_conformance -- --nocapture

printf '\n[2/2] production image admission conformance scenarios\n'
cargo test -p nimbus-bin production_compose_admission -- --nocapture

printf '\ntenant isolation conformance gate: pass\n'
