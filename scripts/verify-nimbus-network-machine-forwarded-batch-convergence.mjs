#!/usr/bin/env node

import fs from "node:fs";

const paths = {
  publication:
    "crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication.rs",
  store:
    "crates/nimbus-sandbox/src/backends/container/runtime/machine_port_publication/store.rs",
  forwarding: "crates/nimbus-sandbox/src/backends/oci/network/forwarding.rs",
  observation:
    "crates/nimbus-sandbox/src/backends/oci/network/forwarding/receipt.rs",
  processLifetime:
    "crates/nimbus-sandbox/src/backends/oci/network/process/machine_proxy_lifetime.rs",
  freshProcess:
    "crates/nimbus-sandbox/src/backends/container/runtime/tests/machine_port_batch_recovery/fresh_process.rs",
  lifecycle:
    "crates/nimbus-sandbox/src/backends/container/runtime/lifecycle.rs",
  providerCleanup:
    "crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup.rs",
  portLifecycle: "crates/nimbus-sandbox/src/backends/oci/port_lifecycle.rs",
};
const modularityProofPath =
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4-atomic-provision-caller-cutover.md";

const legacyPaths = [
  "crates/nimbus-sandbox/src/backends/container/runtime/machine_port_evidence.rs",
  "crates/nimbus-sandbox/src/backends/container/runtime/machine_port_evidence/tests.rs",
];

const sources = Object.fromEntries(
  Object.entries(paths).map(([name, sourcePath]) => [
    name,
    fs.readFileSync(sourcePath, "utf8"),
  ]),
);
const modularityProof = fs.readFileSync(modularityProofPath, "utf8");
let legacyAuthorityPresent = legacyPaths.some((sourcePath) =>
  fs.existsSync(sourcePath),
);

switch (
  process.env.NIMBUS_NETWORK_VERIFY_TEST_MACHINE_BATCH_CONVERGENCE_MUTATION
) {
  case undefined:
    break;
  case "legacy-authority":
    legacyAuthorityPresent = true;
    break;
  case "restored-process-local-authority":
    sources.processLifetime = sources.processLifetime.replace(
      "pub(crate) struct MachinePortProxyRegistration {\n",
      [
        "pub(crate) struct MachinePortProxyRegistration {",
        "    pub(crate) publication_may_exist: bool,",
        "    pub(crate) publication_withdrawn: bool,",
        "    pub(crate) publication_absence_receipts: Vec<String>,",
        "",
      ].join("\n"),
    );
    break;
  case "missing-record-field":
    sources.publication = sources.publication.replace(
      "    attachment_version: NetworkResourceVersion,\n",
      "",
    );
    break;
  case "collapsed-ambiguity":
    sources.publication = sources.publication.replace(
      "    EffectMayExist,\n",
      "",
    );
    break;
  case "effect-before-journal":
    sources.publication = sources.publication.replace(
      [
        "                record.slots[index] = MachinePortPublicationSlot::EffectMayExist;",
        "                publish_record_locked(state_dir, &record)?;",
      ].join("\n"),
      [
        "                record.slots[index] = MachinePortPublicationSlot::EffectMayExist;",
        "                // deliberately missing durable publication before the effect",
      ].join("\n"),
    );
    break;
  case "missing-post-effect-inspection":
    sources.publication = sources.publication.replace(
      "            let after = inspect_provider(provider, &expectation).map_err(|inspection_error| {",
      "            let after = observation.clone(); /* no exact post-effect inspection */\n            let after = Ok(after).map_err(|inspection_error| {",
    );
    break;
  case "weakened-store":
    sources.store = sources.store.replace(
      "            .and_then(|()| stage.sync_all())",
      "            .and_then(|()| Ok(()))",
    );
    break;
  case "unbounded-lock":
    sources.store = sources.store.replace(
      "    let deadline = Instant::now() + MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT;",
      "    let deadline = Instant::now();",
    );
    break;
  case "broadened-provider":
    sources.forwarding = sources.forwarding.replace(
      "pub(crate) trait MachinePortForwardingProvider",
      "pub(crate) trait NetworkProvider",
    );
    break;
  case "reordered-cuts": {
    const first = "machine.expose.local_provider_ready";
    const second = "machine.expose.batch_prepared";
    sources.freshProcess = sources.freshProcess
      .replace(first, "__NNC54A_FIRST_CUT__")
      .replace(second, first)
      .replace("__NNC54A_FIRST_CUT__", second);
    break;
  }
  case "removed-contention":
    sources.freshProcess = sources.freshProcess.replace(
      "nnc5_4a_two_process_contenders_share_one_generation_and_effect_sequence",
      "nnc5_4a_missing_two_process_contention_proof",
    );
    break;
  default:
    throw new Error("unknown NNC5.4a verifier mutation");
}

