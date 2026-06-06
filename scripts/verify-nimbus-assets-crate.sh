#!/usr/bin/env bash
# Control-plane verifier for docs/plans/archive/nimbus-assets-crate-plan.md.
#
# AE0 starts as a baseline inventory gate: it proves the current in-scope
# production embeds are known and rejects any new distribution/UI/template
# embed site outside the inventoried owners. Later phases tighten these checks
# to the final nimbus-assets-owned shape.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}"

PASS=0
FAIL=0
FAIL_DETAIL=()

pass() {
  PASS=$((PASS + 1))
  printf '  \033[32mPASS\033[0m  %s\n' "$1"
}

fail() {
  FAIL=$((FAIL + 1))
  printf '  \033[31mFAIL\033[0m  %s\n' "$1"
  FAIL_DETAIL+=("$1")
}

check() {
  local desc="$1"
  shift
  if "$@" >/dev/null 2>&1; then
    pass "${desc}"
  else
    fail "${desc}"
  fi
}

has() { grep -RqE "$1" "${@:2}" 2>/dev/null; }
absent() { ! grep -RqE "$1" "${@:2}" 2>/dev/null; }

printf '\nNimbus Assets Crate — control-plane verifier\n\n'

PLAN_ACTIVE="docs/plans/nimbus-assets-crate-plan.md"
PLAN_ARCHIVED="docs/plans/archive/nimbus-assets-crate-plan.md"
PLAN_INDEX="docs/plans/README.md"
BPD_OWNER="crates/nimbus-assets/src/js_packages.rs"
OLD_BPD_OWNER="crates/nimbus-bin/src/embedded_packages.rs"
UI_OWNER="crates/nimbus-assets/src/ui.rs"
SERVER_UI_OWNER="crates/nimbus-server/src/http/ui.rs"
INIT_OWNER="crates/nimbus-bin/src/init.rs"
MACHINE_OWNER="crates/nimbus-bin/src/machine/bootstrap.rs"
TEMPLATE_OWNER="crates/nimbus-assets/src/templates.rs"
ASSETS_CARGO="crates/nimbus-assets/Cargo.toml"
ASSETS_LIB="crates/nimbus-assets/src/lib.rs"
ASSETS_BUILD="crates/nimbus-assets/build.rs"

if [ -f "${PLAN_ACTIVE}" ]; then
  PLAN="${PLAN_ACTIVE}"
elif [ -f "${PLAN_ARCHIVED}" ]; then
  PLAN="${PLAN_ARCHIVED}"
else
  PLAN="${PLAN_ARCHIVED}"
fi

c_plan_routed() {
  [ -f "${PLAN}" ] || return 1
  if [ "${PLAN}" = "${PLAN_ACTIVE}" ]; then
    grep -qF "${PLAN_ACTIVE}" "${PLAN_INDEX}"
  else
    grep -qF "${PLAN_ARCHIVED}" "${PLAN_INDEX}" &&
      ! grep -qF "${PLAN_ACTIVE}" "${PLAN_INDEX}"
  fi
}
check "AE0. plan is routed correctly from docs/plans/README.md" c_plan_routed

c_bpd_owner_inventory() {
  [ -f "${BPD_OWNER}" ] &&
    has 'derive\(Embed\)' "${BPD_OWNER}" &&
    has 'folder = "\$CARGO_MANIFEST_DIR/embedded/packages/"' "${BPD_OWNER}" &&
    has 'fn manifest\(' "${BPD_OWNER}" &&
    has 'fn verify_manifest_integrity\(' "${BPD_OWNER}"
}
check "AE2. BPD package embed owner is nimbus-assets::js_packages" c_bpd_owner_inventory

