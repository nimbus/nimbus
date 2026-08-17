#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import process from "node:process";

const inventoryFlag = process.argv.indexOf("--inventory");
if (inventoryFlag < 0 || !process.argv[inventoryFlag + 1]) {
  process.stderr.write(
    "usage: verify-nimbus-network-bind-census.mjs --inventory <path> " +
      "[--print-candidates|--print-risks]\n",
  );
  process.exit(2);
}

const inventoryPath = process.argv[inventoryFlag + 1];
const printCandidates = process.argv.includes("--print-candidates");
const printRisks = process.argv.includes("--print-risks");
const printComposition = process.argv.includes("--print-composition");
const focusedBindChild =
  process.env.NIMBUS_NETWORK_VERIFY_FOCUSED_BIND_CHILD === "1";
const focusedBindFixture =
  process.env.NIMBUS_NETWORK_VERIFY_TEST_RUST_FIXTURE ||
  process.env.NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED;
if (
  focusedBindChild &&
  (process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD !== "1" ||
    !focusedBindFixture)
) {
  process.stderr.write(
    "focused bind census requires a self-test child with an injected bind fixture\n",
  );
  process.exit(2);
}
const errors = [];

function readInventory() {
  if (!fs.existsSync(inventoryPath)) {
    errors.push(`bind inventory missing: ${inventoryPath}`);
    return null;
  }
  try {
    return JSON.parse(fs.readFileSync(inventoryPath, "utf8"));
  } catch (error) {
    errors.push(`bind inventory invalid: ${error.message}`);
    return null;
  }
}

function blankRange(view, start, end) {
  for (let cursor = start; cursor < end; cursor += 1) {
    if (view[cursor] !== "\n" && view[cursor] !== "\r") {
      view[cursor] = " ";
    }
  }
}

