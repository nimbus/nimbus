#!/usr/bin/env node

import { spawnSync } from "node:child_process";
import crypto from "node:crypto";
import fs from "node:fs";
import os from "node:os";
import path from "node:path";
import process from "node:process";

const repoRoot = process.cwd();
const defaultBaseline =
  "docs/private/plans/proof/nimbus-network-control-plane/" +
  "nnc9.1-compiler-authority-baseline.json";
const defaultInventory =
  "docs/private/plans/proof/nimbus-network-control-plane/" +
  "nnc0.1-bind-owner-inventory.json";
const ownerPackages = [
  "nimbus-cli",
  "nimbus-kv",
  "nimbus-network",
  "nimbus-proxy",
  "nimbus-sandbox",
  "nimbus-server",
];
const callKinds = [
  "tcp-bind",
  "tcp-from-std",
  "tcp-from-raw-fd",
  "tcp-from-owned-fd",
  "tcp-from-raw-socket",
  "tcp-from-owned-socket",
  "udp-bind",
  "udp-from-std",
  "udp-from-raw-fd",
  "udp-from-owned-fd",
  "udp-from-raw-socket",
  "udp-from-owned-socket",
  "unix-bind",
  "unix-from-std",
  "unix-from-raw-fd",
  "unix-from-owned-fd",
];
const generatedScanKinds = [
  "authorities",
  "risks",
  "composition",
  "boundaries",
];
const inventoryKinds = new Set(callKinds);
const structuralScannerManifest =
  "scripts/nimbus-network-bind-census-ast/Cargo.toml";
const generatedOutputRoster = [
  "firebase_grpc.rs",
  "google.api.rs",
  "google.firestore.v1.rs",
  "google.r#type.rs",
  "google.rpc.rs",
];
const generatedIncludeRoster = [
  { file: "firebase_grpc.rs", target: "google.api.rs" },
  { file: "firebase_grpc.rs", target: "google.firestore.v1.rs" },
  { file: "firebase_grpc.rs", target: "google.r#type.rs" },
  { file: "firebase_grpc.rs", target: "google.rpc.rs" },
];
function argument(name, fallback = null) {
  const index = process.argv.indexOf(name);
  if (index < 0) return fallback;
  if (!process.argv[index + 1]) throw new Error(`${name} requires a value`);
  return process.argv[index + 1];
}

function sha256(value) {
  return crypto.createHash("sha256").update(value).digest("hex");
}

function readJson(file, label) {
  if (!fs.existsSync(file)) throw new Error(`${label} missing: ${file}`);
  const text = fs.readFileSync(file, "utf8");
  if (!text.trim()) throw new Error(`${label} empty: ${file}`);
  try {
    return { value: JSON.parse(text), text };
  } catch (error) {
    throw new Error(`${label} invalid JSON: ${file}: ${error.message}`);
  }
}

function walk(directory, accept, excluded = new Set()) {
  if (!fs.existsSync(directory)) return [];
  const found = [];
  for (const entry of fs.readdirSync(directory, { withFileTypes: true })) {
    if (excluded.has(entry.name)) continue;
    const full = path.join(directory, entry.name);
    if (entry.isDirectory()) found.push(...walk(full, accept, excluded));
    else if (entry.isFile() && accept(full)) found.push(full);
  }
  return found;
}

function relative(file) {
  return path.relative(repoRoot, file).split(path.sep).join("/");
}

function productionRustFiles() {
  return walk(
    path.join(repoRoot, "crates"),
    (file) => file.endsWith(".rs") && path.basename(file) !== "tests.rs",
    new Set(["tests", "benches", "examples", "target"]),
  ).sort();
}

function evidenceInputFiles() {
  const files = productionRustFiles();
  for (const file of walk(
    path.join(repoRoot, "crates"),
    (candidate) =>
      [".toml", ".proto", ".json"].includes(path.extname(candidate)),
    new Set(["tests", "benches", "examples", "target"]),
  )) {
    files.push(file);
  }
  for (const file of walk(
    path.join(repoRoot, "scripts", "nimbus-network-bind-census-ast"),
    (candidate) => [".rs", ".toml", ".lock"].includes(path.extname(candidate)),
    new Set(["target"]),
  )) {
    files.push(file);
  }
  for (const name of [
    "Cargo.toml",
    "Cargo.lock",
    "rust-toolchain.toml",
    "scripts/verify-nimbus-network-bind-census.mjs",
    "scripts/nimbus-network-control-plane/source-contract-scanner.mjs",
    "scripts/nimbus-network-control-plane/compiler-authority-contract.mjs",
  ]) {
    const file = path.join(repoRoot, name);
    if (fs.existsSync(file)) files.push(file);
  }
  for (const file of walk(
    path.join(repoRoot, ".cargo"),
    () => true,
    new Set(["target"]),
  )) {
    files.push(file);
  }
  return [...new Set(files)].sort((left, right) =>
    relative(left).localeCompare(relative(right)),
  );
}

function digestInputs() {
  const hash = crypto.createHash("sha256");
  const files = evidenceInputFiles();
  for (const file of files) {
    hash.update(relative(file));
    hash.update("\0");
    hash.update(fs.readFileSync(file));
    hash.update("\0");
  }
  return { sha256: hash.digest("hex"), files: files.length };
}