const failures = [];

function requireText(source, text, detail) {
  if (!source.includes(text)) failures.push(detail);
}

function requireAbsent(source, text, detail) {
  if (source.includes(text)) failures.push(detail);
}

function requireOrder(source, tokens, detail) {
  let cursor = -1;
  for (const token of tokens) {
    const next = source.indexOf(token, cursor + 1);
    if (next < 0 || next <= cursor) {
      failures.push(detail);
      return;
    }
    cursor = next;
  }
}

function enumVariants(source, enumName) {
  const match = source.match(
    new RegExp(`enum ${enumName}\\s*\\{([\\s\\S]*?)\\n\\}`),
  );
  if (!match) return [];
  return [...match[1].matchAll(/^\s*([A-Za-z][A-Za-z0-9_]*)/gm)].map(
    (entry) => entry[1],
  );
}

function structBody(source, structName) {
  const match = source.match(
    new RegExp(`struct ${structName}\\s*\\{([\\s\\S]*?)\\n\\}`),
  );
  return match?.[1] ?? "";
}

function stringArrayConstant(source, constantName) {
  const match = source.match(
    new RegExp(
      `const ${constantName}: \\[&str; \\d+\\] = \\[([\\s\\S]*?)\\n\\];`,
    ),
  );
  if (!match) return [];
  return [...match[1].matchAll(/"([^"]+)"/g)].map((entry) => entry[1]);
}

if (legacyAuthorityPresent) {
  failures.push(
    "legacy terminal-only machine-port evidence authority still exists",
  );
}
for (const token of [
  "publication_may_exist",
  "publication_withdrawn",
  "publication_absence_receipts",
]) {
  for (const source of [sources.publication, sources.processLifetime]) {
    requireAbsent(
      source,
      token,
      `process-local provider outcome authority ${token} reappeared`,
    );
  }
}

const phases = enumVariants(sources.publication, "MachinePortPublicationPhase");
if (
  JSON.stringify(phases) !==
  JSON.stringify(["Absent", "Exposing", "Exposed", "Withdrawing"])
) {
  failures.push(
    `machine publication phases are ${JSON.stringify(phases)}, expected strict Absent/Exposing/Exposed/Withdrawing`,
  );
}
const slots = enumVariants(sources.publication, "MachinePortPublicationSlot");
if (
  JSON.stringify(slots) !==
  JSON.stringify([
    "Pending",
    "EffectMayExist",
    "ObservedExposed",
    "ObservedAbsent",
  ])
) {
  failures.push(
    `machine publication slots are ${JSON.stringify(slots)}, expected explicit ambiguity and observed outcomes`,
  );
}

