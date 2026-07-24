#!/usr/bin/env bash
# Aggregate static verifier for the transport-free Nimbus network control plane.
#
# NNC0.8 intentionally lands this gate red: later extraction bands remove the
# named legacy authorities and create `nimbus-network`. Missing inputs are hard
# failures. Run `--self-test` to prove missing-input, unclassified-bind, and
# low-level source-contract diagnostics themselves fail closed.

set -u

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "${REPO_ROOT}" || exit 1

PLAN="${NIMBUS_NETWORK_VERIFY_PLAN:-docs/private/plans/nimbus-network-control-plane-plan.md}"
PLAN_INDEX="docs/private/plans/README.md"
INVENTORY="${NIMBUS_NETWORK_VERIFY_INVENTORY:-docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-bind-owner-inventory.json}"
DEPENDENCIES="${NIMBUS_NETWORK_VERIFY_DEPENDENCIES:-docs/private/plans/proof/nimbus-network-control-plane/nnc0.1-dependency-graph.json}"
NETWORK_MANIFEST="crates/nimbus-network/Cargo.toml"
CORE_SOURCE_ROOT="${NIMBUS_NETWORK_VERIFY_CORE_SCAN_ROOT:-crates/nimbus-core/src}"
SOURCE_CONTRACT_HELPER="scripts/verify-nimbus-network-source-contract.mjs"

PASS_COUNT=0
FAIL_COUNT=0

pass() {
  PASS_COUNT=$((PASS_COUNT + 1))
  printf 'PASS %s %s\n' "$1" "$2"
}