function commandPath(name) {
  const configured = process.env[name.toUpperCase()];
  if (configured && (path.isAbsolute(configured) || configured.includes("/"))) {
    const candidate = path.resolve(repoRoot, configured);
    if (!fs.existsSync(candidate))
      throw new Error(`${name} command does not exist: ${candidate}`);
    return candidate;
  }
  const command = configured || name;
  const result = run("/usr/bin/which", [command]);
  const candidate = result.stdout.trim();
  if (!candidate || !fs.existsSync(candidate)) {
    throw new Error(`${name} command could not be resolved: ${command}`);
  }
  return candidate;
}

function compilerEnvironment() {
  return Object.fromEntries(
    Object.entries(process.env)
      .filter(([name]) =>
        /^(?:RUSTC|RUSTFLAGS|RUSTC_BOOTSTRAP|RUSTC_WRAPPER|RUSTC_WORKSPACE_WRAPPER|CARGO_ENCODED_RUSTFLAGS|CARGO_BUILD_.+|CARGO_TARGET_.+|CARGO_PROFILE_.+|CARGO_INCREMENTAL|CARGO_CACHE_RUSTC_INFO|CC(?:_.+)?|CFLAGS(?:_.+)?|CPPFLAGS(?:_.+)?|AR(?:_.+)?|BINDGEN_EXTRA_CLANG_ARGS(?:_.+)?|MACOSX_DEPLOYMENT_TARGET|SDKROOT)$/.test(
          name,
        ),
      )
      .sort(([left], [right]) => left.localeCompare(right)),
  );
}

function cargoConfigFiles() {
  const cargoHome = process.env.CARGO_HOME
    ? path.resolve(process.env.CARGO_HOME)
    : path.join(os.homedir(), ".cargo");
  const candidates = [];
  const seen = new Set();
  const add = (label, file) => {
    if (!fs.existsSync(file) || !fs.statSync(file).isFile()) return;
    const canonical = fs.realpathSync(file);
    if (seen.has(canonical)) return;
    seen.add(canonical);
    candidates.push({
      label,
      search_order: candidates.length,
      bytes: fs.statSync(canonical).size,
      sha256: sha256(fs.readFileSync(canonical)),
    });
  };
  let directory = repoRoot;
  while (true) {
    for (const name of ["config", "config.toml"]) {
      const file = path.join(directory, ".cargo", name);
      const label =
        path.dirname(file) === cargoHome
          ? `$CARGO_HOME/${name}`
          : `${directory === repoRoot ? "$REPO" : directory}/.cargo/${name}`;
      add(label, file);
    }
    const parent = path.dirname(directory);
    if (parent === directory) break;
    directory = parent;
  }
  for (const name of ["config", "config.toml"]) {
    add(`$CARGO_HOME/${name}`, path.join(cargoHome, name));
  }
  return candidates;
}

function toolchainIdentity() {
  const rustcCommand = commandPath("rustc");
  const cargoCommand = commandPath("cargo");
  const rustcVersion = run(rustcCommand, [
    "--version",
    "--verbose",
  ]).stdout.trim();
  const host = rustcVersion
    .split("\n")
    .find((line) => line.startsWith("host: "))
    ?.slice("host: ".length);
  if (!host) throw new Error("rustc --version --verbose did not report a host");
  const sysroot = run(rustcCommand, ["--print", "sysroot"]).stdout.trim();
  const compilerBinary = path.join(
    sysroot,
    "bin",
    `rustc${process.platform === "win32" ? ".exe" : ""}`,
  );
  const cargoBinary = path.join(
    sysroot,
    "bin",
    `cargo${process.platform === "win32" ? ".exe" : ""}`,
  );
  for (const [label, file] of [
    ["rustc", compilerBinary],
    ["cargo", cargoBinary],
  ]) {
    if (!fs.existsSync(file))
      throw new Error(`${label} toolchain binary missing: ${file}`);
  }
  return {
    rustc: {
      command: rustcCommand,
      command_sha256: sha256(fs.readFileSync(rustcCommand)),
      binary: compilerBinary,
      binary_sha256: sha256(fs.readFileSync(compilerBinary)),
      version: rustcVersion,
    },
    cargo: {
      command: cargoCommand,
      command_sha256: sha256(fs.readFileSync(cargoCommand)),
      binary: cargoBinary,
      binary_sha256: sha256(fs.readFileSync(cargoBinary)),
      version: run(cargoCommand, ["-vV"]).stdout.trim(),
    },
    host,
    target: host,
    target_cfg: run(rustcCommand, ["--print", "cfg", "--target", host])
      .stdout.trim()
      .split("\n")
      .filter(Boolean)
      .sort(),
    sysroot,
    environment: compilerEnvironment(),
    cargo_configs: cargoConfigFiles(),
  };
}

function cargoRun(identity, args, options = {}) {
  const { env = {}, ...rest } = options;
  return run(identity.cargo.command, args, {
    env: {
      ...process.env,
      RUSTC: identity.rustc.command,
      ...env,
    },
    ...rest,
  });
}

function productionTargetMatrix(identity) {
  const metadata = JSON.parse(
    cargoRun(identity, [
      "metadata",
      "--locked",
      "--no-deps",
      "--format-version",
      "1",
    ]).stdout,
  );
  return ownerPackages.map((packageName) => {
    const owner = metadata.packages.find((entry) => entry.name === packageName);
    if (!owner)
      throw new Error(`cargo metadata lacks owner package ${packageName}`);
    const targets = owner.targets
      .flatMap((target) => {
        if (target.kind.includes("lib")) {
          return [{ name: target.name, kind: "lib", selector: ["--lib"] }];
        }
        if (target.kind.includes("bin")) {
          return [
            {
              name: target.name,
              kind: "bin",
              selector: ["--bin", target.name],
            },
          ];
        }
        return [];
      })
      .sort((left, right) =>
        `${left.kind}:${left.name}`.localeCompare(
          `${right.kind}:${right.name}`,
        ),
      );
    if (targets.length === 0) {
      throw new Error(`owner package has no production target: ${packageName}`);
    }
    return { package: packageName, targets };
  });
}

