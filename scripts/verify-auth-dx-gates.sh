#!/usr/bin/env bash
# Desktop Auth & Sign-in DX regression gates.
#
# These checks lock in the cross-CLI microcopy decisions from
# `docs/plans/desktop-auth-dx-plan.md` (DA1-DA6). They catch microcopy drift
# at lint-time before it reaches the operator console.
#
# Gates:
#   1. The platform-specific `Library/Application Support/nimbus/auth/token`
#      literal must stay confined to the disclosure block in the operator
#      console (auth.html) plus the path-resolution unit tests. CLI hint
#      surfaces (anything under crates/nimbus-bin) must not embed it.
#   2. The legacy `nimbus dev --open` opt-in flag was renamed to `--no-open`
#      in DA3. The `--open` spelling must not reappear anywhere.
#   3. The canonical sign-in sentence (`Open this URL to sign in:`) must
#      remain present in the surfaces that announce it — dev.rs and the
#      first-boot banner — so that all CLI surfaces converge on the same
#      copy.

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
      echo "::error::auth-dx grep gate '$label' has unexpected hits:"
      printf '%s\n' "$matches"
      fail=1
    fi
  fi
}

check_present() {
  local label=$1
  local pattern=$2
  local target=$3
  if ! grep -q "$pattern" "$target"; then
    echo "::error::auth-dx grep gate '$label' is missing the required pattern '$pattern' in '$target'"
    fail=1
  fi
}

# 1. Platform-specific token-file literal must not leak into the CLI hint
#    surfaces. The disclosure in the operator console and the path tests are
#    the only places this string is allowed to appear.
check_gate \
  "Application Support/nimbus/auth/token literal in crates/nimbus-bin" \
  grep -rn "Application Support/nimbus/auth/token" crates/nimbus-bin/src

# 2. The legacy --open opt-in flag spelling must not reappear. DA3 inverted
#    the default and renamed the flag to --no-open.
check_gate \
  "legacy 'nimbus dev --open' opt-in flag" \
  bash -c '
    grep -rn -- "--open" crates/nimbus-bin/src packages/nimbus-ui/src 2>/dev/null \
      | grep -v -- "--no-open" \
      || true
  '

# 3. Canonical sign-in sentence must remain present in the surfaces that
#    announce it. This is a positive sanity gate — if any of these drift
#    or disappear, the user-facing copy will fragment again.
check_present \
  "canonical sign-in sentence in dev.rs" \
  "Open this URL to sign in:" \
  crates/nimbus-bin/src/dev.rs

check_present \
  "canonical sign-in sentence in first_boot.rs" \
  "Open this URL to sign in:" \
  crates/nimbus-bin/src/start/first_boot.rs

if (( fail )); then
  exit 1
fi

echo "auth-dx microcopy grep gates clean"