fail() {
  FAIL_COUNT=$((FAIL_COUNT + 1))
  printf 'FAIL %s %s\n' "$1" "$2"
  if [ $# -ge 3 ] && [ -n "$3" ]; then
    printf '     %s\n' "$3"
  fi
}

require_tools() {
  missing=""
  for tool in git rg node cargo; do
    if ! command -v "${tool}" >/dev/null 2>&1; then
      missing="${missing}${missing:+, }${tool}"
    fi
  done
  if [ -z "${missing}" ]; then
    pass "NNCV000" "required-tools"
  else
    fail "NNCV000" "required-tools" "missing: ${missing}"
  fi
}

verify_plan_in_head() {
  if [ ! -f "${PLAN}" ]; then
    fail "NNCV001" "plan-in-HEAD" "missing working-tree input: ${PLAN}"
    return
  fi
  case "${PLAN}" in
    /*)
      fail "NNCV001" "plan-in-HEAD" "canonical plan override is not repository-relative: ${PLAN}"
      ;;
    *)
      if git cat-file -e "HEAD:${PLAN}" 2>/dev/null; then
        pass "NNCV001" "plan-in-HEAD"
      else
        fail "NNCV001" "plan-in-HEAD" "${PLAN} exists only outside HEAD or is unreadable from HEAD"
      fi
      ;;
  esac
}

verify_baseline_inputs() {
  error="$(
    node - "${INVENTORY}" "${DEPENDENCIES}" <<'NODE'
const fs = require("fs");
const [inventoryPath, dependencyPath] = process.argv.slice(2);
const errors = [];

function readJson(path, label) {
  if (!fs.existsSync(path)) {
    errors.push(label + " missing: " + path);
    return null;
  }
  let text;
  try {
    text = fs.readFileSync(path, "utf8");
  } catch (error) {
    errors.push(label + " unreadable: " + path + ": " + error.message);
    return null;
  }
  if (!text.trim()) {
    errors.push(label + " empty: " + path);
    return null;
  }
  try {
    return JSON.parse(text);
  } catch (error) {
    errors.push(label + " invalid JSON: " + path + ": " + error.message);
    return null;
  }
}

const inventory = readJson(inventoryPath, "bind inventory");
const dependencies = readJson(dependencyPath, "dependency baseline");

if (inventory) {
  if (inventory.schema_version !== 1) errors.push("bind inventory schema_version must be 1");
  if (!Array.isArray(inventory.production_sites) || inventory.production_sites.length === 0) {
    errors.push("bind inventory production_sites must be non-empty");
  }
  if (!Array.isArray(inventory.non_production_exemptions)) {
    errors.push("bind inventory non_production_exemptions must be an array");
  }
  if (!inventory.summary || inventory.summary.production_sites !== inventory.production_sites?.length) {
    errors.push("bind inventory summary.production_sites does not match production_sites");
  }
  if (inventory.summary?.unclassified_production_sites !== 0) {
    errors.push("bind inventory baseline records unclassified production sites");
  }
  const ids = new Set();
  for (const site of inventory.production_sites || []) {
    for (const field of ["id", "path", "current_owner", "target_owner", "owner_item"]) {
      if (typeof site[field] !== "string" || !site[field].trim()) {
        errors.push("bind inventory site " + (site.id || "<unknown>") + " lacks " + field);
      }
    }
    if (ids.has(site.id)) errors.push("duplicate bind inventory id: " + site.id);
    ids.add(site.id);
    if (site.path && !fs.existsSync(site.path)) errors.push("bind inventory path missing: " + site.path);
  }
  if (typeof inventory.source_head !== "string" || !/^[0-9a-f]{7,40}$/.test(inventory.source_head)) {
    errors.push("bind inventory source_head is missing or invalid");
  }
}

if (dependencies) {
  if (dependencies.schema_version !== 1) errors.push("dependency baseline schema_version must be 1");
  if (!Array.isArray(dependencies.profiles) || dependencies.profiles.length < 4) {
    errors.push("dependency baseline must contain at least four profiles");
  }
  if (!Array.isArray(dependencies.targets) || dependencies.targets.length < 2) {
    errors.push("dependency baseline must contain multiple targets");
  }
  if (typeof dependencies.source_head !== "string" || !/^[0-9a-f]{7,40}$/.test(dependencies.source_head)) {
    errors.push("dependency baseline source_head is missing or invalid");
  }
}

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  node_status=$?

  ancestry_error=""
  if [ "${node_status}" -eq 0 ]; then
    for input in "${INVENTORY}" "${DEPENDENCIES}"; do
      source_head="$(node -e 'const fs=require("fs"); process.stdout.write(JSON.parse(fs.readFileSync(process.argv[1],"utf8")).source_head)' "${input}" 2>/dev/null)"
      if [ -z "${source_head}" ] || ! git merge-base --is-ancestor "${source_head}" HEAD 2>/dev/null; then
        ancestry_error="${ancestry_error}${ancestry_error:+; }${input} source_head ${source_head:-<missing>} is not an ancestor of HEAD"
      fi
    done
  fi

  if [ "${node_status}" -eq 0 ] && [ -z "${ancestry_error}" ]; then
    pass "NNCV002" "required-baseline-inputs"
  else
    fail "NNCV002" "required-baseline-inputs" "${error}${error:+; }${ancestry_error}"
  fi
}

verify_network_crate() {
  if [ ! -f "${NETWORK_MANIFEST}" ]; then
    fail "NNCV003" "nimbus-network-crate" "missing ${NETWORK_MANIFEST}"
    return
  fi
  if cargo metadata --no-deps --format-version 1 2>/dev/null |
    node -e '
      let text = "";
      process.stdin.on("data", chunk => text += chunk);
      process.stdin.on("end", () => {
        const metadata = JSON.parse(text);
        const pkg = metadata.packages.find(candidate => candidate.name === "nimbus-network");
        process.exit(pkg && metadata.workspace_members.includes(pkg.id) ? 0 : 1);
      });
    '; then
    pass "NNCV003" "nimbus-network-crate"
  else
    fail "NNCV003" "nimbus-network-crate" "manifest exists but package is not a workspace member"
  fi
}

verify_network_dependency_contract() {
  if [ ! -f "${NETWORK_MANIFEST}" ]; then
    fail "NNCV004" "network-dependency-contract" "unavailable because nimbus-network crate is missing"
    return
  fi
  error="$(
    cargo metadata --no-deps --format-version 1 2>/dev/null |
      node -e '
        let text = "";
        process.stdin.on("data", chunk => text += chunk);
        process.stdin.on("end", () => {
          const metadata = JSON.parse(text);
          const pkg = metadata.packages.find(candidate => candidate.name === "nimbus-network");
          if (!pkg) {
            process.stdout.write("nimbus-network package absent from cargo metadata");
            process.exit(1);
          }
          const workspaceNames = new Set(metadata.packages.map(candidate => candidate.name));
          const edges = pkg.dependencies
            .filter(dep => dep.source === null && workspaceNames.has(dep.name))
            .map(dep => dep.name + ":" + (dep.kind || "normal"));
          const invalid = edges.filter(edge => !edge.startsWith("nimbus-core:"));
          const core = edges.filter(edge => edge.startsWith("nimbus-core:"));
          if (invalid.length || core.length !== 1 || edges.some(edge => edge.startsWith("nimbus-testing:"))) {
            process.stdout.write("workspace dependencies must be exactly nimbus-core; found " + (edges.join(", ") || "<none>"));
            process.exit(1);
          }
        });
      '
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV004" "network-dependency-contract"
  else
    fail "NNCV004" "network-dependency-contract" "${error:-cargo metadata failed}"
  fi
}

verify_single_port_authority() {
  legacy="$(
    rg -n \
      -e 'struct PortManager' \
      -e 'fn resolve_listener_port' \
      -e 'fn ephemeral_port' \
      -e 'fn allocate_machine_ssh_port' \
      -e 'fn machine_port_is_available' \
      crates/nimbus-sandbox/src \
      crates/nimbus-cli/src 2>&1
  )"
  scan_status=$?
  if [ "${scan_status}" -gt 1 ]; then
    fail "NNCV005" "no-duplicate-port-allocation-authority" "source scan failed: ${legacy}"
  elif [ -z "${legacy}" ] && [ -f "${NETWORK_MANIFEST}" ]; then
    pass "NNCV005" "no-duplicate-port-allocation-authority"
  else
    detail="${legacy:-nimbus-network lease authority is absent}"
    fail "NNCV005" "no-duplicate-port-allocation-authority" "${detail}"
  fi
}

verify_bind_census() {
  census_output="$(mktemp "${TMPDIR:-/tmp}/nimbus-network-bind-census.XXXXXX")" || {
    fail "NNCV006" "unclassified-production-bind" "unable to create census output file"
    return
  }
  node - "${INVENTORY}" >"${census_output}" <<'NODE'
const fs = require("fs");
const path = require("path");

const inventoryPath = process.argv[2];
const errors = [];
if (!fs.existsSync("crates") || !fs.statSync("crates").isDirectory()) {
  errors.push("production source root missing: crates");
}
if (!fs.existsSync(inventoryPath)) {
  process.stdout.write("bind inventory missing: " + inventoryPath);
  process.exit(1);
}

let inventory;
try {
  inventory = JSON.parse(fs.readFileSync(inventoryPath, "utf8"));
} catch (error) {
  process.stdout.write("bind inventory invalid: " + error.message);
  process.exit(1);
}

const classifiedPaths = new Set((inventory.production_sites || []).map(site => site.path));
const explicitTestFiles = new Set(
  (inventory.non_production_exemptions || []).flatMap(entry => entry.examples || [])
);
const candidates = [];

function walk(directory) {
  if (!fs.existsSync(directory)) return;
  for (const entry of fs.readdirSync(directory, {withFileTypes: true})) {
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) {
      if (entry.name === "tests" || entry.name === "benches") continue;
      walk(full);
    } else if (entry.isFile() && entry.name.endsWith(".rs") && entry.name !== "tests.rs") {
      candidates.push({file: full.split(path.sep).join("/")});
    }
  }
}
walk("crates");
if (process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1" &&
    process.env.NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE) {
  candidates.push({
    file: "__nimbus_network_verifier_self_test__/cfg-test-followed-by-production.rs",
    source: process.env.NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE,
  });
}

const authorityPatterns = [
  /(?:TcpListener|UdpSocket|TcpSocket|Socket)::bind\s*\(/,
  /\bfn\s+(?:resolve_listener_port|ephemeral_port|allocate_machine_ssh_port|machine_port_is_available)\b/,
  /\bstruct\s+PortManager\b/,
  /["\u0027]LISTEN_FDS["\u0027]/,
];

function maskNonCode(rustText) {
  const lexicalView = rustText.split("");
  const blank = (start, end) => {
    for (let cursor = start; cursor < end; cursor += 1) {
      if (lexicalView.at(cursor) !== "\n" && lexicalView.at(cursor) !== "\r") {
        lexicalView.splice(cursor, 1, " ");
      }
    }
  };

  let cursor = 0;
  while (cursor < rustText.length) {
    if (rustText.startsWith("//", cursor)) {
      const end = rustText.indexOf("\n", cursor + 2);
      blank(cursor, end < 0 ? rustText.length : end);
      cursor = end < 0 ? rustText.length : end;
      continue;
    }
    if (rustText.startsWith("/*", cursor)) {
      let depth = 1;
      let end = cursor + 2;
      while (end < rustText.length && depth > 0) {
        if (rustText.startsWith("/*", end)) {
          depth += 1;
          end += 2;
        } else if (rustText.startsWith("*/", end)) {
          depth -= 1;
          end += 2;
        } else {
          end += 1;
        }
      }
      blank(cursor, end);
      cursor = end;
      continue;
    }

    const raw = rustText.slice(cursor).match(/^(?:br|rb|cr|r)(#*)"/);
    if (raw) {
      const terminator = "\"" + raw[1];
      const contentStart = cursor + raw[0].length;
      const found = rustText.indexOf(terminator, contentStart);
      const end = found < 0 ? rustText.length : found + terminator.length;
      blank(cursor, end);
      cursor = end;
      continue;
    }

    const quoteOffset = ["b", "c"].includes(rustText[cursor]) && rustText[cursor + 1] === "\"" ? 1 : 0;
    if (rustText[cursor + quoteOffset] === "\"") {
      let end = cursor + quoteOffset + 1;
      while (end < rustText.length) {
        if (rustText[end] === "\\") {
          end += 2;
        } else if (rustText[end] === "\"") {
          end += 1;
          break;
        } else {
          end += 1;
        }
      }
      blank(cursor, end);
      cursor = end;
      continue;
    }

    if (rustText[cursor] === "\u0027") {
      const character = rustText.slice(cursor).match(/^\u0027(?:\\.|[^\\\u0027\r\n])\u0027/u);
      if (character) {
        blank(cursor, cursor + character[0].length);
        cursor += character[0].length;
        continue;
      }
    }
    cursor += 1;
  }
  return lexicalView.join("");
}