function runStructuralScanner(root, exclusions, identity) {
  const arguments_ = [
    "run",
    "--quiet",
    "--manifest-path",
    structuralScannerManifest,
    "--locked",
    "--",
    "--root",
    root,
  ];
  for (const exclusion of exclusions) arguments_.push("--exclude", exclusion);
  const result = cargoRun(identity, arguments_);
  let scan;
  try {
    scan = JSON.parse(result.stdout);
  } catch (error) {
    throw new Error(
      `structural scanner returned invalid JSON: ${error.message}`,
    );
  }
  if (!Array.isArray(scan.errors) || scan.errors.length !== 0) {
    throw new Error(
      `structural scanner reported errors: ${JSON.stringify(scan.errors ?? null)}`,
    );
  }
  if (!Array.isArray(scan.boundaries)) {
    throw new Error("structural scanner omitted parsed compiler boundaries");
  }
  for (const field of ["authorities", "risks", "composition"]) {
    if (!Array.isArray(scan[field])) {
      throw new Error(`structural scanner omitted ${field}`);
    }
  }
  return scan;
}

function structuralScan(inventory, identity) {
  const exclusions = [];
  for (const exemption of inventory.non_production_exemptions ?? []) {
    if (exemption.mechanism !== "path-owned-test-module") continue;
    for (const file of exemption.files ?? []) {
      if (typeof file.path === "string") exclusions.push(file.path);
    }
  }
  const scan = runStructuralScanner("crates", exclusions, identity);
  const byKind = (kind) =>
    scan.boundaries
      .filter((entry) => entry.kind === kind)
      .map(({ path: sourcePath, kind: boundaryKind, detail, line }) => ({
        path: sourcePath,
        kind: boundaryKind,
        detail,
        line,
      }));
  return {
    scanner_sha256: sha256(`${JSON.stringify(scan)}\n`),
    include_expansions: byKind("include-expansion"),
    conditional_module_paths: byKind("module-path"),
    conditional_modules: byKind("conditional-module"),
    qself_bind_adoptions: byKind("qself-bind-adoption"),
    network_glob_imports: byKind("network-glob-import"),
    authority_shaped_macros: byKind("authority-shaped-macro"),
    classified_risks: scan.risks,
  };
}

function generatedStructuralScan(outDirectory, identity) {
  const scan = runStructuralScanner(outDirectory, [], identity);
  const findings = [];
  const coveredIncludes = [];
  const counts = {};
  const forbiddenBoundaryKinds = new Set([
    "qself-bind-adoption",
    "network-glob-import",
    "authority-shaped-macro",
    "include-expansion",
  ]);
  for (const field of ["authorities", "risks", "composition", "boundaries"]) {
    const entries =
      field === "boundaries"
        ? scan[field].filter((entry) => forbiddenBoundaryKinds.has(entry.kind))
        : scan[field];
    let findingCount = 0;
    for (const entry of entries) {
      const absolute = path.isAbsolute(entry.path)
        ? entry.path
        : path.resolve(repoRoot, entry.path);
      const file = path
        .relative(outDirectory, absolute)
        .split(path.sep)
        .join("/");
      const detail = entry.detail ?? entry.symbol ?? "";
      if (field === "boundaries" && entry.kind === "include-expansion") {
        const targets = [...detail.matchAll(/"\/?([^"/]+\.rs)"/g)].map(
          (match) => match[1],
        );
        const target = targets.length === 1 ? targets[0] : null;
        if (
          detail.includes("OUT_DIR") &&
          target !== null &&
          generatedIncludeRoster.some(
            (edge) => edge.file === file && edge.target === target,
          )
        ) {
          coveredIncludes.push({ file, line: entry.line, target });
          continue;
        }
      }
      findingCount += 1;
      findings.push({
        file,
        line: entry.line,
        category: field,
        kind: entry.kind,
        detail,
      });
    }
    counts[field] = findingCount;
  }
  return { counts, findings, coveredIncludes };
}

function zeroCalls() {
  return Object.fromEntries(callKinds.map((kind) => [kind, 0]));
}

function expectedCalls(inventory) {
  const counts = zeroCalls();
  for (const occurrence of inventory.authority_occurrences ?? []) {
    if (inventoryKinds.has(occurrence.kind)) counts[occurrence.kind] += 1;
  }
  return counts;
}

function expectedCallsByPackage(inventory) {
  const expected = Object.fromEntries(
    ownerPackages.map((packageName) => [packageName, zeroCalls()]),
  );
  for (const occurrence of inventory.authority_occurrences ?? []) {
    if (!inventoryKinds.has(occurrence.kind)) continue;
    const match = /^crates\/([^/]+)\//.exec(occurrence.path ?? "");
    if (!match || !Object.hasOwn(expected, match[1])) {
      throw new Error(
        `socket authority inventory occurrence has no compiler owner: ${occurrence.path}`,
      );
    }
    expected[match[1]][occurrence.kind] += 1;
  }
  return expected;
}