c_ui_owner_inventory() {
  [ -f "${UI_OWNER}" ] &&
    has 'derive\(Embed\)' "${UI_OWNER}" &&
    has 'packages/nimbus-ui/dist/' "${UI_OWNER}" &&
    has 'include_str!\("\.\./embedded/ui-auth/auth\.html"\)' "${UI_OWNER}" &&
    has 'include_str!\("\.\./embedded/ui-auth/auth\.js"\)' "${UI_OWNER}" &&
    has 'use nimbus_assets::ui' "${SERVER_UI_OWNER}" &&
    ! has 'derive\(Embed\)' "${SERVER_UI_OWNER}"
}
check "AE3. UI and auth static asset owner is nimbus-assets::ui" c_ui_owner_inventory

c_init_template_inventory() {
  [ -f "${TEMPLATE_OWNER}" ] &&
    has 'embedded/templates/convex/convex/schema\.ts' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/convex/convex/messages\.ts' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/convex/package\.json\.tmpl' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/cloud-functions/firebase\.json' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/cloud-functions/functions/package\.json\.tmpl' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/cloud-functions/functions/src/index\.ts' "${TEMPLATE_OWNER}" &&
    has 'use nimbus_assets::templates::\{cloud_functions, convex\}' "${INIT_OWNER}"
}
check "AE4. init production templates are owned by nimbus-assets::templates" c_init_template_inventory

c_machine_template_inventory() {
  [ -f "${TEMPLATE_OWNER}" ] &&
    has 'embedded/templates/machine/ready\.service\.tmpl' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/machine/nimbus\.service\.tmpl' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/machine/nimbus\.socket\.tmpl' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/machine/virtiofs-root-off\.service' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/machine/virtiofs-root-on\.service' "${TEMPLATE_OWNER}" &&
    has 'embedded/templates/machine/virtiofs-mount\.service\.tmpl' "${TEMPLATE_OWNER}" &&
    has 'use nimbus_assets::templates::machine as machine_templates' "${MACHINE_OWNER}"
}
check "AE4. machine production templates are owned by nimbus-assets::templates" c_machine_template_inventory

c_assets_skeleton() {
  [ -f "${ASSETS_CARGO}" ] &&
    [ -f "${ASSETS_LIB}" ] &&
    [ -f "${ASSETS_BUILD}" ] &&
    grep -qF '"crates/nimbus-assets"' Cargo.toml &&
    has '^name = "nimbus-assets"$' "${ASSETS_CARGO}" &&
    has '^publish\.workspace = true$' "${ASSETS_CARGO}" &&
    has '^default = \[\]$' "${ASSETS_CARGO}" &&
    has '^ui = \["dep:rust-embed"\]$' "${ASSETS_CARGO}" &&
    has '^js-packages = \["dep:rust-embed", "dep:serde", "dep:serde_json", "dep:sha2"\]$' "${ASSETS_CARGO}" &&
    has '^templates = \[\]$' "${ASSETS_CARGO}" &&
    has '^all = \["ui", "js-packages", "templates"\]$' "${ASSETS_CARGO}" &&
    has '#\[cfg\(feature = "ui"\)\]' "${ASSETS_LIB}" &&
    has '#\[cfg\(feature = "js-packages"\)\]' "${ASSETS_LIB}" &&
    has '#\[cfg\(feature = "templates"\)\]' "${ASSETS_LIB}" &&
    has 'packages/nimbus-ui/dist/index\.html' "${ASSETS_BUILD}" &&
    has 'embedded/packages/manifest\.json' "${ASSETS_BUILD}" &&
    has 'embedded/templates/convex' "${ASSETS_BUILD}" &&
    has 'embedded/templates/cloud-functions' "${ASSETS_BUILD}" &&
    has 'embedded/templates/machine' "${ASSETS_BUILD}"
}
check "AE1. nimbus-assets skeleton and feature contract exist" c_assets_skeleton

