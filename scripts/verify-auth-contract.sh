#!/usr/bin/env bash
# Agent auth contract regression gate.
#
# Locks in the doc-only contract introduced by DA10 of
# `docs/private/plans/desktop-auth-dx-plan.md` so that the agent auth shape stays
# committed before the `nimbus agent` implementation lands. Without this
# gate, the contract is easy to drift past unnoticed.
#
# The grep targets are intentionally narrow: section heading, scoped-session
# shape vocabulary, and the three audit-log event names. Edits that rename
# any of these must update this gate intentionally.

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

target="docs/private/architecture/server/auth-runtime-trust.md"
fail=0

check_present() {
  local label=$1
  local pattern=$2
  if ! grep -q -- "$pattern" "$target"; then
    echo "::error::auth-contract grep gate '$label' is missing pattern '$pattern' in '$target'"
    fail=1
  fi
}

check_present "agent auth contract section heading" \
  "## Agent Auth Contract"

check_present "scoped session shape sub-heading" \
  "### Scoped session shape"

check_present "scoped session: capabilities field" \
  "capabilities"

check_present "scoped session: tenant_id field" \
  "tenant_id"

check_present "scoped session: parent_generation field" \
  "parent_generation"

check_present "revocation requirements sub-heading" \
  "### Revocation requirements"

check_present "rotate-admin kill-switch reference" \
  "nimbus auth rotate-admin"

check_present "audit log requirements sub-heading" \
  "### Audit-log requirements"

check_present "audit event: agent_session_minted" \
  "agent_session_minted"

check_present "audit event: agent_session_used" \
  "agent_session_used"

check_present "audit event: agent_session_revoked" \
  "agent_session_revoked"

if (( fail )); then
  exit 1
fi

echo "agent auth contract grep gates clean"