function validateCalls(calls, label, errors) {
  if (!calls || typeof calls !== "object" || Array.isArray(calls)) {
    errors.push(`${label} call counts are not an object`);
    return false;
  }
  const keys = Object.keys(calls).sort();
  const expectedKeys = [...callKinds].sort();
  if (!compareObjects(keys, expectedKeys)) {
    errors.push(`${label} call-count keys are incomplete or unexpected`);
    return false;
  }
  for (const kind of callKinds) {
    if (!Number.isSafeInteger(calls[kind]) || calls[kind] < 0) {
      errors.push(`${label} has an invalid ${kind} call count`);
      return false;
    }
  }
  return true;
}

function validateGeneratedScanCounts(counts, findings, errors) {
  if (!counts || typeof counts !== "object" || Array.isArray(counts)) {
    errors.push("generated Rust scan counts are missing");
    return;
  }
  if (
    !compareObjects(Object.keys(counts).sort(), [...generatedScanKinds].sort())
  ) {
    errors.push("generated Rust scan-count keys are incomplete or unexpected");
    return;
  }
  for (const kind of generatedScanKinds) {
    if (!Number.isSafeInteger(counts[kind]) || counts[kind] < 0) {
      errors.push(`generated Rust scan has invalid ${kind} count`);
      return;
    }
  }
  if (
    Object.values(counts).reduce((sum, count) => sum + count, 0) !==
    findings.length
  ) {
    errors.push("generated Rust scan counts do not match its findings");
  }
}

function directMirCalls(mir) {
  const calls = zeroCalls();
  const patterns = [
    [
      "tcp-bind",
      /= (?:std::net::TcpListener|tokio::net::TcpListener)::bind(?:::<[^>\n]*>)?\([^\n]*\) ->/g,
    ],
    [
      "tcp-from-std",
      /= (?:std::net::TcpListener|tokio::net::TcpListener)::from_std\([^\n]*\) ->/g,
    ],
    [
      "tcp-from-raw-fd",
      /= <std::net::TcpListener as FromRawFd>::from_raw_fd\([^\n]*\) ->/g,
    ],
    [
      "tcp-from-owned-fd",
      /= <std::net::TcpListener as From<OwnedFd>>::from\([^\n]*\) ->/g,
    ],
    [
      "tcp-from-raw-socket",
      /= <std::net::TcpListener as FromRawSocket>::from_raw_socket\([^\n]*\) ->/g,
    ],
    [
      "tcp-from-owned-socket",
      /= <std::net::TcpListener as From<OwnedSocket>>::from\([^\n]*\) ->/g,
    ],
    [
      "udp-bind",
      /= (?:std::net::UdpSocket|tokio::net::UdpSocket)::bind(?:::<[^>\n]*>)?\([^\n]*\) ->/g,
    ],
    [
      "udp-from-std",
      /= (?:std::net::UdpSocket|tokio::net::UdpSocket)::from_std\([^\n]*\) ->/g,
    ],
    [
      "udp-from-raw-fd",
      /= <std::net::UdpSocket as FromRawFd>::from_raw_fd\([^\n]*\) ->/g,
    ],
    [
      "udp-from-owned-fd",
      /= <std::net::UdpSocket as From<OwnedFd>>::from\([^\n]*\) ->/g,
    ],
    [
      "udp-from-raw-socket",
      /= <std::net::UdpSocket as FromRawSocket>::from_raw_socket\([^\n]*\) ->/g,
    ],
    [
      "udp-from-owned-socket",
      /= <std::net::UdpSocket as From<OwnedSocket>>::from\([^\n]*\) ->/g,
    ],
    [
      "unix-bind",
      /= (?:std::os::unix::net::(?:UnixListener|UnixDatagram)|tokio::net::(?:UnixListener|UnixDatagram))::bind(?:::<[^>\n]*>)?\([^\n]*\) ->/g,
    ],
    [
      "unix-from-std",
      /= (?:std::os::unix::net::(?:UnixListener|UnixDatagram)|tokio::net::(?:UnixListener|UnixDatagram))::from_std\([^\n]*\) ->/g,
    ],
    [
      "unix-from-raw-fd",
      /= <std::os::unix::net::(?:UnixListener|UnixDatagram) as FromRawFd>::from_raw_fd\([^\n]*\) ->/g,
    ],
    [
      "unix-from-owned-fd",
      /= <std::os::unix::net::(?:UnixListener|UnixDatagram) as From<OwnedFd>>::from\([^\n]*\) ->/g,
    ],
  ];
  for (const [kind, pattern] of patterns) {
    calls[kind] = [...mir.matchAll(pattern)].length;
  }
  return calls;
}

function addCalls(target, source) {
  for (const kind of callKinds) target[kind] += source[kind] ?? 0;
}

function run(command, args, options = {}) {
  const result = spawnSync(command, args, {
    cwd: repoRoot,
    encoding: "utf8",
    maxBuffer: 128 * 1024 * 1024,
    ...options,
  });
  if (result.error)
    throw new Error(`${command} failed to start: ${result.error.message}`);
  if (result.status !== 0) {
    const detail = [result.stderr, result.stdout]
      .filter(Boolean)
      .join("\n")
      .trim();
    throw new Error(
      `${command} exited ${result.status}: ${detail || "<no output>"}`,
    );
  }
  return result;
}

function mirFile(directory, packageName, target) {
  const stem = `${packageName}-${target.kind}-${target.name}`.replaceAll(
    /[^A-Za-z0-9_.-]/g,
    "_",
  );
  return path.join(directory, `${stem}.mir`);
}