function withoutCfgTestItems(rustText) {
  const lexicalView = maskNonCode(rustText);
  const ranges = [];
  const cfgTest = /#\s*\[\s*cfg\s*\(\s*test\s*\)\s*\]/g;
  let attribute;
  while ((attribute = cfgTest.exec(lexicalView)) !== null) {
    if (ranges.some(([start, end]) => attribute.index >= start && attribute.index < end)) continue;
    let cursor = cfgTest.lastIndex;
    let parentheses = 0;
    let brackets = 0;
    let itemEnd = -1;
    while (cursor < lexicalView.length) {
      const token = lexicalView.at(cursor);
      if (token === "(") parentheses += 1;
      else if (token === ")") parentheses = Math.max(0, parentheses - 1);
      else if (token === "[") brackets += 1;
      else if (token === "]") brackets = Math.max(0, brackets - 1);
      else if (parentheses === 0 && brackets === 0 && token === ";") {
        itemEnd = cursor + 1;
        break;
      } else if (parentheses === 0 && brackets === 0 && token === "{") {
        let depth = 1;
        cursor += 1;
        while (cursor < lexicalView.length && depth > 0) {
          if (lexicalView.at(cursor) === "{") depth += 1;
          else if (lexicalView.at(cursor) === "}") depth -= 1;
          cursor += 1;
        }
        itemEnd = cursor;
        break;
      }
      cursor += 1;
    }
    ranges.push([attribute.index, itemEnd < 0 ? lexicalView.length : itemEnd]);
  }

  const visible = rustText.split("");
  for (const [start, end] of ranges) {
    for (let cursor = start; cursor < end; cursor += 1) {
      if (visible[cursor] !== "\n" && visible[cursor] !== "\r") visible[cursor] = " ";
    }
  }
  return visible.join("");
}

