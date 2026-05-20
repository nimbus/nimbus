#!/usr/bin/env bash
# Desktop UI shell-component regression gates.
#
# These checks lock in the UX2/UX4/UX6/UX7 fixes from
# `docs/plans/archive/desktop-ui-ux-review-fixes-plan.md`. They catch a
# regression at lint-time before it ships to the operator console.
#
# Gates:
#   1. No raw `<select` in route components — `<Select>` shell is canonical.
#   2. No raw `<input type="radio"` outside the SegmentedControl
#      implementation — exclusive-choice goes through the radiogroup shell.
#   3. `/debug/runtime/metrics` references stay in the wired-up hook plus
#      msw fixtures; any new caller needs to land alongside its consumer.

set -euo pipefail

repo_root="$(cd "$(dirname "$0")/.." && pwd)"
cd "$repo_root"

fail=0

check_gate() {
  local label=$1
  shift
  local matches
  if matches=$("$@" 2>/dev/null); then
    if [[ -n "$matches" ]]; then
      echo "::error::desktop-ui grep gate '$label' has unexpected hits:"
      printf '%s\n' "$matches"
      fail=1
    fi
  fi
}

# 1. Raw <select in route components.
check_gate \
  "raw <select in packages/nimbus-ui/src/routes" \
  grep -rn '<select' packages/nimbus-ui/src/routes

# 2. Raw radio inputs anywhere under packages/nimbus-ui/src outside the
#    SegmentedControl shell (which owns the radio role internally).
check_gate \
  "raw <input type=\"radio\" outside SegmentedControl" \
  bash -c '
    grep -rln "<input type=\"radio\"" packages/nimbus-ui/src 2>/dev/null \
      | grep -v "packages/nimbus-ui/src/components/segmented-control" \
      || true
  '

# 3. /debug/runtime/metrics references must stay in the wired-up hook and
#    its msw fixtures; surface any new caller.
allowed_debug_runtime_metrics=$(cat <<'EOF'
packages/nimbus-ui/src/routes/admin/settings/hooks.ts
packages/nimbus-ui/src/test/handlers.ts
packages/nimbus-ui/src/test/msw.spec.ts
EOF
)
check_gate \
  "/debug/runtime/metrics references outside the wired-up consumer" \
  bash -c '
    allowed="$1"
    grep -rln "/debug/runtime/metrics" packages/nimbus-ui/src 2>/dev/null \
      | grep -v -F -x -f <(printf "%s\n" "$allowed") \
      || true
  ' -- "$allowed_debug_runtime_metrics"

if (( fail )); then
  exit 1
fi

echo "desktop-ui shell-component grep gates clean"