function readMirReports(directory, matrix) {
  return matrix.map((owner) => {
    const calls = zeroCalls();
    const targets = owner.targets.map((target) => {
      const file = mirFile(directory, owner.package, target);
      if (!fs.existsSync(file) || fs.statSync(file).size === 0) {
        throw new Error(
          `compiler emitted no MIR for ${owner.package} ${target.kind} ${target.name}`,
        );
      }
      const mir = fs.readFileSync(file, "utf8");
      const targetCalls = directMirCalls(mir);
      addCalls(calls, targetCalls);
      return {
        name: target.name,
        kind: target.kind,
        bytes: Buffer.byteLength(mir),
        sha256: sha256(mir),
        calls: targetCalls,
      };
    });
    return { package: owner.package, targets, calls };
  });
}

function collectMir(temporary, matrix, identity) {
  for (const owner of matrix) {
    for (const target of owner.targets) {
      const file = mirFile(temporary, owner.package, target);
      cargoRun(
        identity,
        [
          "rustc",
          "--locked",
          "-p",
          owner.package,
          ...target.selector,
          "--all-features",
          "--target",
          identity.target,
          "--",
          `--emit=mir=${file}`,
        ],
        { stdio: "inherit" },
      );
      if (!fs.existsSync(file) || fs.statSync(file).size === 0) {
        throw new Error(
          `compiler emitted no MIR for ${owner.package} ${target.kind} ${target.name}`,
        );
      }
    }
  }
  return readMirReports(temporary, matrix);
}

function collectGeneratedOutputs(identity) {
  const targetDirectory = fs.mkdtempSync(
    path.join(os.tmpdir(), "nimbus-network-generated-"),
  );
  try {
    const result = cargoRun(
      identity,
      [
        "check",
        "--locked",
        "-p",
        "nimbus-firebase",
        "--all-features",
        "--target",
        identity.target,
        "--message-format=json-render-diagnostics",
      ],
      { env: { CARGO_TARGET_DIR: targetDirectory } },
    );
    const outDirectories = new Set();
    for (const line of result.stdout.split("\n")) {
      if (!line.trim().startsWith("{")) continue;
      let message;
      try {
        message = JSON.parse(line);
      } catch {
        continue;
      }
      if (
        message.reason === "build-script-executed" &&
        String(message.package_id).includes("nimbus-firebase") &&
        message.out_dir
      ) {
        outDirectories.add(message.out_dir);
      }
    }
    if (outDirectories.size !== 1) {
      throw new Error(
        `nimbus-firebase build produced ${outDirectories.size} current OUT_DIR values`,
      );
    }
    const [outDirectory] = outDirectories;
    const files = walk(outDirectory, (file) => file.endsWith(".rs")).sort();
    const relativeFiles = files.map((file) =>
      path.relative(outDirectory, file).split(path.sep).join("/"),
    );
    if (!compareObjects(relativeFiles, generatedOutputRoster)) {
      throw new Error(
        `nimbus-firebase generated roster differs: ${JSON.stringify(relativeFiles)}`,
      );
    }
    const outputs = [];
    for (const file of files) {
      const source = fs.readFileSync(file, "utf8");
      const relativeFile = path
        .relative(outDirectory, file)
        .split(path.sep)
        .join("/");
      outputs.push({
        file: relativeFile,
        bytes: Buffer.byteLength(source),
        sha256: sha256(source),
      });
    }
    const structural = generatedStructuralScan(outDirectory, identity);
    return {
      outputs,
      scan_counts: structural.counts,
      covered_includes: structural.coveredIncludes,
      forbidden_findings: structural.findings,
    };
  } finally {
    fs.rmSync(targetDirectory, { recursive: true, force: true });
  }
}

function compilerConfiguration(identity) {
  return {
    target: identity.target,
    features: "all-features",
    production_target_kinds: ["lib", "bin"],
    test_cfg: false,
    uncompiled_source_rule:
      "direct calls must reconcile with MIR; parsed qself/glob/operation-shaped macros fail closed",
  };
}

function collectReport(inventoryPath, inventoryText, inventory) {
  const identity = toolchainIdentity();
  const matrix = productionTargetMatrix(identity);
  const temporary = fs.mkdtempSync(
    path.join(os.tmpdir(), "nimbus-network-mir-"),
  );
  try {
    const packages = collectMir(temporary, matrix, identity);
    const aggregate = zeroCalls();
    for (const packageReport of packages)
      addCalls(aggregate, packageReport.calls);
    return {
      schema_version: 2,
      input: digestInputs(),
      inventory: {
        path: inventoryPath,
        sha256: sha256(inventoryText),
      },
      compiler: identity,
      configuration: compilerConfiguration(identity),
      production_targets: matrix,
      owner_packages: packages,
      aggregate_calls: aggregate,
      expected_calls: expectedCalls(inventory),
      expected_calls_by_package: expectedCallsByPackage(inventory),
      source_boundaries: structuralScan(inventory, identity),
      generated: collectGeneratedOutputs(identity),
    };
  } finally {
    fs.rmSync(temporary, { recursive: true, force: true });
  }
}

function compareObjects(left, right) {
  return JSON.stringify(left) === JSON.stringify(right);
}

function validationContext(inventory) {
  const identity = toolchainIdentity();
  return {
    input: digestInputs(),
    identity,
    configuration: compilerConfiguration(identity),
    matrix: productionTargetMatrix(identity),
    expected: expectedCalls(inventory),
    expectedByPackage: expectedCallsByPackage(inventory),
    boundaries: structuralScan(inventory, identity),
  };
}