const publicationRecord = structBody(
  sources.publication,
  "MachinePortPublicationRecord",
);
for (const field of [
  "version: u32",
  "phase: MachinePortPublicationPhase",
  "tenant_id: TenantId",
  "sandbox_id: SandboxId",
  "attachment_id: NetworkAttachmentId",
  "attachment_version: NetworkResourceVersion",
  "provider_instance: NetworkProviderHandle",
  "provider_generation: NetworkResourceGeneration",
  "batch_generation: u64",
  "bindings: Vec<SandboxPortBinding>",
  "port_leases: Vec<PortLeaseRequest>",
  "slots: Vec<MachinePortPublicationSlot>",
]) {
  requireText(
    publicationRecord,
    field,
    `durable machine publication record lacks exact field ${field}`,
  );
}
requireText(
  sources.publication,
  "#[serde(deny_unknown_fields)]\nstruct MachinePortPublicationRecord",
  "machine publication record is not strict against unknown fields",
);
requireText(
  sources.store,
  "record_sha256: String",
  "durable publication envelope lacks SHA-256 integrity",
);
requireText(
  sources.store,
  "self.record_sha256 != record_sha256(&self.record)?",
  "durable publication reopen does not authenticate exact record bytes",
);

const effectBlockStart = sources.publication.indexOf(
  "if record.slots[index] != MachinePortPublicationSlot::EffectMayExist",
);
const effectBlockEnd = sources.publication.indexOf(
  "observer.checkpoint(MachinePortPublicationCheckpoint::SlotEffectReturned",
  effectBlockStart,
);
const effectBlock =
  effectBlockStart >= 0 && effectBlockEnd > effectBlockStart
    ? sources.publication.slice(effectBlockStart, effectBlockEnd)
    : "";
requireOrder(
  effectBlock,
  [
    "record.slots[index] = MachinePortPublicationSlot::EffectMayExist",
    "publish_record_locked(state_dir, &record)?",
    "provider.expose_one",
  ],
  "exposure effect is not preceded by durable EffectMayExist publication",
);
requireOrder(
  effectBlock,
  [
    "record.slots[index] = MachinePortPublicationSlot::EffectMayExist",
    "publish_record_locked(state_dir, &record)?",
    "provider.withdraw_one",
  ],
  "withdrawal effect is not preceded by durable EffectMayExist publication",
);
requireOrder(
  sources.publication,
  [
    "observer.checkpoint(MachinePortPublicationCheckpoint::SlotEffectReturned",
    "let after = inspect_provider(provider, &expectation)",
    "persist_observed_progress(state_dir, &mut record, action, &after)",
  ],
  "mutation return is not followed by exact provider inspection before durable observation",
);

requireOrder(
  sources.store,
  [
    ".write_all(&rendered)",
    ".and_then(|()| stage.sync_all())",
    "MachinePortEvidenceStoreCheckpoint::StageDurable",
    "fs::rename(&stage_path, &evidence_path)",
    "MachinePortEvidenceStoreCheckpoint::CanonicalRenamed",
    "sync_directory(state_dir)",
  ],
  "machine publication store lost staged-write/file-sync/rename/directory-sync ordering",
);
for (const token of [
  "MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT",
  "let deadline = Instant::now() + MACHINE_PORT_EVIDENCE_LOCK_TIMEOUT",
  "FileExt::try_lock_exclusive(&lock)",
  "MachinePortEvidenceLockError::Timeout",
]) {
  requireText(
    sources.store,
    token,
    `cross-process publication lock lacks bounded typed behavior: ${token}`,
  );
}

