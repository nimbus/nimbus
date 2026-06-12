// Prove the embedded package payload is dependency-closed for offline install
// (BPD1, completion-gate condition 6). Fails — loudly — if any provisioned or
// co-provisioned package declares a dependency that is not satisfiable offline.
//
// A `dependencies` entry is allowed only if it names another embedded root.
// A `peerDependencies` entry is allowed only if it names an embedded root OR is
// an explicitly out-of-contract, developer-supplied peer (ALLOWED_PEERS).
// Installer-active fields (`devDependencies`, `optionalDependencies`, bundled
// dependency metadata, and lifecycle scripts) are forbidden in staged runtime
// manifests because they can trigger registry probes or package-managed code
// during an offline `file:` install.
//
// Run after staging: `node scripts/stage-embedded-packages.mjs` then this.

import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const REPO_ROOT = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");
const EMBED_DIR = path.join(REPO_ROOT, "crates", "nimbus-assets", "embedded", "packages");
const MANIFEST = path.join(EMBED_DIR, "manifest.json");

// Out-of-contract, developer-supplied peers documented in the plan's Offline
// contract boundaries. These are NEVER embedded; the developer's own app
// provides them (react UI deps; the AWS SDK for DynamoDB).
const ALLOWED_PEERS = new Set(["react", "react-dom", "@aws-sdk/client-dynamodb"]);

// Every Nimbus package whose dist must be provisioned.
const REQUIRED_PROVISIONED = ["convex", "@nimbus/nimbus", "firebase", "@nimbus/mongodb", "@nimbus/dynamodb"];

const errors = [];

if (!fs.existsSync(MANIFEST)) {
  console.error(`closure: missing ${path.relative(REPO_ROOT, MANIFEST)} — run scripts/stage-embedded-packages.mjs first`);
  process.exit(1);
}

const manifest = JSON.parse(fs.readFileSync(MANIFEST, "utf8"));
const embeddedNames = new Set(manifest.packages.map((p) => p.name));

// Resolve each embedded package's authoritative manifest to read its deps.
function manifestPathFor(pkg) {
  return pkg.thirdParty
    ? path.join(EMBED_DIR, pkg.dir, "package.json")
    : path.join(REPO_ROOT, "packages", pkg.sourceDir ?? pkg.dir, "dist", "package.json");
}

for (const pkg of manifest.packages) {
  const mpath = manifestPathFor(pkg);
  if (!fs.existsSync(mpath)) {
    errors.push(`${pkg.name}: missing manifest ${path.relative(REPO_ROOT, mpath)}`);
    continue;
  }
  const m = JSON.parse(fs.readFileSync(mpath, "utf8"));

  for (const dep of Object.keys(m.dependencies ?? {})) {
    if (!embeddedNames.has(dep)) {
      errors.push(
        `${pkg.name}: dependency "${dep}" is not an embedded root — it would be fetched from the registry offline`,
      );
    }
  }
  // npm installs the devDependencies of a `file:`-linked package, so any that
  // survive staging would be fetched from the registry on an offline install.
  // Staging strips them (stage-embedded-packages.mjs); fail if any remain.
  for (const dep of Object.keys(m.devDependencies ?? {})) {
    errors.push(
      `${pkg.name}: devDependency "${dep}" survives in the staged manifest — npm installs devDependencies of file: links; strip it in stage-embedded-packages.mjs`,
    );
  }
  for (const dep of Object.keys(m.optionalDependencies ?? {})) {
    errors.push(
      `${pkg.name}: optionalDependency "${dep}" survives in the staged manifest — optional deps may still trigger registry probes; strip or rewrite it in stage-embedded-packages.mjs`,
    );
  }
  for (const field of ["bundleDependencies", "bundledDependencies"]) {
    if (m[field] !== undefined) {
      errors.push(
        `${pkg.name}: ${field} survives in the staged manifest — bundled dependency metadata is outside the offline closure contract`,
      );
    }
  }
  for (const script of Object.keys(m.scripts ?? {})) {
    errors.push(
      `${pkg.name}: script "${script}" survives in the staged manifest — lifecycle scripts must not run during offline file: installs`,
    );
  }
  for (const peer of Object.keys(m.peerDependencies ?? {})) {
    if (!embeddedNames.has(peer) && !ALLOWED_PEERS.has(peer)) {
      errors.push(
        `${pkg.name}: peerDependency "${peer}" is neither embedded nor an allowed developer-supplied peer`,
      );
    }
  }
}

for (const required of REQUIRED_PROVISIONED) {
  if (!embeddedNames.has(required)) {
    errors.push(`required provisioned package "${required}" is not embedded`);
  }
}

// Every co-provisioned third-party root must be attributed in the repo NOTICE
// (G4 third-party attribution). Their embedded files also retain per-file SPDX
// headers, but the NOTICE entry is the auditable record.
const noticePath = path.join(REPO_ROOT, "NOTICE");
const notice = fs.existsSync(noticePath) ? fs.readFileSync(noticePath, "utf8") : "";
for (const pkg of manifest.packages) {
  if (pkg.thirdParty && !notice.includes(pkg.name)) {
    errors.push(`third-party root "${pkg.name}" is embedded but not attributed in NOTICE`);
  }
}

if (errors.length > 0) {
  console.error("closure: FAILED — unsupported dependencies survive:");
  for (const e of errors) console.error(`  - ${e}`);
  process.exit(1);
}

const counts = manifest.packages.reduce(
  (acc, p) => {
    acc[p.thirdParty ? "thirdParty" : "nimbus"] += 1;
    return acc;
  },
  { nimbus: 0, thirdParty: 0 },
);
console.log(
  `closure: OK — ${counts.nimbus} Nimbus + ${counts.thirdParty} co-provisioned third-party roots, all dependencies closed`,
);
process.exit(0);