function validateReport(
  report,
  inventoryPath,
  inventoryText,
  inventory,
  suppliedContext = null,
) {
  const errors = [];
  const context = suppliedContext ?? validationContext(inventory);
  if (report?.schema_version !== 2)
    errors.push("baseline schema_version must be 2");
  const currentInput = context.input;
  if (report?.input?.sha256 !== currentInput.sha256) {
    errors.push("compiler evidence input digest is stale");
  }
  if (report?.input?.files !== currentInput.files) {
    errors.push("compiler evidence input file count is stale");
  }
  if (report?.inventory?.path !== inventoryPath) {
    errors.push("compiler evidence inventory path is stale");
  }
  if (report?.inventory?.sha256 !== sha256(inventoryText)) {
    errors.push("compiler evidence inventory digest is stale");
  }
  const identity = context.identity;
  if (!compareObjects(report?.compiler, identity)) {
    errors.push("effective compiler or Cargo configuration is stale");
  }
  const configuration = context.configuration;
  if (!compareObjects(report?.configuration, configuration)) {
    errors.push("compiler evidence configuration posture is stale");
  }
  const matrix = context.matrix;
  if (!compareObjects(report?.production_targets, matrix)) {
    errors.push("compiler evidence production target matrix is stale");
  }
  const packageNames = (report?.owner_packages ?? []).map(
    (entry) => entry.package,
  );
  if (!compareObjects(packageNames, ownerPackages)) {
    errors.push(
      "compiler evidence owner package roster is incomplete or unordered",
    );
  }
  const expected = context.expected;
  const expectedByPackage = context.expectedByPackage;
  if (!compareObjects(report?.expected_calls, expected)) {
    errors.push(
      "compiler evidence expected call counts do not match the authority inventory",
    );
  }
  if (!compareObjects(report?.expected_calls_by_package, expectedByPackage)) {
    errors.push(
      "compiler evidence per-package expectations do not match the authority inventory",
    );
  }
  validateCalls(report?.aggregate_calls, "aggregate", errors);
  if (!compareObjects(report?.aggregate_calls, expected)) {
    errors.push(
      "compiler-resolved call counts do not match the authority inventory",
    );
  }
  const summed = zeroCalls();
  for (const [index, entry] of (report?.owner_packages ?? []).entries()) {
    const packageName = ownerPackages[index] ?? `<unexpected-${index}>`;
    validateCalls(entry?.calls, `${packageName} package`, errors);
    const targetSum = zeroCalls();
    const expectedTargets = matrix[index]?.targets ?? [];
    const actualTargetRoster = (entry?.targets ?? []).map(({ name, kind }) => ({
      name,
      kind,
    }));
    const expectedTargetRoster = expectedTargets.map(({ name, kind }) => ({
      name,
      kind,
    }));
    if (!compareObjects(actualTargetRoster, expectedTargetRoster)) {
      errors.push(`${packageName} production MIR target roster is incomplete`);
    }
    for (const target of entry?.targets ?? []) {
      const label = `${packageName} ${target.kind ?? "?"} ${target.name ?? "?"}`;
      if (!Number.isSafeInteger(target.bytes) || target.bytes <= 0) {
        errors.push(`${label} has invalid MIR size`);
      }
      if (!/^[0-9a-f]{64}$/.test(target.sha256 ?? "")) {
        errors.push(`${label} has invalid MIR digest`);
      }
      validateCalls(target.calls, `${label} target`, errors);
      addCalls(targetSum, target.calls ?? {});
    }
    if (!compareObjects(targetSum, entry?.calls)) {
      errors.push(
        `${packageName} target call counts do not sum to the package`,
      );
    }
    if (!compareObjects(entry?.calls, expectedByPackage[packageName])) {
      errors.push(
        `${packageName} compiler-resolved calls do not match its authority inventory`,
      );
    }
    if (
      packageName === "nimbus-network" &&
      Object.values(entry?.calls ?? {}).some((count) => count !== 0)
    ) {
      errors.push("nimbus-network owns a resolved socket authority call");
    }
    addCalls(summed, entry?.calls ?? {});
  }
  if (!compareObjects(summed, report?.aggregate_calls)) {
    errors.push("per-package MIR call counts do not sum to the aggregate");
  }
  const currentBoundaries = context.boundaries;
  if (!compareObjects(report?.source_boundaries, currentBoundaries)) {
    errors.push("parsed compiler source-boundary census is stale");
  }
  for (const field of ["qself_bind_adoptions", "network_glob_imports"]) {
    if ((report?.source_boundaries?.[field] ?? []).length !== 0) {
      errors.push(`unclassified compiler boundary is present: ${field}`);
    }
  }
  const includes = report?.source_boundaries?.include_expansions ?? [];
  if (
    includes.length !== 1 ||
    includes[0].path !== "crates/nimbus-firebase/src/grpc.rs"
  ) {
    errors.push("generated include boundary is missing or unclassified");
  }
  const classifiedMacroKeys = new Set(
    (inventory.non_authority_occurrences ?? [])
      .filter((entry) => entry.kind === "authority-shaped-macro")
      .map((entry) => `${entry.path}:${entry.line}`),
  );
  for (const boundary of report?.source_boundaries?.authority_shaped_macros ??
    []) {
    if (!classifiedMacroKeys.has(`${boundary.path}:${boundary.line}`)) {
      errors.push(
        `authority-shaped macro lacks exact inventory classification: ${boundary.path}:${boundary.line}`,
      );
    }
    if (
      /\b(?:TcpListener|UdpSocket|UnixListener|UnixDatagram|TcpSocket)\b[\s\S]{0,300}\b(?:bind|from_std|from_raw_fd|from_owned_fd|from_raw_socket|from_owned_socket)\b/.test(
        boundary.detail ?? "",
      )
    ) {
      errors.push(
        `socket authority operation is hidden in a macro: ${boundary.path}:${boundary.line}`,
      );
    }
  }
  const outputs = Array.isArray(report?.generated?.outputs)
    ? report.generated.outputs
    : [];
  const generatedRoster = outputs.map((entry) => entry.file);
  if (!compareObjects(generatedRoster, generatedOutputRoster)) {
    errors.push("compiler evidence generated-output roster is incomplete");
  }
  for (const output of outputs) {
    if (!Number.isSafeInteger(output.bytes) || output.bytes <= 0) {
      errors.push(
        `generated output has invalid size: ${output.file ?? "<unknown>"}`,
      );
    }
    if (!/^[0-9a-f]{64}$/.test(output.sha256 ?? "")) {
      errors.push(
        `generated output has invalid digest: ${output.file ?? "<unknown>"}`,
      );
    }
  }
  const coveredIncludes = report?.generated?.covered_includes;
  if (!Array.isArray(coveredIncludes)) {
    errors.push("generated Rust covered-include evidence is missing");
  } else {
    for (const edge of coveredIncludes) {
      if (
        !edge ||
        typeof edge !== "object" ||
        Array.isArray(edge) ||
        !compareObjects(Object.keys(edge).sort(), ["file", "line", "target"])
      ) {
        errors.push("generated Rust covered-include entry is malformed");
        continue;
      }
      if (!Number.isSafeInteger(edge.line) || edge.line <= 0) {
        errors.push(
          `generated Rust covered include has invalid line: ${edge.file ?? "<unknown>"}`,
        );
      }
    }
    const actualEdges = coveredIncludes
      .map((edge) => ({ file: edge?.file, target: edge?.target }))
      .sort((left, right) =>
        `${left.file}\0${left.target}`.localeCompare(
          `${right.file}\0${right.target}`,
        ),
      );
    const expectedEdges = generatedIncludeRoster
      .map((edge) => ({ ...edge }))
      .sort((left, right) =>
        `${left.file}\0${left.target}`.localeCompare(
          `${right.file}\0${right.target}`,
        ),
      );
    if (!compareObjects(actualEdges, expectedEdges)) {
      errors.push("generated Rust covered-include roster is incomplete");
    }
  }
  const findings = report?.generated?.forbidden_findings;
  if (!Array.isArray(findings)) {
    errors.push("generated Rust scan result is missing");
  } else if (findings.length !== 0) {
    errors.push(
      `generated Rust output contains network authority: ${JSON.stringify(findings.slice(0, 10))}`,
    );
  }
  validateGeneratedScanCounts(
    report?.generated?.scan_counts,
    findings ?? [],
    errors,
  );
  return errors;
}

