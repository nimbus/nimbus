#!/usr/bin/env bash
# Enforces G4 (Fork-Health Guardrails) from docs/private/plans/nimbus-sandbox-plan.md.
#
# For each Nimbus crate that hosts lifted upstream code, this gate enforces:
#   1. A LICENSE-* file matching the upstream license is present at the crate root.
#   2. A THIRD_PARTY.md manifest lists every lifted file with its provenance.
#   3. Each file listed in THIRD_PARTY.md carries a "Lifted from <project>@<sha>"
#      or "Adapted from <project>@<sha>" header in its first 20 lines.
#
# Scope (per G4): crates/nimbus-guest, crates/nimbus-libkrun-*; plus
# crates/nimbus-blob (RustFS-adapted disk primitives, rustfs-storage-hardening plan).
# Pre-existence: if none of the guarded crates exist yet, this gate passes
# with an informational note — the crates land in Band B2 / Band S.

set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

printf 'third-party attribution gate (G4)\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

fail_count=0
checked_crates=0

fail() {
  printf '  [FAIL] %s\n' "$1" >&2
  fail_count=$((fail_count + 1))
}

pass() {
  printf '  [pass] %s\n' "$1"
}

required_license_for() {
  case "$1" in
    nimbus-blob) printf 'LICENSE-APACHE-rustfs' ;;
    nimbus-guest) printf 'LICENSE-MIT-muvm' ;;
    nimbus-libkrun-snapshot) printf 'LICENSE-APACHE-firecracker' ;;
    nimbus-libkrun-fork-primitive) printf 'LICENSE-APACHE-zeroboot' ;;
    *) printf '' ;;
  esac
}

check_crate() {
  local crate_dir="$1"
  local crate_name
  crate_name="$(basename "${crate_dir}")"

  checked_crates=$((checked_crates + 1))
  printf '[crate] %s\n' "${crate_name}"

  local required_license
  required_license="$(required_license_for "${crate_name}")"
  if [ -n "${required_license}" ]; then
    if [ -f "${crate_dir}/${required_license}" ]; then
      pass "${required_license} present"
    else
      fail "${crate_name}: missing ${required_license} at crate root"
    fi
  else
    pass "no upstream-license file required for ${crate_name}"
  fi

  local manifest="${crate_dir}/THIRD_PARTY.md"
  if [ ! -f "${manifest}" ]; then
    fail "${crate_name}: missing THIRD_PARTY.md manifest"
    return
  fi
  pass "THIRD_PARTY.md present"

  local listed_files
  listed_files="$(awk '
    /^[[:space:]]*\|[[:space:]]*`[^`]+`/ {
      match($0, /`[^`]+`/)
      if (RSTART > 0) {
        path = substr($0, RSTART + 1, RLENGTH - 2)
        print path
      }
    }
  ' "${manifest}")"

  if [ -z "${listed_files}" ]; then
    pass "${crate_name}: THIRD_PARTY.md has no listed files yet"
    return
  fi

  while IFS= read -r rel_path; do
    [ -z "${rel_path}" ] && continue
    local full_path="${crate_dir}/${rel_path}"
    if [ ! -f "${full_path}" ]; then
      fail "${crate_name}: THIRD_PARTY.md lists ${rel_path} but file is missing"
      continue
    fi
    if head -n 20 "${full_path}" | grep -Eq '(Lifted|Adapted) from [^[:space:]]+@[0-9a-f]{7,}'; then
      pass "${rel_path}: provenance header present"
    else
      fail "${crate_name}: ${rel_path} missing 'Lifted from <project>@<sha>' or 'Adapted from <project>@<sha>' in first 20 lines"
    fi
  done <<< "${listed_files}"
}

shopt -s nullglob
crate_dirs=()
if [ -d "crates/nimbus-blob" ]; then
  crate_dirs+=("crates/nimbus-blob")
fi
if [ -d "crates/nimbus-guest" ]; then
  crate_dirs+=("crates/nimbus-guest")
fi
for d in crates/nimbus-libkrun-*; do
  [ -d "${d}" ] && crate_dirs+=("${d}")
done
shopt -u nullglob

if [ "${#crate_dirs[@]}" -eq 0 ]; then
  printf 'no guarded crates exist yet (crates/nimbus-blob, crates/nimbus-guest, crates/nimbus-libkrun-*)\n'
  printf 'gate passes: nothing to enforce until Band B2 / Band S land\n'
  exit 0
fi

for d in "${crate_dirs[@]}"; do
  check_crate "${d}"
  printf '\n'
done

printf 'crates checked: %d\n' "${checked_crates}"
if [ "${fail_count}" -gt 0 ]; then
  printf 'third-party attribution gate: FAIL (%d violation(s))\n' "${fail_count}" >&2
  exit 1
fi

printf 'third-party attribution gate: pass\n'