requireText(
  sources.forwarding,
  "pub(crate) trait MachinePortForwardingProvider",
  "small machine-forwarding capability trait is missing",
);
for (const method of ["fn inspect(", "fn expose_one(", "fn withdraw_one("]) {
  requireText(
    sources.forwarding,
    method,
    `machine-forwarding capability lacks ${method}`,
  );
}
requireText(
  sources.forwarding,
  "impl MachinePortForwardingProvider for DeterministicMachinePortForwardingProvider",
  "deterministic machine-forwarding substitute is missing",
);
requireAbsent(
  sources.forwarding,
  "trait NetworkProvider",
  "speculative god NetworkProvider reappeared",
);
const observationDeclaration =
  sources.observation.match(
    /(?:#\[[^\]]+\]\s*)*pub\(crate\) struct CurrentMachinePortForwardingObservation/,
  )?.[0] ?? "";
if (/Serialize|Deserialize/.test(observationDeclaration)) {
  failures.push(
    "current provider observation became serializable provider truth",
  );
}

const exposureCuts = [
  "machine.expose.local_provider_ready",
  "machine.expose.batch_prepared",
  "machine.expose.slot_effect_prepared",
  "machine.expose.slot_effect_returned",
  "machine.expose.slot_observed",
  "machine.expose.batch_exposed",
  "machine.expose.attachment_active",
];
const withdrawalCuts = [
  "machine.withdraw.batch_prepared",
  "machine.withdraw.local_provider_stopped",
  "machine.withdraw.slot_effect_prepared",
  "machine.withdraw.slot_effect_returned",
  "machine.withdraw.slot_observed_absent",
  "machine.withdraw.batch_absent",
  "machine.withdraw.listener_settled",
];
const actualExposureCuts = stringArrayConstant(
  sources.freshProcess,
  "EXPOSURE_CUTS",
);
if (JSON.stringify(actualExposureCuts) !== JSON.stringify(exposureCuts)) {
  failures.push(
    `real-process exposure cuts are ${JSON.stringify(actualExposureCuts)}, expected ${JSON.stringify(exposureCuts)}`,
  );
}
const actualWithdrawalCuts = stringArrayConstant(
  sources.freshProcess,
  "WITHDRAWAL_CUTS",
);
if (JSON.stringify(actualWithdrawalCuts) !== JSON.stringify(withdrawalCuts)) {
  failures.push(
    `real-process withdrawal cuts are ${JSON.stringify(actualWithdrawalCuts)}, expected ${JSON.stringify(withdrawalCuts)}`,
  );
}
for (const token of [
  "Command::new(std::env::current_exe()",
  "child.kill()",
  "const CHILD_TIMEOUT",
  "SurvivingProvider",
  "nnc5_4a_two_process_contenders_share_one_generation_and_effect_sequence",
  "contention-timeout",
  "exposure-replay",
  "withdrawal-replay",
]) {
  requireText(
    sources.freshProcess,
    token,
    `fresh-process recovery proof lacks ${token}`,
  );
}

const modularityExpectations = {
  publication: {
    sourceTokens: ["mod store;"],
    rationaleTokens: [
      "external-publication journal",
      "command/authority authentication state machine",
      "transport",
      "provider selection remain outside",
    ],
  },
  portLifecycle: {
    sourceTokens: [
      "mod authority;",
      "mod batch_state;",
      "mod machine;",
      "mod netavark_lifetime;",
    ],
    rationaleTokens: [
      "port transition state machine",
      "machine-specific behavior",
      "child",
    ],
  },
};

for (const [name, sourcePath, source] of [
  ["publication", paths.publication, sources.publication],
  ["lifecycle", paths.lifecycle, sources.lifecycle],
  ["providerCleanup", paths.providerCleanup, sources.providerCleanup],
  ["portLifecycle", paths.portLifecycle, sources.portLifecycle],
  ["forwarding", paths.forwarding, sources.forwarding],
]) {
  const lines = source.split("\n").length - 1;
  if (lines >= 2000) {
    failures.push(
      `${name} composition owner is ${lines} lines (must remain below 2000)`,
    );
  } else if (lines >= 1500) {
    const exactPrefix = `| \`${sourcePath}\` | ${lines.toLocaleString("en-US")} |`;
    const disposition = modularityProof
      .split("\n")
      .find((line) => line.startsWith(exactPrefix));
    if (!disposition) {
      failures.push(
        `${name} composition owner is ${lines} lines without an exact NNC6.4 ownership disposition`,
      );
      continue;
    }
    const expectations = modularityExpectations[name];
    if (expectations) {
      for (const token of expectations.sourceTokens) {
        requireText(
          source,
          token,
          `${name} composition owner lacks concept-owned child ${token}`,
        );
      }
      for (const token of expectations.rationaleTokens) {
        requireText(
          disposition,
          token,
          `${name} modularity disposition lacks ownership rationale ${token}`,
        );
      }
    }
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(failure);
  process.exit(1);
}