c_ae2_js_package_wiring() {
  [ ! -f "${OLD_BPD_OWNER}" ] &&
    has 'nimbus-assets = \{ path = "\.\./nimbus-assets", features = \["js-packages", "templates"\] \}' crates/nimbus-bin/Cargo.toml &&
    ! has '^rust-embed = ' crates/nimbus-bin/Cargo.toml &&
    has 'use nimbus_assets::js_packages' crates/nimbus-bin/src/provision.rs crates/nimbus-bin/src/codegen.rs &&
    has 'crates", "nimbus-assets", "embedded", "packages"' scripts/stage-embedded-packages.mjs &&
    has 'crates", "nimbus-assets", "embedded", "packages"' scripts/check-package-closure.mjs &&
    has 'crates/nimbus-assets/embedded/packages/manifest\.json' Makefile &&
    has 'crates/nimbus-assets/embedded/packages' .github/workflows/ci.yml .github/workflows/coverage.yml &&
    has 'CARGO_ASSETS="crates/nimbus-assets/Cargo\.toml"' scripts/verify-binary-embedded-package-distribution.sh
}
check "AE2. JS package payload scripts and nimbus-bin consumers point at nimbus-assets" c_ae2_js_package_wiring

c_ae3_ui_wiring() {
  has 'nimbus-assets = \{ path = "\.\./nimbus-assets", features = \["ui"\] \}' crates/nimbus-server/Cargo.toml &&
    ! has '^rust-embed = ' crates/nimbus-server/Cargo.toml &&
    [ ! -f crates/nimbus-server/assets/auth.html ] &&
    [ ! -f crates/nimbus-server/assets/auth.js ] &&
    [ -f crates/nimbus-assets/embedded/ui-auth/auth.html ] &&
    [ -f crates/nimbus-assets/embedded/ui-auth/auth.js ] &&
    has 'nimbus-assets embeds artifacts produced by the nimbus-ui JS' Makefile
}
check "AE3. nimbus-server consumes nimbus-assets ui without direct embed ownership" c_ae3_ui_wiring

c_ae4_template_wiring() {
  [ -z "$(find crates/nimbus-bin/templates -type f -print 2>/dev/null)" ] &&
    [ ! -d crates/nimbus-bin/src/machine/assets ] &&
    [ -d crates/nimbus-assets/embedded/templates/convex ] &&
    [ -d crates/nimbus-assets/embedded/templates/cloud-functions ] &&
    [ -d crates/nimbus-assets/embedded/templates/machine ] &&
    ! has 'include_str!\("\.\./templates/' "${INIT_OWNER}" &&
    ! has 'include_str!\("assets/' "${MACHINE_OWNER}" &&
    has 'templates::machine' "${PLAN}"
}
check "AE4. nimbus-bin consumes nimbus-assets templates without direct template includes" c_ae4_template_wiring