const unclassified = [];
for (const candidate of candidates) {
  const file = candidate.file;
  if (file.startsWith("crates/nimbus-testing/") || explicitTestFiles.has(file)) continue;
  const source = withoutCfgTestItems(candidate.source ?? fs.readFileSync(file, "utf8"));
  if (!authorityPatterns.some(pattern => pattern.test(source))) continue;
  if (!classifiedPaths.has(file)) unclassified.push(file);
}

const injected = process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
  ? process.env.NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED
  : "";
if (injected) unclassified.push(injected);

if (unclassified.length) {
  errors.push("unclassified production bind/allocation authority: " + [...new Set(unclassified)].sort().join(", "));
}
if (inventory.summary?.unclassified_production_sites !== 0) {
  errors.push("inventory summary reports " + inventory.summary?.unclassified_production_sites + " unclassified production sites");
}

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  status=$?
  error="$(<"${census_output}")"
  rm -f "${census_output}"
  if [ "${status}" -eq 0 ]; then
    pass "NNCV006" "unclassified-production-bind"
  else
    fail "NNCV006" "unclassified-production-bind" "${error}"
  fi
}

verify_dependency_baseline() {
  error="$(
    node - "${DEPENDENCIES}" <<'NODE'
const fs = require("fs");
const input = process.argv[2];
const errors = [];
if (!fs.existsSync(input)) {
  process.stdout.write("dependency baseline missing: " + input);
  process.exit(1);
}
let graph;
try {
  graph = JSON.parse(fs.readFileSync(input, "utf8"));
} catch (error) {
  process.stdout.write("dependency baseline invalid: " + error.message);
  process.exit(1);
}
const profiles = graph.profiles || [];
const names = new Set();
for (const profile of profiles) {
  if (!profile.name || names.has(profile.name)) errors.push("missing or duplicate profile name: " + (profile.name || "<empty>"));
  names.add(profile.name);
  if (!Array.isArray(profile.edges)) errors.push((profile.name || "<unknown>") + " lacks edges");
  if (!Array.isArray(profile.cycles)) errors.push((profile.name || "<unknown>") + " lacks cycles");
  else if (profile.cycles.length) errors.push(profile.name + " contains " + profile.cycles.length + " dependency cycle(s)");
}
for (const fragment of ["normal", "dev", "all-feature"]) {
  if (![...names].some(name => name.includes(fragment))) errors.push("missing " + fragment + " dependency profile");
}
process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV007" "dependency-profiles-acyclic"
  else
    fail "NNCV007" "dependency-profiles-acyclic" "${error}"
  fi
}

