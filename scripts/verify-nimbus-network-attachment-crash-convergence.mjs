#!/usr/bin/env node

import fs from "node:fs";

const lifecyclePath =
  "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle.rs";
const activePath =
  "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/active_reconciliation.rs";
const recoveryPath =
  "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/recovery.rs";
const crashTestPath =
  "crates/nimbus-sandbox/src/backends/oci/network/attachment_lifecycle/tests/crash_recovery.rs";
const modularityProofPath =
  "docs/private/plans/proof/nimbus-network-control-plane/nnc6.4-atomic-provision-caller-cutover.md";

let lifecycle = fs.readFileSync(lifecyclePath, "utf8");
let active = fs.readFileSync(activePath, "utf8");
let recovery = fs.readFileSync(recoveryPath, "utf8");
let crashTest = fs.readFileSync(crashTestPath, "utf8");
const modularityProof = fs.readFileSync(modularityProofPath, "utf8");

switch (process.env.NIMBUS_NETWORK_VERIFY_TEST_ATTACHMENT_CRASH_MUTATION) {
  case undefined:
    break;
  case "missing-create-cut":
    crashTest = crashTest.replace(
      "attachment.create.listener_claims_held",
      "attachment.create.missing_claim_cut",
    );
    break;
  case "missing-delete-cut":
    crashTest = crashTest.replace(
      "attachment.delete.namespace_removed",
      "attachment.delete.missing_namespace_cut",
    );
    break;
  case "create-phase-swap":
    crashTest = crashTest
      .replace(
        "phase: AttachmentAttachPhase::ProviderAttemptAuthenticated,",
        "phase: AttachmentAttachPhase::__NNC54_CREATE_PHASE_SWAP__,",
      )
      .replace(
        "phase: AttachmentAttachPhase::NamespaceCreated,",
        "phase: AttachmentAttachPhase::ProviderAttemptAuthenticated,",
      )
      .replace(
        "phase: AttachmentAttachPhase::__NNC54_CREATE_PHASE_SWAP__,",
        "phase: AttachmentAttachPhase::NamespaceCreated,",
      );
    break;
  case "delete-phase-swap":
    crashTest = crashTest
      .replace(
        "phase: AttachmentDetachPhase::ProviderDetached,",
        "phase: AttachmentDetachPhase::__NNC54_DELETE_PHASE_SWAP__,",
      )
      .replace(
        "phase: AttachmentDetachPhase::NamespaceRemoved,",
        "phase: AttachmentDetachPhase::ProviderDetached,",
      )
      .replace(
        "phase: AttachmentDetachPhase::__NNC54_DELETE_PHASE_SWAP__,",
        "phase: AttachmentDetachPhase::NamespaceRemoved,",
      );
    break;
  case "publishing-never-bound":
    active = active.replace(
      "self.resume_attachment_publication(",
      "self.missing_publication_recovery(",
    );
    break;
  case "detached-namespace-unknown":
    recovery = recovery.replaceAll(
      "AttachmentProviderObservation::DetachedNamespacePending",
      "AttachmentProviderObservation::Unknown",
    );
    break;
  case "duplicate-delete-unproven":
    crashTest = crashTest.replaceAll(
      "expected_teardown_count",
      "missing_teardown_count_proof",
    );
    break;
  case "unbounded-child":
    crashTest = crashTest.replace(
      "const CHILD_TIMEOUT",
      "const MISSING_CHILD_TIMEOUT",
    );
    break;
  case "missing-pre-crash-witness":
    crashTest = crashTest.replace(
      "const PRE_CRASH_WITNESS",
      "const MISSING_PRE_CRASH_WITNESS",
    );
    break;
  default:
    throw new Error("unknown NNC5.4 verifier mutation");
}

const failures = [];
const requireText = (source, text, detail) => {
  if (!source.includes(text)) failures.push(detail);
};

const createCuts = [
  [
    "attachment.create.provider_attempt_prepared",
    "ProviderAttemptAuthenticated",
  ],
  ["attachment.create.namespace_created", "NamespaceCreated"],
  ["attachment.create.listener_claims_held", "ListenerClaimsHeld"],
  ["attachment.create.provider_ready", "ProviderSetupComplete"],
  ["attachment.create.publishing", "Publishing"],
  ["attachment.create.listeners_active", "ListenerBindingsActive"],
  [
    "attachment.create.backend_publication_complete",
    "BackendPublicationComplete",
  ],
  ["attachment.create.lifetime_registered", "LifetimeRegistered"],
  ["attachment.create.attachment_confirmed", "AttachmentConfirmed"],
  ["attachment.create.active", "Active"],
];
const deleteCuts = [
  ["attachment.delete.attempt_prepared", "AttemptPrepared"],
  ["attachment.delete.backend_withdrawn", "BackendWithdrawn"],
  ["attachment.delete.segment_quarantined", "SegmentQuarantined"],
  ["attachment.delete.listener_cleanup_prepared", "ListenerCleanupPrepared"],
  ["attachment.delete.provider_detached", "ProviderDetached"],
  ["attachment.delete.namespace_removed", "NamespaceRemoved"],
  ["attachment.delete.listeners_settled", "ListenersSettled"],
  ["attachment.delete.ipam_released", "IpamReleased"],
  ["attachment.delete.segment_released", "SegmentReleased"],
  ["attachment.delete.attachment_terminal", "AttachmentTerminal"],
];

