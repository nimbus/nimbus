#!/usr/bin/env bash
set -euo pipefail

usage() {
  cat <<'EOF'
usage: verify-release-version-contract.sh <version-or-tag>

Verify that the intended Nimbus release version matches:
- every Rust crate version under crates/
- every JS workspace package version and local dependency pin
- the package-lock workspace entries
- the top-level CHANGELOG.md release heading

examples:
  bash scripts/verify-release-version-contract.sh v0.1.9
  bash scripts/verify-release-version-contract.sh 0.1.9
EOF
}

if [[ "${1:-}" == "-h" || "${1:-}" == "--help" ]]; then
  usage
  exit 0
fi

if [[ $# -ne 1 ]]; then
  usage >&2
  exit 64
fi

repo_root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
expected_input="$1"
expected_version="${expected_input#v}"
error_count=0

record_mismatch() {
  local label="$1"
  local actual="$2"
  printf 'mismatch: %s expected=%s actual=%s\n' \
    "${label}" "${expected_version}" "${actual:-<missing>}" >&2
  error_count=$((error_count + 1))
}

extract_cargo_version() {
  sed -n 's/^version = "\(.*\)"$/\1/p' "$1" | head -n1
}

manifest_uses_workspace_version() {
  grep -Eq '^version\.workspace = true$' "$1"
}

extract_workspace_package_version() {
  awk '
    /^\[workspace\.package\]$/ { in_workspace_package = 1; next }
    /^\[/ { in_workspace_package = 0 }
    in_workspace_package && /^version = "/ {
      gsub(/^version = "|\"$/, "", $0)
      print
      exit
    }
  ' "$1"
}

workspace_version="$(extract_workspace_package_version "${repo_root}/Cargo.toml")"
if [[ "${workspace_version}" != "${expected_version}" ]]; then
  record_mismatch "workspace.package version" "${workspace_version}"
fi

for manifest in "${repo_root}"/crates/*/Cargo.toml; do
  if manifest_uses_workspace_version "${manifest}"; then
    continue
  fi

  actual_version="$(extract_cargo_version "${manifest}")"
  if [[ "${actual_version}" != "${expected_version}" ]]; then
    record_mismatch "crate $(basename "$(dirname "${manifest}")")" "${actual_version}"
  fi
done

if ! command -v node >/dev/null 2>&1; then
  printf 'mismatch: JS/package verification requires node on PATH\n' >&2
  error_count=$((error_count + 1))
else
  if ! node - "${repo_root}" "${expected_version}" <<'EOF'
const fs = require("fs");
const path = require("path");

const repoRoot = process.argv[2];
const expectedVersion = process.argv[3];
const readJson = (relativePath) =>
  JSON.parse(fs.readFileSync(path.join(repoRoot, relativePath), "utf8"));

const convex = readJson("packages/convex/package.json");
const nimbus = readJson("packages/nimbus/package.json");
const codegen = readJson("packages/codegen/package.json");
const lock = readJson("package-lock.json");
const checks = [
  ["packages/codegen/package.json version", codegen.version],
  ["packages/convex/package.json version", convex.version],
  ["packages/convex/package.json dependency @nimbus/codegen", convex.dependencies?.["@nimbus/codegen"]],
  ["packages/convex/package.json dependency nimbus", convex.dependencies?.["nimbus"]],
  ["packages/nimbus/package.json version", nimbus.version],
  ["packages/codegen version", lock.packages?.["packages/codegen"]?.version],
  ["packages/convex version", lock.packages?.["packages/convex"]?.version],
  ["packages/convex dependency @nimbus/codegen", lock.packages?.["packages/convex"]?.dependencies?.["@nimbus/codegen"]],
  ["packages/convex dependency nimbus", lock.packages?.["packages/convex"]?.dependencies?.["nimbus"]],
  ["packages/nimbus version", lock.packages?.["packages/nimbus"]?.version],
];

const failures = checks
  .filter(([, actual]) => actual !== expectedVersion)
  .map(([label, actual]) => `mismatch: ${label} expected=${expectedVersion} actual=${actual ?? "<missing>"}`);

if (failures.length > 0) {
  console.error(failures.join("\n"));
  process.exit(1);
}
EOF
  then
    error_count=$((error_count + 1))
  fi
fi

if ! grep -Eq "^## \\[${expected_version//./\\.}\\] - " "${repo_root}/CHANGELOG.md"; then
  printf 'mismatch: CHANGELOG.md missing heading for %s\n' "${expected_version}" >&2
  error_count=$((error_count + 1))
fi

if [[ "${error_count}" -ne 0 ]]; then
  exit 1
fi

printf 'verified: release version contract matches %s\n' "${expected_input}"