verify_checkpoint_ledger() {
  error="$(
    node - "${PLAN}" <<'NODE'
const fs = require("fs");
const {spawnSync} = require("child_process");
const input = process.argv[2];
if (!fs.existsSync(input)) {
  process.stdout.write("plan missing: " + input);
  process.exit(1);
}
const text = fs.readFileSync(input, "utf8");
const marker = "## Item Checkpoint Ledger";
const offset = text.indexOf(marker);
const errors = [];
if (offset < 0) {
  process.stdout.write("Item Checkpoint Ledger heading missing");
  process.exit(1);
}
const itemPattern = /^\| (NNC\d+\.\d+[a-z]?) \|/gm;
const planned = [...text.slice(0, offset).matchAll(itemPattern)].map(match => match[1]);
const tick = String.fromCharCode(96);
const rows = text.slice(offset).split("\n")
  .filter(line => /^\| NNC\d+\.\d+[a-z]? \|/.test(line))
  .map(line => line.split("|").slice(1, -1).map(cell => cell.trim()))
  .map(cells => ({id: cells[0], status: cells[1].replaceAll(tick, ""), evidence: cells.slice(2).join("|").trim()}));
const unique = values => new Set(values).size === values.length;
if (!planned.length) errors.push("no implementation-band items found");
if (!unique(planned)) errors.push("duplicate implementation-band item IDs");
if (!unique(rows.map(row => row.id))) errors.push("duplicate checkpoint-ledger item IDs");
const plannedSet = new Set(planned);
const ledgerSet = new Set(rows.map(row => row.id));
const missing = planned.filter(item => !ledgerSet.has(item));
const extra = rows.map(row => row.id).filter(item => !plannedSet.has(item));
if (missing.length || extra.length) errors.push("band/ledger mismatch: missing=" + (missing.join(",") || "<none>") + " extra=" + (extra.join(",") || "<none>"));
const inProgress = rows.filter(row => row.status === "in_progress");
if (inProgress.length !== 1) errors.push("expected exactly one in_progress row, found " + inProgress.length);
for (const row of rows) {
  if (!["done", "in_progress", "todo"].includes(row.status)) errors.push(row.id + " has invalid status " + row.status);
  if (row.status === "done" && (!row.evidence.trim() || row.evidence.trim() === "—")) errors.push(row.id + " done without evidence");
  if (row.status === "in_progress") {
    for (const field of ["Owned paths", "Last green", "Next", "Blocker"]) {
      if (!row.evidence.toLowerCase().includes(field.toLowerCase())) errors.push(row.id + " recovery checkpoint lacks " + field);
    }
  }
}
const currentItemLine = text.split("\n").find(line => line.startsWith("| Current item |"));
const currentItem = currentItemLine?.split("|")[2]?.trim().replaceAll(tick, "");
if (!currentItem || !inProgress[0] || !currentItem.startsWith(inProgress[0].id + " ")) {
  errors.push("Recovery Header current item does not match the in_progress ledger row");
}
const checkpointLine = text.split("\n").find(line => line.startsWith("| Last checkpoint commit |"));
const checkpointHashes = checkpointLine?.match(/\b[0-9a-f]{40}\b/g) || [];
if (checkpointHashes.length !== 1) {
  errors.push("Recovery Header must contain exactly one full Last checkpoint commit hash");
} else {
  const checkpoint = checkpointHashes[0];
  const resolution = spawnSync("git", ["cat-file", "-e", checkpoint + "^{commit}"], {
    stdio: "ignore",
  });
  if (resolution.status !== 0) {
    errors.push("Recovery Header Last checkpoint commit does not resolve: " + checkpoint);
  }
}
process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV008" "checkpoint-ledger-recoverable"
  else
    fail "NNCV008" "checkpoint-ledger-recoverable" "${error}"
  fi
}

verify_routing_owner() {
  error="$(
    node - "${PLAN}" "${PLAN_INDEX}" <<'NODE'
const fs = require("fs");
const [planPath, indexPath] = process.argv.slice(2);
const errors = [];
if (!fs.existsSync(planPath)) errors.push("plan missing: " + planPath);
if (!fs.existsSync(indexPath)) errors.push("plan index missing: " + indexPath);
if (!errors.length) {
  const plan = fs.readFileSync(planPath, "utf8");
  const index = fs.readFileSync(indexPath, "utf8");
  const normalizedIndex = index.replace(/\s+/g, " ");
  const tick = String.fromCharCode(96);
  const statusLine = plan.split("\n").find(line => line.startsWith("Status: "));
  const status = statusLine?.slice("Status: ".length).replaceAll(tick, "");
  if (!index.includes("nimbus-network-control-plane-plan.md")) errors.push("plan index does not route the canonical network owner");
  const expectedRoute = "nimbus-network-control-plane-plan.md" + tick + " - " + tick + status + tick;
  if (!status || !normalizedIndex.includes(expectedRoute)) {
    errors.push("plan index status does not match canonical status: " + (status || "<missing>"));
  }
  const ownerClaims = (index.match(/nimbus-network-control-plane-plan\.md/g) || []).length;
  if (ownerClaims !== 1) errors.push("plan index contains " + ownerClaims + " network-plan routes; expected exactly one");
}
process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
NODE
  )"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV009" "sole-plan-routing-owner"
  else
    fail "NNCV009" "sole-plan-routing-owner" "${error}"
  fi
}