function compareDeepReport(baseline, current) {
  const errors = [];
  for (const field of [
    "input",
    "inventory",
    "compiler",
    "configuration",
    "production_targets",
    "owner_packages",
    "aggregate_calls",
    "expected_calls",
    "expected_calls_by_package",
    "source_boundaries",
    "generated",
  ]) {
    if (!compareObjects(baseline[field], current[field])) {
      errors.push(`deep compiler evidence differs: ${field}`);
    }
  }
  return errors;
}

function selfTest(inventoryPath, inventoryText, inventory) {
  const context = validationContext(inventory);
  const { input, expected, expectedByPackage, identity, matrix, boundaries } =
    context;
  const packages = matrix.map((owner) => ({
    package: owner.package,
    targets: owner.targets.map((target, index) => ({
      name: target.name,
      kind: target.kind,
      bytes: 1,
      sha256: sha256(`${owner.package}:${target.kind}:${target.name}`),
      calls:
        index === 0
          ? structuredClone(expectedByPackage[owner.package])
          : zeroCalls(),
    })),
    calls: structuredClone(expectedByPackage[owner.package]),
  }));
  const valid = {
    schema_version: 2,
    input,
    inventory: { path: inventoryPath, sha256: sha256(inventoryText) },
    compiler: identity,
    configuration: compilerConfiguration(identity),
    production_targets: matrix,
    owner_packages: packages,
    aggregate_calls: structuredClone(expected),
    expected_calls: structuredClone(expected),
    expected_calls_by_package: structuredClone(expectedByPackage),
    source_boundaries: boundaries,
    generated: {
      outputs: generatedOutputRoster.map((file) => ({
        file,
        bytes: 1,
        sha256: sha256(file),
      })),
      scan_counts: Object.fromEntries(
        generatedScanKinds.map((kind) => [kind, 0]),
      ),
      covered_includes: generatedIncludeRoster.map((edge, index) => ({
        ...edge,
        line: index + 1,
      })),
      forbidden_findings: [],
    },
  };
  const validErrors = validateReport(
    valid,
    inventoryPath,
    inventoryText,
    inventory,
    context,
  );
  if (validErrors.length !== 0) {
    throw new Error(
      `compiler authority self-test fixture is invalid: ${validErrors.join("; ")}`,
    );
  }
  const cases = [
    [
      "stale-input",
      (copy) => (copy.input.sha256 = "f".repeat(64)),
      "input digest",
    ],
    [
      "stale-inventory",
      (copy) => (copy.inventory.sha256 = "f".repeat(64)),
      "inventory digest",
    ],
    [
      "stale-compiler-config",
      (copy) => (copy.compiler.target = "stale-target"),
      "compiler or Cargo configuration",
    ],
    [
      "stale-compiler-environment",
      (copy) =>
        (copy.compiler.environment.CARGO_BUILD_RUSTFLAGS = "--cfg stale"),
      "compiler or Cargo configuration",
    ],
    [
      "missing-production-target",
      (copy) => copy.production_targets[0].targets.pop(),
      "production target matrix",
    ],
    ["missing-owner", (copy) => copy.owner_packages.pop(), "package roster"],
    [
      "negative-package-call",
      (copy) => (copy.owner_packages[0].calls["tcp-bind"] = -1),
      "invalid tcp-bind",
    ],
    [
      "unknown-package-call",
      (copy) => (copy.owner_packages[0].calls["unknown-call"] = 0),
      "keys are incomplete or unexpected",
    ],
    [
      "aggregate-mismatch",
      (copy) => (copy.aggregate_calls["tcp-bind"] += 1),
      "call counts",
    ],
    [
      "target-sum-mismatch",
      (copy) => (copy.owner_packages[0].targets[0].calls["tcp-bind"] += 1),
      "target call counts do not sum",
    ],
    [
      "network-package-call",
      (copy) => {
        const network = copy.owner_packages.find(
          (entry) => entry.package === "nimbus-network",
        );
        network.calls["tcp-bind"] = 1;
        network.targets[0].calls["tcp-bind"] = 1;
      },
      "nimbus-network compiler-resolved calls",
    ],
    [
      "qself-boundary",
      (copy) =>
        copy.source_boundaries.qself_bind_adoptions.push({
          path: "x",
          line: 1,
        }),
      "qself_bind_adoptions",
    ],
    [
      "network-glob-boundary",
      (copy) =>
        copy.source_boundaries.network_glob_imports.push({
          path: "x",
          line: 1,
        }),
      "network_glob_imports",
    ],
    [
      "macro-socket-operation",
      (copy) =>
        copy.source_boundaries.authority_shaped_macros.push({
          path: "x",
          line: 1,
          detail: "macro|TcpListener :: bind",
        }),
      "lacks exact inventory classification",
    ],
    [
      "missing-generated",
      (copy) => copy.generated.outputs.pop(),
      "roster is incomplete",
    ],
    [
      "invalid-generated-size",
      (copy) => (copy.generated.outputs[0].bytes = 0),
      "invalid size",
    ],
    [
      "generated-authority",
      (copy) => {
        copy.generated.scan_counts.risks = 1;
        copy.generated.forbidden_findings.push({
          kind: "ambiguous-instance-bind",
        });
      },
      "contains network authority",
    ],
    [
      "missing-generated-scan-result",
      (copy) => delete copy.generated.forbidden_findings,
      "scan result is missing",
    ],
  ];
  let passed = 0;
  for (const [name, mutate, expectedText] of cases) {
    const copy = structuredClone(valid);
    mutate(copy);
    const errors = validateReport(
      copy,
      inventoryPath,
      inventoryText,
      inventory,
      context,
    );
    if (!errors.some((error) => error.includes(expectedText))) {
      process.stderr.write(
        `SELFTEST FAIL ${name}: expected ${expectedText}: ${errors.join("; ")}\n`,
      );
      process.exit(1);
    }
    process.stdout.write(`SELFTEST PASS ${name}\n`);
    passed += 1;
  }
  process.stdout.write(
    `compiler authority self-test: ${passed} passed, 0 failed\n`,
  );
}