// These two lexical views are deliberately limited to proving that a named
// whole-file exemption is nested under a cfg(test) module. Rust authority
// discovery itself is owned by the structural scanner below.
function maskNonCode(rustText) {
  const lexicalView = rustText.split("");
  let cursor = 0;
  while (cursor < rustText.length) {
    if (rustText.startsWith("//", cursor)) {
      const end = rustText.indexOf("\n", cursor + 2);
      blankRange(lexicalView, cursor, end < 0 ? rustText.length : end);
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
      blankRange(lexicalView, cursor, end);
      cursor = end;
      continue;
    }

    const raw = rustText.slice(cursor).match(/^(?:br|rb|cr|r)(#*)"/);
    if (raw) {
      const terminator = `"${raw[1]}`;
      const contentStart = cursor + raw[0].length;
      const found = rustText.indexOf(terminator, contentStart);
      const end = found < 0 ? rustText.length : found + terminator.length;
      blankRange(lexicalView, cursor, end);
      cursor = end;
      continue;
    }

    const quoteOffset =
      ["b", "c"].includes(rustText[cursor]) && rustText[cursor + 1] === '"'
        ? 1
        : 0;
    if (rustText[cursor + quoteOffset] === '"') {
      let end = cursor + quoteOffset + 1;
      while (end < rustText.length) {
        if (rustText[end] === "\\") {
          end += 2;
        } else if (rustText[end] === '"') {
          end += 1;
          break;
        } else {
          end += 1;
        }
      }
      blankRange(lexicalView, cursor, end);
      cursor = end;
      continue;
    }

    if (rustText[cursor] === "'") {
      const character = rustText.slice(cursor).match(/^'(?:\\.|[^\\'\r\n])'/u);
      if (character) {
        blankRange(lexicalView, cursor, cursor + character[0].length);
        cursor += character[0].length;
        continue;
      }
    }
    cursor += 1;
  }
  return lexicalView.join("");
}

function maskCommentsPreserveStrings(rustText) {
  const semanticView = rustText.split("");
  let cursor = 0;
  while (cursor < rustText.length) {
    if (rustText.startsWith("//", cursor)) {
      const end = rustText.indexOf("\n", cursor + 2);
      blankRange(semanticView, cursor, end < 0 ? rustText.length : end);
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
      blankRange(semanticView, cursor, end);
      cursor = end;
      continue;
    }

    const raw = rustText.slice(cursor).match(/^(?:br|rb|cr|r)(#*)"/);
    if (raw) {
      const terminator = `"${raw[1]}`;
      const contentStart = cursor + raw[0].length;
      const found = rustText.indexOf(terminator, contentStart);
      cursor = found < 0 ? rustText.length : found + terminator.length;
      continue;
    }

    const quoteOffset =
      ["b", "c"].includes(rustText[cursor]) && rustText[cursor + 1] === '"'
        ? 1
        : 0;
    if (rustText[cursor + quoteOffset] === '"') {
      let end = cursor + quoteOffset + 1;
      while (end < rustText.length) {
        if (rustText[end] === "\\") end += 2;
        else if (rustText[end] === '"') {
          end += 1;
          break;
        } else end += 1;
      }
      cursor = end;
      continue;
    }

    if (rustText[cursor] === "'") {
      const character = rustText.slice(cursor).match(/^'(?:\\.|[^\\'\r\n])'/u);
      if (character) {
        cursor += character[0].length;
        continue;
      }
    }
    cursor += 1;
  }
  return semanticView.join("");
}

function normalizedPath(value) {
  return value.split(path.sep).join("/");
}

function escapeRegExp(value) {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

function validateExemptionPolicy(inventory) {
  const exemptions = inventory.non_production_exemptions ?? [];
  const pathPatterns = new Set(
    exemptions
      .filter((entry) => entry.mechanism === "path-convention")
      .flatMap((entry) => entry.patterns ?? []),
  );
  const requiredPatterns = new Set([
    "crates/**/tests/**",
    "crates/**/tests.rs",
    "crates/**/benches/**",
  ]);
  for (const pattern of requiredPatterns) {
    if (!pathPatterns.has(pattern)) {
      errors.push(`bind inventory lacks path exemption: ${pattern}`);
    }
  }
  for (const pattern of pathPatterns) {
    if (!requiredPatterns.has(pattern)) {
      errors.push(`unsupported broad path exemption: ${pattern}`);
    }
  }

  const cfgTestItems = exemptions.filter(
    (entry) => entry.mechanism === "cfg-test-item",
  );
  if (cfgTestItems.length !== 1) {
    errors.push(
      "bind inventory must declare exactly one cfg-test-item exemption",
    );
  }

  const testSupport = exemptions.filter(
    (entry) => entry.mechanism === "test-support-crate",
  );
  if (
    testSupport.length !== 1 ||
    testSupport[0].path_prefix !== "crates/nimbus-testing/"
  ) {
    errors.push(
      "bind inventory test-support exemption must be exactly crates/nimbus-testing/",
    );
  }
}

function validatePathOwnedTestModules(inventory) {
  const exemptions = new Set();
  const evidenceRows = (inventory.non_production_exemptions ?? [])
    .filter((entry) => entry.mechanism === "path-owned-test-module")
    .flatMap((entry) => entry.files ?? []);
  const evidenceByPath = new Map(
    evidenceRows.map((evidence) => [
      normalizedPath(path.normalize(evidence.path ?? "")),
      evidence,
    ]),
  );

  function exactExplicitModuleLink(evidence) {
    if (
      !evidence ||
      !fs.existsSync(evidence.path) ||
      !fs.existsSync(evidence.declared_from)
    ) {
      return false;
    }
    const relativePath = normalizedPath(
      path.relative(path.dirname(evidence.declared_from), evidence.path),
    );
    if (relativePath.startsWith("../") || path.isAbsolute(relativePath)) {
      return false;
    }
    const declaration = maskCommentsPreserveStrings(
      fs.readFileSync(evidence.declared_from, "utf8"),
    );
    const pattern = new RegExp(
      `#\\s*\\[\\s*path\\s*=\\s*"${escapeRegExp(relativePath)}"\\s*\\]\\s*` +
        `mod\\s+${escapeRegExp(evidence.module)}\\s*;`,
      "g",
    );
    return pattern.test(declaration);
  }

  function conventionalModuleTargets(evidence) {
    const declaration = normalizedPath(path.normalize(evidence.declared_from));
    const directory = path.dirname(declaration);
    const basename = path.basename(declaration);
    const parentEvidence = evidenceByPath.get(declaration);
    const moduleDirectory =
      basename === "lib.rs" ||
      basename === "main.rs" ||
      basename === "mod.rs" ||
      exactExplicitModuleLink(parentEvidence)
        ? directory
        : path.join(directory, path.basename(declaration, ".rs"));
    return new Set([
      normalizedPath(path.join(moduleDirectory, `${evidence.module}.rs`)),
      normalizedPath(path.join(moduleDirectory, evidence.module, "mod.rs")),
    ]);
  }

  for (const evidence of evidenceRows) {
    for (const field of [
      "path",
      "declared_from",
      "cfg_owner",
      "owner_module",
      "module",
    ]) {
      if (typeof evidence[field] !== "string" || !evidence[field].trim()) {
        errors.push(`path-owned test exemption lacks ${field}`);
      }
    }
    if (
      !fs.existsSync(evidence.path) ||
      !fs.existsSync(evidence.declared_from) ||
      !fs.existsSync(evidence.cfg_owner)
    ) {
      errors.push(
        `path-owned test exemption evidence missing: ${evidence.path}`,
      );
      continue;
    }

    const normalizedEvidencePath = normalizedPath(
      path.normalize(evidence.path),
    );
    const normalizedDeclaration = normalizedPath(
      path.normalize(evidence.declared_from),
    );
    const normalizedOwner = normalizedPath(path.normalize(evidence.cfg_owner));
    const ownerStem = path.basename(normalizedOwner, ".rs");
    const expectedNestedDeclaration = normalizedPath(
      path.join(
        path.dirname(normalizedOwner),
        ownerStem,
        `${evidence.owner_module}.rs`,
      ),
    );
    const relativeEvidencePath = normalizedPath(
      path.relative(
        path.dirname(normalizedDeclaration),
        normalizedEvidencePath,
      ),
    );
    if (
      relativeEvidencePath.startsWith("../") ||
      path.isAbsolute(relativeEvidencePath)
    ) {
      errors.push(
        `path-owned test exemption module linkage is inconsistent: ${evidence.path}`,
      );
      continue;
    }

    const declaration = fs.readFileSync(evidence.declared_from, "utf8");
    const declarationCode = maskNonCode(declaration);
    const declarationSemantic = maskCommentsPreserveStrings(declaration);
    const explicitModulePattern = new RegExp(
      `#\\s*\\[\\s*path\\s*=\\s*"${escapeRegExp(relativeEvidencePath)}"\\s*\\]\\s*` +
        `mod\\s+${escapeRegExp(evidence.module)}\\s*;`,
      "g",
    );
    const anyExplicitModulePattern = new RegExp(
      `#\\s*\\[\\s*path\\s*=\\s*"[^"]+"\\s*\\]\\s*` +
        `mod\\s+${escapeRegExp(evidence.module)}\\s*;`,
      "g",
    );
    const conventionalModulePattern = new RegExp(
      `\\bmod\\s+${escapeRegExp(evidence.module)}\\s*;`,
      "g",
    );
    let hasExplicitPathOverride = false;
    let explicitOverrideMatch;
    while (
      (explicitOverrideMatch =
        anyExplicitModulePattern.exec(declarationSemantic)) !== null
    ) {
      if (declarationCode[explicitOverrideMatch.index] === "#") {
        hasExplicitPathOverride = true;
        break;
      }
    }
    let linkedDeclaration = false;
    for (const modulePattern of [explicitModulePattern]) {
      let moduleMatch;
      while ((moduleMatch = modulePattern.exec(declarationSemantic)) !== null) {
        const codeStart = declarationCode[moduleMatch.index];
        if (codeStart === "#" || codeStart === "m") {
          linkedDeclaration = true;
          break;
        }
      }
      if (linkedDeclaration) break;
    }
    if (
      !linkedDeclaration &&
      !hasExplicitPathOverride &&
      conventionalModuleTargets(evidence).has(normalizedEvidencePath)
    ) {
      let moduleMatch;
      while (
        (moduleMatch = conventionalModulePattern.exec(declarationSemantic)) !==
        null
      ) {
        if (declarationCode[moduleMatch.index] === "m") {
          linkedDeclaration = true;
          break;
        }
      }
    }

    const owner = maskNonCode(fs.readFileSync(evidence.cfg_owner, "utf8"));
    const cfgOwnerPattern = new RegExp(
      `#\\s*\\[\\s*cfg\\s*\\(\\s*` +
        `(?:test|all\\s*\\(\\s*test(?:\\s*,[^()]*)*\\))` +
        `\\s*\\)\\s*\\]\\s*` +
        `(?:#\\s*\\[[^\\]]+\\]\\s*)*` +
        `mod\\s+${escapeRegExp(evidence.owner_module)}\\s*;`,
    );
    const directOwner =
      (normalizedDeclaration === normalizedOwner ||
        normalizedDeclaration === expectedNestedDeclaration) &&
      cfgOwnerPattern.test(owner);
    const parentEvidence = evidenceByPath.get(normalizedDeclaration);
    const transitiveOwner =
      parentEvidence &&
      normalizedPath(path.normalize(parentEvidence.cfg_owner)) ===
        normalizedOwner &&
      parentEvidence.owner_module === evidence.owner_module;
    if (!linkedDeclaration || (!directOwner && !transitiveOwner)) {
      errors.push(
        `path-owned test exemption is not mechanically cfg(test)-owned: ${evidence.path}`,
      );
      continue;
    }
    exemptions.add(normalizedEvidencePath);
  }
  return exemptions;
}

function validScannerOccurrence(occurrence) {
  return (
    occurrence &&
    typeof occurrence.path === "string" &&
    typeof occurrence.kind === "string" &&
    typeof occurrence.symbol === "string" &&
    Number.isSafeInteger(occurrence.ordinal) &&
    occurrence.ordinal > 0 &&
    Number.isSafeInteger(occurrence.line) &&
    occurrence.line > 0
  );
}

function runStructuralScan(wholeFileExemptions) {
  const manifest = "scripts/nimbus-network-bind-census-ast/Cargo.toml";
  if (!fs.existsSync(manifest)) {
    errors.push(`bind census structural scanner manifest missing: ${manifest}`);
    return { authorities: [], risks: [], composition: [], declarations: [] };
  }

  const arguments_ = [
    "run",
    "--quiet",
    "--manifest-path",
    manifest,
    "--locked",
    "--",
    "--root",
    "crates",
  ];
  for (const excluded of [...wholeFileExemptions].sort()) {
    arguments_.push("--exclude", excluded);
  }
  const result = spawnSync("cargo", arguments_, {
    encoding: "utf8",
    env: process.env,
    maxBuffer: 64 * 1024 * 1024,
  });
  if (result.error) {
    errors.push(
      `bind census structural scanner failed to start: ${result.error.message}`,
    );
    return { authorities: [], risks: [], composition: [], declarations: [] };
  }
  if (result.status !== 0) {
    const detail = [result.stderr, result.stdout]
      .filter(Boolean)
      .join("\n")
      .trim();
    errors.push(
      `bind census structural scanner exited ${result.status}: ${detail || "<no output>"}`,
    );
    return { authorities: [], risks: [], composition: [], declarations: [] };
  }

  let output;
  try {
    output = JSON.parse(result.stdout);
  } catch (error) {
    errors.push(
      `bind census structural scanner returned invalid JSON: ${error.message}`,
    );
    return { authorities: [], risks: [], composition: [], declarations: [] };
  }
  for (const field of [
    "authorities",
    "risks",
    "composition",
    "declarations",
    "errors",
  ]) {
    if (!Array.isArray(output[field])) {
      errors.push(`bind census structural scanner lacks ${field} array`);
      output[field] = [];
    }
  }
  for (const [field, occurrences] of [
    ["authority", output.authorities],
    ["risk", output.risks],
    ["composition", output.composition],
  ]) {
    for (const occurrence of occurrences) {
      if (!validScannerOccurrence(occurrence)) {
        errors.push(
          `bind census structural scanner returned malformed ${field} occurrence`,
        );
      }
    }
  }
  for (const declaration of output.declarations) {
    if (
      !declaration ||
      typeof declaration.path !== "string" ||
      typeof declaration.name !== "string" ||
      !Number.isSafeInteger(declaration.line) ||
      declaration.line < 1
    ) {
      errors.push(
        "bind census structural scanner returned malformed declaration",
      );
    }
  }
  for (const scannerError of output.errors) {
    if (typeof scannerError !== "string" || !scannerError.trim()) {
      errors.push("bind census structural scanner returned malformed error");
    } else {
      errors.push(scannerError);
    }
  }
  return output;
}

function occurrenceKey(occurrence) {
  return [
    occurrence.path,
    occurrence.kind,
    occurrence.symbol,
    occurrence.ordinal,
  ].join("|");
}

function validateSiteLinks(inventory, classified, declarations) {
  const sites = new Map(
    (inventory.production_sites ?? []).map((site) => [site.id, site]),
  );
  for (const occurrence of classified) {
    if (occurrence.site_id === "__self_test__") continue;
    const site = sites.get(occurrence.site_id);
    if (!site) {
      errors.push(
        `authority occurrence references unknown site ${occurrence.site_id}`,
      );
      continue;
    }
    if (site.status !== "active") {
      errors.push(
        `authority occurrence references non-active site ${occurrence.site_id}`,
      );
    }
    const authorityPaths = Array.isArray(site.authority_paths)
      ? site.authority_paths
      : [site.path];
    if (!authorityPaths.includes(occurrence.path)) {
      errors.push(
        `authority occurrence path differs from site ${occurrence.site_id}:` +
          `${occurrence.path} not in ${authorityPaths.join(",")}`,
      );
    }
    if (site.verification !== "source-occurrence") {
      errors.push(
        `authority occurrence references non-occurrence site ${occurrence.site_id}`,
      );
    }
    if (
      !Array.isArray(site.authority_kinds) ||
      !site.authority_kinds.includes(occurrence.kind)
    ) {
      errors.push(
        `authority kind ${occurrence.kind} is invalid for site ${occurrence.site_id}`,
      );
    }
    if (
      !Array.isArray(site.authority_symbols) ||
      !site.authority_symbols.includes(occurrence.symbol)
    ) {
      errors.push(
        `authority symbol ${occurrence.symbol} is invalid for site ${occurrence.site_id}`,
      );
    }
  }

  for (const site of sites.values()) {
    if (site.status === "active") {
      if (!fs.existsSync(site.path)) {
        errors.push(`active bind inventory path missing: ${site.path}`);
      }
      if (site.verification === "source-occurrence") {
        if (!classified.some((entry) => entry.site_id === site.id)) {
          errors.push(`active site lacks source occurrence: ${site.id}`);
        }
      } else if (site.verification === "symbol-presence") {
        const observed = declarations.filter(
          (declaration) =>
            declaration.path === site.path &&
            declaration.name === site.declaration_name,
        );
        if (observed.length !== 1 || observed[0].line !== site.line) {
          const observedLines =
            observed.map((declaration) => declaration.line).join(",") ||
            "<missing>";
          errors.push(
            `active site declaration missing or stale: ${site.id}:` +
              `${site.declaration_name}:inventory=${site.line}:source=${observedLines}`,
          );
        }
      } else {
        errors.push(
          `active site ${site.id} has invalid verification ${site.verification ?? "<missing>"}`,
        );
      }
    } else if (site.status === "retired") {
      if (classified.some((entry) => entry.site_id === site.id)) {
        errors.push(`retired site retains source occurrence: ${site.id}`);
      }
      if (typeof site.retired_item !== "string" || !site.retired_item.trim()) {
        errors.push(`retired site lacks retired_item: ${site.id}`);
      }
    } else {
      errors.push(
        `site ${site.id} has invalid status ${site.status ?? "<missing>"}`,
      );
    }
  }
}

const inventory = readInventory();
if (!inventory) {
  process.stdout.write(errors.join("\n"));
  process.exit(1);
}

if (!focusedBindChild) validateExemptionPolicy(inventory);
const wholeFileExemptions = focusedBindChild
  ? new Set()
  : validatePathOwnedTestModules(inventory);
const scan = runStructuralScan(wholeFileExemptions);
const occurrences = scan.authorities;
const risks = scan.risks;
const composition = scan.composition;
const declarations = scan.declarations;

const tcpBindCount = occurrences.filter(
  (occurrence) => occurrence.kind === "tcp-bind",
).length;
const udpBindCount = occurrences.filter(
  (occurrence) => occurrence.kind === "udp-bind",
).length;
const localIpcCount = occurrences.filter((occurrence) =>
  occurrence.kind.startsWith("unix-"),
).length;
if (!focusedBindChild) {
  for (const [label, inventoryValue, observedValue] of [
    [
      "authority occurrence",
      inventory.summary?.authority_occurrences,
      occurrences.length,
    ],
    [
      "TCP bind",
      inventory.summary?.production_tcp_bind_occurrences,
      tcpBindCount,
    ],
    [
      "UDP bind",
      inventory.summary?.production_udp_bind_occurrences,
      udpBindCount,
    ],
    ["local IPC", inventory.summary?.local_ipc_occurrences, localIpcCount],
    [
      "non-authority",
      inventory.summary?.non_authority_occurrences,
      risks.length,
    ],
  ]) {
    if (inventoryValue !== observedValue) {
      errors.push(
        `inventory ${label} summary is stale: inventory=${inventoryValue}:source=${observedValue}`,
      );
    }
  }
}

if (printCandidates) {
  process.stdout.write(`${JSON.stringify(occurrences, null, 2)}\n`);
  process.exit(errors.length === 0 ? 0 : 1);
}
if (printRisks) {
  process.stdout.write(`${JSON.stringify(risks, null, 2)}\n`);
  process.exit(errors.length === 0 ? 0 : 1);
}
if (printComposition) {
  process.stdout.write(`${JSON.stringify(composition, null, 2)}\n`);
  process.exit(errors.length === 0 ? 0 : 1);
}

const classified = focusedBindChild
  ? []
  : (inventory.authority_occurrences ?? []).map((entry) => ({ ...entry }));
const swapSiteIds =
  process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
    ? process.env.NIMBUS_NETWORK_VERIFY_TEST_SWAP_SITE_IDS
    : "";
if (swapSiteIds) {
  const [left, right] = swapSiteIds.split(",");
  for (const occurrence of classified) {
    if (occurrence.site_id === left) occurrence.site_id = right;
    else if (occurrence.site_id === right) occurrence.site_id = left;
  }
}

const corruptDeclarationSite =
  process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
    ? process.env.NIMBUS_NETWORK_VERIFY_TEST_CORRUPT_SITE_DECLARATION
    : "";
if (corruptDeclarationSite) {
  const site = (inventory.production_sites ?? []).find(
    (candidate) => candidate.id === corruptDeclarationSite,
  );
  if (site) site.line += 1;
}

const injectedClassifiedOccurrence =
  process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
    ? process.env.NIMBUS_NETWORK_VERIFY_TEST_CLASSIFIED_OCCURRENCE
    : "";
if (injectedClassifiedOccurrence) {
  const [occurrencePath, kind, symbol, ordinal] =
    injectedClassifiedOccurrence.split("|");
  classified.push({
    site_id: "__self_test__",
    path: occurrencePath,
    kind,
    symbol,
    ordinal: Number.parseInt(ordinal, 10),
    line: 1,
  });
}

if (!focusedBindChild) validateSiteLinks(inventory, classified, declarations);
const classifiedByKey = new Map();
for (const occurrence of classified) {
  const key = occurrenceKey(occurrence);
  if (classifiedByKey.has(key)) {
    errors.push(`duplicate authority occurrence classification: ${key}`);
  } else {
    classifiedByKey.set(key, occurrence);
  }
}

const observedByKey = new Map();
for (const occurrence of occurrences) {
  const key = occurrenceKey(occurrence);
  if (observedByKey.has(key)) {
    errors.push(`duplicate observed authority occurrence: ${key}`);
  } else {
    observedByKey.set(key, occurrence);
  }
  const classification = classifiedByKey.get(key);
  if (!classification) {
    errors.push(
      `unclassified production bind/allocation authority: ${key}:line=${occurrence.line}`,
    );
  } else if (classification.line !== occurrence.line) {
    errors.push(
      `stale authority occurrence line: ${key}:inventory=${classification.line}:source=${occurrence.line}`,
    );
  }
}
for (const key of classifiedByKey.keys()) {
  if (!observedByKey.has(key)) {
    errors.push(`stale authority occurrence classification: ${key}`);
  }
}

const riskClassifications = focusedBindChild
  ? []
  : (inventory.non_authority_occurrences ?? []);
const riskClassifiedByKey = new Map();
for (const classification of riskClassifications) {
  const key = occurrenceKey(classification);
  if (
    typeof classification.reason !== "string" ||
    !classification.reason.trim()
  ) {
    errors.push(`non-authority occurrence lacks reason: ${key}`);
  }
  if (riskClassifiedByKey.has(key)) {
    errors.push(`duplicate non-authority occurrence classification: ${key}`);
  } else {
    riskClassifiedByKey.set(key, classification);
  }
}

const observedRiskByKey = new Map();
for (const risk of risks) {
  const key = occurrenceKey(risk);
  if (observedRiskByKey.has(key)) {
    errors.push(`duplicate observed non-authority occurrence: ${key}`);
  } else {
    observedRiskByKey.set(key, risk);
  }
  const classification = riskClassifiedByKey.get(key);
  if (!classification) {
    errors.push(
      `unclassified ambiguous bind/adoption operation: ${key}:line=${risk.line}`,
    );
  } else if (classification.line !== risk.line) {
    errors.push(
      `stale non-authority occurrence line: ${key}:inventory=${classification.line}:source=${risk.line}`,
    );
  }
}
for (const key of riskClassifiedByKey.keys()) {
  if (!observedRiskByKey.has(key)) {
    errors.push(`stale non-authority occurrence classification: ${key}`);
  }
}

const injectedUnclassified =
  process.env.NIMBUS_NETWORK_VERIFY_SELF_TEST_CHILD === "1"
    ? process.env.NIMBUS_NETWORK_VERIFY_TEST_UNCLASSIFIED
    : "";
if (injectedUnclassified) {
  errors.push(
    `unclassified production bind/allocation authority: ${injectedUnclassified}`,
  );
}
if (!focusedBindChild && inventory.summary?.unclassified_production_sites !== 0) {
  errors.push(
    `inventory summary reports ${inventory.summary?.unclassified_production_sites} unclassified production sites`,
  );
}

process.stdout.write(errors.join("\n"));
process.exit(errors.length === 0 ? 0 : 1);