verify_foundation_invariants() {
  metadata_error="$(
    cargo metadata --no-deps --format-version 1 2>/dev/null |
      node -e '
        let text = "";
        process.stdin.on("data", chunk => text += chunk);
        process.stdin.on("end", () => {
          let metadata;
          try { metadata = JSON.parse(text); }
          catch (error) {
            process.stdout.write("cargo metadata invalid: " + error.message);
            process.exit(1);
          }
          const runtime = metadata.packages.find(pkg => pkg.name === "nimbus-runtime");
          if (!runtime) {
            process.stdout.write("nimbus-runtime missing from cargo metadata");
            process.exit(1);
          }
          const workspaceNames = new Set(metadata.packages.map(pkg => pkg.name));
          const workspaceEdges = runtime.dependencies
            .filter(dep => dep.source === null && workspaceNames.has(dep.name))
            .map(dep => dep.name);
          if (workspaceEdges.length) {
            process.stdout.write("nimbus-runtime workspace dependencies: " + workspaceEdges.join(", "));
            process.exit(1);
          }
        });
      '
  )"
  metadata_status=$?
  core_io="$(
    rg -n \
      -e '\b(?:TcpListener|TcpStream|UdpSocket)::' \
      -e '\b(?:File|OpenOptions)::(?:open|create|new)' \
      -e '\bstd::fs::' \
      -e '\btokio::(?:fs|net)::' \
      "${CORE_SOURCE_ROOT}" 2>&1
  )"
  core_scan_status=$?
  if [ "${core_scan_status}" -gt 1 ]; then
    fail "NNCV010" "core-runtime-foundation-invariants" "nimbus-core source scan failed: ${core_io}"
  elif [ "${metadata_status}" -eq 0 ] && [ -z "${core_io}" ]; then
    pass "NNCV010" "core-runtime-foundation-invariants"
  else
    fail "NNCV010" "core-runtime-foundation-invariants" "${metadata_error}${metadata_error:+; }${core_io}"
  fi
}

verify_portable_vocabulary_owner() {
  legacy="$(
    rg -n \
      -e 'pub struct NetworkSegment' \
      -e 'trait NetworkSegmentAllocator' \
      -e 'pub (enum|struct) PublishedEndpoint' \
      crates/nimbus-core/src \
      crates/nimbus-sandbox/src 2>&1
  )"
  scan_status=$?
  required_missing=""
  if [ ! -d "crates/nimbus-network/src" ]; then
    required_missing="nimbus-network portable vocabulary owner is absent"
  elif ! rg -q 'NetworkAttachmentId' crates/nimbus-network/src; then
    required_missing="nimbus-network lacks NetworkAttachmentId"
  fi
  if [ "${scan_status}" -gt 1 ]; then
    fail "NNCV011" "single-portable-vocabulary-owner" "source scan failed: ${legacy}"
  elif [ -z "${legacy}" ] && [ -z "${required_missing}" ]; then
    pass "NNCV011" "single-portable-vocabulary-owner"
  else
    fail "NNCV011" "single-portable-vocabulary-owner" "${required_missing}${required_missing:+; }${legacy}"
  fi
}

verify_forbidden_network_dependencies_effects() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV012" "forbidden-network-dependencies-effects" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" forbidden-dependencies-effects 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV012" "forbidden-network-dependencies-effects"
  else
    fail "NNCV012" "forbidden-network-dependencies-effects" "${error}"
  fi
}

verify_single_network_definition_owner() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV013" "single-network-definition-owner" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" single-definition-owner 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV013" "single-network-definition-owner"
  else
    fail "NNCV013" "single-network-definition-owner" "${error}"
  fi
}

verify_address_is_not_network_identity() {
  if [ ! -f "${SOURCE_CONTRACT_HELPER}" ]; then
    fail "NNCV014" "address-is-not-network-identity" "missing ${SOURCE_CONTRACT_HELPER}"
    return
  fi
  error="$(node "${SOURCE_CONTRACT_HELPER}" address-is-not-identity 2>&1)"
  status=$?
  if [ "${status}" -eq 0 ]; then
    pass "NNCV014" "address-is-not-network-identity"
  else
    fail "NNCV014" "address-is-not-network-identity" "${error}"
  fi
}

