#!/usr/bin/env bash
# Runs the enterprise policy and sandbox egress conformance gate.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

printf 'enterprise policy and sandbox egress gate\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

printf '[1/8] operator policy, external backend, draft, and prove fixtures\n'
cargo test -p nimbus-server operator_policy -- --nocapture

printf '\n[2/8] policy CLI fixtures\n'
cargo test -p nimbus-bin policy -- --nocapture

printf '\n[3/8] Compose x-nimbus.egress lowering fixtures\n'
cargo test -p nimbus-bin x_nimbus_egress -- --nocapture

printf '\n[4/8] service-manager policy materialization fixtures\n'
cargo test -p nimbus-server service_manager -- --nocapture

printf '\n[5/8] sandbox egress policy and enforcement contract fixtures\n'
cargo test -p nimbus-egress -- --test-threads=1 --nocapture
cargo test -p nimbus-sandbox egress -- --test-threads=1 --nocapture

printf '\n[6/8] sandbox egress proxy enforcement fixtures\n'
cargo test -p nimbus-sandbox egress_proxy -- --test-threads=1 --nocapture

printf '\n[7/8] tenant isolation audit export and redaction fixtures\n'
cargo test -p nimbus-server audit_events -- --nocapture

printf '\n[8/8] tenant isolation drift fixtures\n'
cargo test -p nimbus-server tenant_isolation_drift -- --nocapture

printf '\nenterprise policy and sandbox egress gate: pass\n'
