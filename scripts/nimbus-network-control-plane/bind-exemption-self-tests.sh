#!/usr/bin/env bash

# Exact path-owned test-module mutation cases for the bind census. This file is
# sourced by the aggregate verifier so its composition root stays thin.

run_nnc46f_bind_exemption_self_tests() {
  local script="$1"
  local temporary="$2"
  local inventory="$3"
  local failures=0

  local invalid_path_exemption="${temporary}/invalid-path-exemption.json"
  node - "${inventory}" "${invalid_path_exemption}" <<'NODE'
const fs = require("fs");
const [source, target] = process.argv.slice(2);
const inventory = JSON.parse(fs.readFileSync(source, "utf8"));
const exemption = inventory.non_production_exemptions.find(
  entry => entry.mechanism === "path-owned-test-module",
);
if (!exemption?.files?.[0]) process.exit(1);
exemption.files[0].module = "lifecycle.*";
fs.writeFileSync(target, JSON.stringify(inventory, null, 2) + "\n");
NODE
  if NIMBUS_NETWORK_VERIFY_INVENTORY="${invalid_path_exemption}" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/invalid-path-exemption.out" 2>&1; then
    printf 'SELFTEST FAIL regex-shaped path exemption unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/invalid-path-exemption.out" ||
    ! grep -q 'path-owned test exemption is not mechanically cfg(test)-owned:' "${temporary}/invalid-path-exemption.out"; then
    printf 'SELFTEST FAIL regex-shaped path exemption did not fail NNCV006 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS regex-shaped path exemption fails closed as NNCV006\n'
  fi

  local misresolved_path_exemption="${temporary}/misresolved-path-exemption.json"
  node - "${inventory}" "${misresolved_path_exemption}" <<'NODE'
const fs = require("fs");
const [source, target] = process.argv.slice(2);
const inventory = JSON.parse(fs.readFileSync(source, "utf8"));
const exemption = inventory.non_production_exemptions.find(
  entry => entry.mechanism === "path-owned-test-module",
);
const evidence = exemption?.files?.find(
  row => row.module === "test_support",
);
if (!evidence) process.exit(1);
evidence.path =
  "crates/nimbus-sandbox/src/backends/oci/network/cluster.rs";
fs.writeFileSync(target, JSON.stringify(inventory, null, 2) + "\n");
NODE
  if NIMBUS_NETWORK_VERIFY_INVENTORY="${misresolved_path_exemption}" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/misresolved-path-exemption.out" 2>&1; then
    printf 'SELFTEST FAIL misresolved conventional test module unexpectedly exited zero\n'
    failures=$((failures + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/misresolved-path-exemption.out" ||
    ! grep -q 'path-owned test exemption is not mechanically cfg(test)-owned: crates/nimbus-sandbox/src/backends/oci/network/cluster.rs' "${temporary}/misresolved-path-exemption.out"; then
    printf 'SELFTEST FAIL misresolved conventional test module did not fail NNCV006 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS conventional test-module exemptions require the exact Rust path\n'
  fi

  local explicit_override_inventory="${temporary}/explicit-path-override.json"
  node - "${inventory}" "${explicit_override_inventory}" "${temporary}/explicit-path-owner" <<'NODE'
const fs = require("fs");
const path = require("path");
const [source, target, fixtureRoot] = process.argv.slice(2);
fs.mkdirSync(fixtureRoot, { recursive: true });
const owner = path.join(fixtureRoot, "owner.rs");
const alternate = path.join(fixtureRoot, "alternate.rs");
const conventional = path.join(fixtureRoot, "child.rs");
fs.writeFileSync(
  owner,
  '#[cfg(test)]\n#[path = "alternate.rs"]\nmod child;\n',
);
fs.writeFileSync(alternate, "fn explicit_child() {}\n");
fs.writeFileSync(conventional, "fn conventional_child() {}\n");
const inventory = JSON.parse(fs.readFileSync(source, "utf8"));
const exemption = inventory.non_production_exemptions.find(
  entry => entry.mechanism === "path-owned-test-module",
);
if (!exemption?.files) process.exit(1);
exemption.files.push({
  path: conventional,
  declared_from: owner,
  cfg_owner: owner,
  owner_module: "child",
  module: "child",
});
fs.writeFileSync(target, JSON.stringify(inventory, null, 2) + "\n");
NODE
  if NIMBUS_NETWORK_VERIFY_INVENTORY="${explicit_override_inventory}" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/explicit-path-override.out" 2>&1; then
    printf 'SELFTEST FAIL conventional target overrode an explicit Rust module path\n'
    failures=$((failures + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/explicit-path-override.out" ||
    ! grep -q 'path-owned test exemption is not mechanically cfg(test)-owned: .*/explicit-path-owner/child.rs' "${temporary}/explicit-path-override.out"; then
    printf 'SELFTEST FAIL explicit Rust module path override did not fail NNCV006 precisely\n'
    failures=$((failures + 1))
  else
    printf 'SELFTEST PASS explicit Rust module paths cannot fall back to conventional targets\n'
  fi

  return "${failures}"
}
