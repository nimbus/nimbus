#!/usr/bin/env bash
# Runs the enterprise policy and sandbox egress conformance gate.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

printf 'enterprise policy and sandbox egress gate\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

printf '[1/5] operator policy, external backend, draft, and prove fixtures\n'
cargo test -p nimbus-server operator_policy -- --nocapture

printf '\n[2/5] sandbox egress policy and enforcement contract fixtures\n'
cargo test -p nimbus-sandbox egress -- --nocapture

printf '\n[3/5] sandbox egress proxy enforcement fixtures\n'
cargo test -p nimbus-sandbox egress_proxy -- --nocapture

printf '\n[4/5] tenant isolation audit export and redaction fixtures\n'
cargo test -p nimbus-server audit_events -- --nocapture

printf '\n[5/5] tenant isolation drift fixtures\n'
cargo test -p nimbus-server tenant_isolation_drift -- --nocapture

printf '\nenterprise policy and sandbox egress gate: pass\n'