c_ae5_cleanup() {
  [ ! -f crates/nimbus-server/build.rs ] &&
    absent '^rust-embed = ' crates/nimbus-bin/Cargo.toml crates/nimbus-server/Cargo.toml &&
    absent 'ensure_(ui|package|asset)|packages/nimbus-ui/dist|embedded/packages/manifest\.json' \
      crates/nimbus-bin/build.rs &&
    absent 'nimbus-assets = \{[^}]*features = \["all"\]' Cargo.toml crates/*/Cargo.toml &&
    absent 'nimbus_assets' crates/nimbus/src/lib.rs &&
    has 'nimbus-assets embeds artifacts produced by the nimbus-ui JS' Makefile &&
    has 'nimbus-assets embeds the dependency-closed JS package payload' .github/workflows/ci.yml .github/workflows/coverage.yml &&
    absent "nimbus-server's \`include_str!\`s|nimbus-server via include_str|nimbus-server.*rust-embed" \
      Makefile .github/workflows/ci.yml docs/operating/local-dev.md
}
check "AE5. old direct embed dependencies, build checks, and stale owner comments are gone" c_ae5_cleanup

c_domain_allowlist_documented() {
  has 'crates/nimbus-runtime/src/module_loader/embedded_builtins\.rs' "${PLAN}" &&
    has 'crates/nimbus-runtime/src/limits/axes\.rs' "${PLAN}" &&
    has 'crates/nimbus-convex/src/registry/loading\.rs' "${PLAN}" &&
    has 'compatibility shims are runtime semantics' "${PLAN}" &&
    has 'generated system Convex' "${PLAN}"
}
check "AE0. domain-owned embeds outside this plan are allowlisted with rationale" c_domain_allowlist_documented

c_domain_allowlist_still_present() {
  [ -f crates/nimbus-runtime/src/module_loader/embedded_builtins.rs ] &&
    [ -f crates/nimbus-runtime/src/limits/axes.rs ] &&
    [ -f crates/nimbus-convex/src/registry/loading.rs ] &&
    has 'include_str!\("builtins/' crates/nimbus-runtime/src/module_loader/embedded_builtins.rs &&
    has 'node-lts-lanes\.json' crates/nimbus-runtime/src/limits/axes.rs &&
    has 'include_(str|bytes)!' crates/nimbus-convex/src/registry/loading.rs
}
check "AE0. allowlisted domain-owned embed sites still belong to their owner crates" c_domain_allowlist_still_present

c_tests_and_fixtures_excluded() {
  rg -n 'include_(str|bytes)!' \
    crates/nimbus-runtime/src/runtime/tests \
    crates/nimbus-bin/tests \
    >/dev/null 2>&1
}
check "AE0. crate-local tests and fixtures are excluded from production movement" c_tests_and_fixtures_excluded

c_no_new_production_embed_roots() {
  local unexpected=0
  local file
  while IFS= read -r file; do
    case "${file}" in
      "${BPD_OWNER}"|"${UI_OWNER}") ;;
      *)
        printf 'unexpected production rust-embed owner: %s\n' "${file}" >&2
        unexpected=1
        ;;
    esac
  done < <(rg -l '#\[derive\(Embed\)\]' crates --glob '*.rs' 2>/dev/null | sort)
  [ "${unexpected}" -eq 0 ]
}
check "AE0. no new production rust-embed roots exist outside inventoried owners" c_no_new_production_embed_roots

c_no_new_production_template_includes() {
  local unexpected=0
  local hit
  while IFS= read -r hit; do
    case "${hit}" in
      ${TEMPLATE_OWNER}:*include_str!\(\"../embedded/templates/convex/*) ;;
      ${TEMPLATE_OWNER}:*include_str!\(\"../embedded/templates/cloud-functions/*) ;;
      ${TEMPLATE_OWNER}:*include_str!\(\"../embedded/templates/machine/*) ;;
      ${UI_OWNER}:*include_str!\(\"../embedded/ui-auth/auth.html\"\)*) ;;
      ${UI_OWNER}:*include_str!\(\"../embedded/ui-auth/auth.js\"\)*) ;;
      *)
        printf 'unexpected production template/static include: %s\n' "${hit}" >&2
        unexpected=1
        ;;
    esac
  done < <(
    rg -n 'include_str!\("(../templates/|assets/|\.\./embedded/templates/|\.\./embedded/ui-auth/|\.\./\.\./assets/auth\.(html|js))' \
      crates/nimbus-bin/src crates/nimbus-server/src crates/nimbus-assets/src \
      --glob '*.rs' --glob '!**/tests/**' 2>/dev/null | sort
  )
  [ "${unexpected}" -eq 0 ]
}
check "AE0. no new production template/static includes exist outside inventoried owners" c_no_new_production_template_includes

printf '\nResult: %s passed, %s failed\n' "${PASS}" "${FAIL}"
if [ "${FAIL}" -ne 0 ]; then
  printf '\nFailures:\n'
  for detail in "${FAIL_DETAIL[@]}"; do
    printf '  - %s\n' "${detail}"
  done
  exit 1
fi