run_self_test() {
  temporary="$(mktemp -d "${TMPDIR:-/tmp}/nimbus-network-verifier-self-test.XXXXXX")" || {
    printf 'SELFTEST FAIL unable to create temporary directory\n'
    exit 1
  }
  trap 'rm -rf "${temporary}"' EXIT
  script="${REPO_ROOT}/scripts/verify-nimbus-network-control-plane.sh"
  self_fail=0

  if NIMBUS_NETWORK_VERIFY_PLAN="${temporary}/missing-plan.md" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-plan.out" 2>&1; then
    printf 'SELFTEST FAIL missing plan unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV001 plan-in-HEAD' "${temporary}/missing-plan.out" ||
    grep -q '^PASS NNCV001 plan-in-HEAD' "${temporary}/missing-plan.out"; then
    printf 'SELFTEST FAIL missing plan did not produce an exclusive NNCV001 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing plan fails closed as NNCV001\n'
  fi

  if NIMBUS_NETWORK_VERIFY_INVENTORY="${temporary}/missing-inventory.json" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-inventory.out" 2>&1; then
    printf 'SELFTEST FAIL missing inventory unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV002 required-baseline-inputs' "${temporary}/missing-inventory.out" ||
    grep -q '^PASS NNCV002 required-baseline-inputs' "${temporary}/missing-inventory.out"; then
    printf 'SELFTEST FAIL missing inventory did not produce an exclusive NNCV002 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing inventory fails closed as NNCV002\n'
  fi

  if NIMBUS_NETWORK_VERIFY_DEPENDENCIES="${temporary}/missing-dependencies.json" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-dependencies.out" 2>&1; then
    printf 'SELFTEST FAIL missing dependency baseline unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV002 required-baseline-inputs' "${temporary}/missing-dependencies.out" ||
    grep -q '^PASS NNCV002 required-baseline-inputs' "${temporary}/missing-dependencies.out"; then
    printf 'SELFTEST FAIL missing dependency baseline did not produce an exclusive NNCV002 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing dependency baseline fails closed as NNCV002\n'
  fi

  invalid_checkpoint_plan="${temporary}/invalid-checkpoint-plan.md"
  node - "${PLAN}" "${invalid_checkpoint_plan}" <<'NODE'
const fs = require("fs");
const [source, target] = process.argv.slice(2);
const text = fs.readFileSync(source, "utf8");
const invalid = text.replace(
  /(\| Last checkpoint commit \| `)[0-9a-f]{40}/,
  "$1" + "0".repeat(40),
);
if (invalid === text) process.exit(1);
fs.writeFileSync(target, invalid);
NODE
  if NIMBUS_NETWORK_VERIFY_PLAN="${invalid_checkpoint_plan}" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/invalid-checkpoint.out" 2>&1; then
    printf 'SELFTEST FAIL nonexistent checkpoint unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV008 checkpoint-ledger-recoverable' "${temporary}/invalid-checkpoint.out" ||
    grep -q '^PASS NNCV008 checkpoint-ledger-recoverable' "${temporary}/invalid-checkpoint.out" ||
    ! grep -q 'Last checkpoint commit does not resolve: 0000000000000000000000000000000000000000' "${temporary}/invalid-checkpoint.out"; then
    printf 'SELFTEST FAIL nonexistent checkpoint did not produce the exact exclusive NNCV008 diagnostic\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS nonexistent checkpoint fails closed as NNCV008\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED="synthetic-unclassified.rs:1:TcpListener::bind" \
    "${script}" >"${temporary}/unclassified.out" 2>&1; then
    printf 'SELFTEST FAIL injected unclassified bind unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/unclassified.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/unclassified.out"; then
    printf 'SELFTEST FAIL injected bind did not produce an exclusive NNCV006 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected bind fails closed as NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nuse std::net::TcpListener;\nfn production_authority() { TcpListener::bind("127.0.0.1:0"); }\n' \
    "${script}" >"${temporary}/production-after-test-cfg.out" 2>&1; then
    printf 'SELFTEST FAIL production bind after test-only item unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/production-after-test-cfg.out" ||
    grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/production-after-test-cfg.out"; then
    printf 'SELFTEST FAIL production bind after test-only item escaped NNCV006\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS production bind after test-only item fails as NNCV006\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE=$'#[cfg(test)]\nmod tests { fn listener_fixture() { TcpListener::bind("127.0.0.1:0"); } }\nfn production_without_bind() {}\n' \
    "${script}" >"${temporary}/test-module-bind.out" 2>&1 || true
  if ! grep -q '^PASS NNCV006 unclassified-production-bind' "${temporary}/test-module-bind.out" ||
    grep -q '^FAIL NNCV006 unclassified-production-bind' "${temporary}/test-module-bind.out"; then
    printf 'SELFTEST FAIL cfg(test) module bind was misclassified as production\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cfg(test) module bind remains test-only for NNCV006\n'
  fi

  if NIMBUS_NETWORK_VERIFY_CORE_SCAN_ROOT="${temporary}/missing-core-source" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-core-source.out" 2>&1; then
    printf 'SELFTEST FAIL missing core source unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV010 core-runtime-foundation-invariants' "${temporary}/missing-core-source.out" ||
    grep -q '^PASS NNCV010 core-runtime-foundation-invariants' "${temporary}/missing-core-source.out"; then
    printf 'SELFTEST FAIL missing core source did not produce an exclusive NNCV010 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing core source fails closed as NNCV010\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='nimbus-tenant' \
    "${script}" >"${temporary}/forbidden-workspace-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected upper workspace dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-workspace-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-workspace-dependency.out"; then
    printf 'SELFTEST FAIL upper workspace dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS upper workspace dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='axum' \
    "${script}" >"${temporary}/forbidden-transport-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected transport dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-transport-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-transport-dependency.out"; then
    printf 'SELFTEST FAIL transport dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS transport dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_DEPENDENCY='aws-sdk-ec2' \
    "${script}" >"${temporary}/forbidden-cloud-dependency.out" 2>&1; then
    printf 'SELFTEST FAIL injected cloud SDK dependency unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-cloud-dependency.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-cloud-dependency.out"; then
    printf 'SELFTEST FAIL cloud SDK dependency did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS cloud SDK dependency fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT='fn bind_effect() { std::net::TcpListener::bind("127.0.0.1:0"); }' \
    "${script}" >"${temporary}/forbidden-effect.out" 2>&1; then
    printf 'SELFTEST FAIL injected provider effect unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-effect.out" ||
    grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/forbidden-effect.out"; then
    printf 'SELFTEST FAIL injected provider effect did not produce an exclusive NNCV012 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected provider effect fails closed as NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_DUPLICATE_DEFINITION='pub struct NetworkPlan;' \
    "${script}" >"${temporary}/duplicate-definition.out" 2>&1; then
    printf 'SELFTEST FAIL injected duplicate definition unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV013 single-network-definition-owner' "${temporary}/duplicate-definition.out" ||
    grep -q '^PASS NNCV013 single-network-definition-owner' "${temporary}/duplicate-definition.out"; then
    printf 'SELFTEST FAIL injected duplicate definition did not produce an exclusive NNCV013 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected duplicate definition fails closed as NNCV013\n'
  fi

  if NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_ADDRESS_IDENTITY='pub struct NetworkSegmentId(Cidr);' \
    "${script}" >"${temporary}/address-identity.out" 2>&1; then
    printf 'SELFTEST FAIL injected address identity unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV014 address-is-not-network-identity' "${temporary}/address-identity.out" ||
    grep -q '^PASS NNCV014 address-is-not-network-identity' "${temporary}/address-identity.out"; then
    printf 'SELFTEST FAIL injected address identity did not produce an exclusive NNCV014 failure\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS injected address identity fails closed as NNCV014\n'
  fi

  NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    NIMBUS_NETWORK_VERIFY_TEST_FORBIDDEN_EFFECT=$'// TcpListener::bind is documentation only.\nconst NOTE: &str = "std::net::TcpListener::bind";\nfn pure_contract() {}\n' \
    "${script}" >"${temporary}/non-code-effect-terms.out" 2>&1 || true
  if ! grep -q '^PASS NNCV012 forbidden-network-dependencies-effects' "${temporary}/non-code-effect-terms.out" ||
    grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/non-code-effect-terms.out"; then
    printf 'SELFTEST FAIL comment/string provider terms were misclassified as effects\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS comment/string provider terms remain non-effects for NNCV012\n'
  fi

  if NIMBUS_NETWORK_VERIFY_NETWORK_SCAN_ROOT="${temporary}/missing-network-source" \
    NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD=1 \
    "${script}" >"${temporary}/missing-network-source.out" 2>&1; then
    printf 'SELFTEST FAIL missing network source unexpectedly exited zero\n'
    self_fail=$((self_fail + 1))
  elif ! grep -q '^FAIL NNCV012 forbidden-network-dependencies-effects' "${temporary}/missing-network-source.out" ||
    ! grep -q '^FAIL NNCV013 single-network-definition-owner' "${temporary}/missing-network-source.out" ||
    ! grep -q '^FAIL NNCV014 address-is-not-network-identity' "${temporary}/missing-network-source.out"; then
    printf 'SELFTEST FAIL missing network source did not fail all source-contract conditions\n'
    self_fail=$((self_fail + 1))
  else
    printf 'SELFTEST PASS missing network source fails closed for NNCV012-NNCV014\n'
  fi

  if [ "${self_fail}" -ne 0 ]; then
    printf 'self-test: %d failed\n' "${self_fail}"
    exit 1
  fi
  printf 'self-test: 16 passed, 0 failed\n'
}

if [ "${1:-}" = "--self-test" ]; then
  run_self_test
  exit 0
fi
if [ $# -ne 0 ]; then
  printf 'usage: %s [--self-test]\n' "$0" >&2
  exit 2
fi

printf 'Nimbus network control-plane static verifier\n'
printf 'Repo: %s\n\n' "${REPO_ROOT}"

require_tools
verify_plan_in_head
verify_baseline_inputs
verify_network_crate
verify_network_dependency_contract
verify_single_port_authority
verify_bind_census
verify_dependency_baseline
verify_checkpoint_ledger
verify_routing_owner
verify_foundation_invariants
verify_portable_vocabulary_owner
verify_forbidden_network_dependencies_effects
verify_single_network_definition_owner
verify_address_is_not_network_identity

printf '\nSummary: %d passed, %d failed\n' "${PASS_COUNT}" "${FAIL_COUNT}"
if [ "${FAIL_COUNT}" -ne 0 ]; then
  exit 1
fi