requireText(
  crashTest,
  "const CREATE_CUTS: [CreateCut; 10]",
  "create matrix is not pinned to 10 cuts",
);
requireText(
  crashTest,
  "const DELETE_CUTS: [DeleteCut; 10]",
  "delete matrix is not pinned to 10 cuts",
);

const extractMappings = (source, entryType, phaseType) => {
  const mappings = [];
  const expression = new RegExp(
    `${entryType}\\s*\\{\\s*label:\\s*"([^"]+)",\\s*phase:\\s*${phaseType}::([A-Za-z0-9_]+),[\\s\\S]*?\\}`,
    "g",
  );
  for (const match of source.matchAll(expression)) {
    mappings.push([match[1], match[2]]);
  }
  return mappings;
};

const requireExactMappings = (actual, expected, kind) => {
  if (actual.length !== expected.length) {
    failures.push(
      `${kind} crash matrix exposes ${actual.length} label/phase mappings, expected ${expected.length}`,
    );
    return;
  }
  for (let index = 0; index < expected.length; index += 1) {
    const [expectedLabel, expectedPhase] = expected[index];
    const [actualLabel, actualPhase] = actual[index];
    if (actualLabel !== expectedLabel || actualPhase !== expectedPhase) {
      failures.push(
        `${kind} crash cut ${index} maps ${JSON.stringify(actualLabel)} to ${actualPhase}, expected ${JSON.stringify(expectedLabel)} to ${expectedPhase}`,
      );
    }
  }
};

for (const [cut] of createCuts) {
  requireText(crashTest, cut, `missing create crash cut ${cut}`);
}
for (const [cut] of deleteCuts) {
  requireText(crashTest, cut, `missing delete crash cut ${cut}`);
}
requireExactMappings(
  extractMappings(crashTest, "CreateCut", "AttachmentAttachPhase"),
  createCuts,
  "create",
);
requireExactMappings(
  extractMappings(crashTest, "DeleteCut", "AttachmentDetachPhase"),
  deleteCuts,
  "delete",
);

requireText(
  active,
  "self.resume_attachment_publication(",
  "provider-present publication does not enter the state-directed recovery seam",
);
requireText(
  active,
  "reconcile_active_netavark_bindings_with_lifetimes",
  "publication recovery does not reuse exact dead-owner listener reconciliation",
);
requireText(
  active,
  "authenticate_attachment_recovery_authority",
  "cleanup-only evidence is not authenticated before its portable fence",
);
requireText(
  recovery,
  "AttachmentProviderObservation::DetachedNamespacePending",
  "exact provider-detached namespace cleanup has no typed observation",
);
requireText(
  recovery,
  "namespace_cleanup_required",
  "detach recovery does not carry the namespace-only cleanup decision",
);
requireText(
  crashTest,
  "expected_teardown_count",
  "fresh recovery does not prove acknowledged provider detach is not repeated",
);
requireText(
  crashTest,
  "Command::new(std::env::current_exe()",
  "crash matrix does not spawn the actual test process",
);
requireText(
  crashTest,
  "child.kill()",
  "crash matrix does not kill the effect-owning child",
);
requireText(
  crashTest,
  "const CHILD_TIMEOUT",
  "crash matrix has no bounded child timeout",
);
requireText(
  crashTest,
  "const PRE_CRASH_WITNESS",
  "crash matrix has no durable pre-crash identity and address witness",
);
requireText(
  crashTest,
  "NetworkResourceVersion::for_plan(",
  "crash matrix does not independently compile the full expected resource version",
);
requireText(
  crashTest,
  "fn requires_stable_handle(&self)",
  "crash matrix does not pin stable-handle presence to the exact create cut",
);
requireText(
  crashTest,
  "ContainerIpamAuthorityState::Released => load_released_container_ips(",
  "delete crash cuts do not compare released IPAM addresses with the pre-crash witness",
);
requireText(
  crashTest,
  "run_child(CREATE_RECOVERY_CHILD",
  "create recovery does not reopen in a fresh child",
);
requireText(
  crashTest,
  "run_child(DELETE_REPLAY_CHILD",
  "terminal delete replay does not reopen in a fresh child",
);

const lifecycleLines = lifecycle.split("\n").length - 1;
if (lifecycleLines >= 2000) {
  failures.push(
    `attachment lifecycle composition root is ${lifecycleLines} lines (must remain below 2000)`,
  );
} else if (lifecycleLines >= 1500) {
  const exactPrefix = `| \`${lifecyclePath}\` | ${lifecycleLines.toLocaleString("en-US")} |`;
  const disposition = modularityProof
    .split("\n")
    .find((line) => line.startsWith(exactPrefix));
  if (!disposition) {
    failures.push(
      `attachment lifecycle composition root is ${lifecycleLines} lines without an exact NNC6.4 ownership disposition`,
    );
  } else {
    for (const rationale of [
      "attachment lifecycle owner",
      "authority",
      "readiness",
      "reconciliation",
      "release",
      "concept children",
      "Publication stays outside",
    ]) {
      requireText(
        disposition,
        rationale,
        `attachment lifecycle modularity disposition lacks ownership rationale ${rationale}`,
      );
    }
  }
  for (const child of [
    "mod active_reconciliation;",
    "mod attachment_readiness;",
    "mod authority;",
    "mod detach_release;",
    "mod recovery;",
  ]) {
    requireText(
      lifecycle,
      child,
      `attachment lifecycle composition root lacks concept-owned child ${child}`,
    );
  }
}

if (failures.length > 0) {
  for (const failure of failures) console.error(failure);
  process.exit(1);
}