function main() {
  const baselinePath = argument("--baseline", defaultBaseline);
  const inventoryPath = argument("--inventory", defaultInventory);
  const { value: inventory, text: inventoryText } = readJson(
    inventoryPath,
    "authority inventory",
  );
  if (process.argv.includes("--self-test")) {
    selfTest(inventoryPath, inventoryText, inventory);
    return;
  }
  if (process.argv.includes("--refresh")) {
    const output = argument("--refresh");
    const report = collectReport(inventoryPath, inventoryText, inventory);
    const errors = validateReport(
      report,
      inventoryPath,
      inventoryText,
      inventory,
    );
    if (errors.length > 0) throw new Error(errors.join("\n"));
    fs.writeFileSync(output, `${JSON.stringify(report, null, 2)}\n`);
    process.stdout.write(
      `compiler authority baseline refreshed: ${output}: ` +
        `${report.owner_packages.length} packages, ` +
        `${report.generated.outputs.length} generated outputs\n`,
    );
    return;
  }
  const { value: baseline } = readJson(
    baselinePath,
    "compiler authority baseline",
  );
  const errors = validateReport(
    baseline,
    inventoryPath,
    inventoryText,
    inventory,
  );
  if (process.argv.includes("--deep-check")) {
    const current = collectReport(inventoryPath, inventoryText, inventory);
    errors.push(...compareDeepReport(baseline, current));
  }
  if (errors.length > 0) throw new Error(errors.join("\n"));
  process.stdout.write(
    `compiler authority contract: ${baseline.owner_packages.length} packages, ` +
      `${Object.values(baseline.aggregate_calls).reduce((sum, count) => sum + count, 0)} resolved calls, ` +
      `${baseline.generated.outputs.length} generated outputs\n`,
  );
}

try {
  main();
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
}
