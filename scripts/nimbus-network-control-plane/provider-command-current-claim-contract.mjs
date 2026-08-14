import fs from "node:fs";
import path from "node:path";

const root = process.env.NIMBUS_NETWORK_NNC82_ROOT ?? process.cwd();
const mutation = process.env.NIMBUS_NETWORK_VERIFY_NNC82_MUTATION ?? "";

const producers = [
  {
    path: "crates/nimbus-compute/src/workload_saga/provision_provider.rs",
    claimCount: 3,
    adoptedRecoveryCount: 2,
    markers: [
      ".execute_current_claim(execution",
      ".inspect_claimed_current_and_publish(execution",
      ".inspect_current_claim_and_publish(observation",
    ],
  },
  {
    path: "crates/nimbus-compute/src/workload_saga/restart_provider_command.rs",
    claimCount: 3,
    adoptedRecoveryCount: 2,
    markers: [
      ".execute_current_claim(execution",
      ".inspect_claimed_current_and_publish(execution",
      ".inspect_current_claim_and_publish(observation",
    ],
  },
  {
    path: "crates/nimbus-cli/src/machine/api/service_workloads/provision.rs",
    claimCount: 2,
    adoptedRecoveryCount: 1,
    markers: [
      ".execute_current_claim_async(execution",
      ".inspect_claimed_current_async_and_publish(execution",
      ".inspect_current_claim_async_and_publish(&observation",
    ],
  },
  {
    path: "crates/nimbus-cli/src/machine/api/service_workloads/restart.rs",
    claimCount: 2,
    adoptedRecoveryCount: 1,
    markers: [
      ".execute_current_claim_async(execution",
      ".inspect_claimed_current_async_and_publish(execution",
      ".inspect_current_claim_async_and_publish(&observation",
    ],
  },
];

const protectedTeardown = [
  [
    "crates/nimbus-sandbox/src/backends/container/runtime/teardown.rs",
    "execute_current_claim(execution_claim",
  ],
  [
    "crates/nimbus-sandbox/src/backends/container/runtime/attachment_teardown.rs",
    "execute_current_claim(execution_claim",
  ],
  [
    "crates/nimbus-sandbox/src/backends/krun/vm/teardown.rs",
    "execute_current_claim(execution_claim",
  ],
  [
    "crates/nimbus-sandbox/src/backends/krun/vm/attachment_teardown.rs",
    "execute_current_claim(execution_claim",
  ],
  [
    "crates/nimbus-cli/src/machine/api/service_workloads/teardown.rs",
    "execute_started_claim_async(execution",
  ],
  [
    "crates/nimbus-cli/src/machine/api/service_workloads/teardown/attachment.rs",
    "execute_started_claim_async(execution",
  ],
  [
    "crates/nimbus-cli/src/machine/backend/teardown.rs",
    "execute_started_claim_async(&validated.provider_command, execution",
  ],
];

const protocolPath =
  "crates/nimbus-sandbox/src/provider_command/current_claim.rs";
const protocolMarkers = [
  "pub fn execute_current_claim<T>",
  "pub fn inspect_claimed_current_and_publish<T>",
  "pub fn inspect_current_claim_and_publish<T>",
  "pub async fn execute_current_claim_async<T, Execute>",
  "pub async fn inspect_claimed_current_async_and_publish<T, Inspect>",
  "pub async fn inspect_current_claim_async_and_publish<T, Inspect>",
];

const sources = new Map();
const diagnostics = [];

function diagnostic(domain, message) {
  diagnostics.push(`provider-command-current-claim/${domain}: ${message}`);
}

function read(relative) {
  if (sources.has(relative)) return sources.get(relative);
  const absolute = path.join(root, relative);
  try {
    const source = fs.readFileSync(absolute, "utf8");
    sources.set(relative, source);
    return source;
  } catch (error) {
    diagnostic(
      "inputs",
      `${relative} is missing or unreadable: ${error.message}`,
    );
    sources.set(relative, "");
    return "";
  }
}

function replaceOne(relative, before, after) {
  const source = read(relative);
  const index = source.indexOf(before);
  if (index < 0)
    throw new Error(`${mutation}: missing mutation anchor ${before}`);
  sources.set(
    relative,
    source.slice(0, index) + after + source.slice(index + before.length),
  );
}

for (const producer of producers) read(producer.path);
for (const [relative] of protectedTeardown) read(relative);
read(protocolPath);

switch (mutation) {
  case "":
    break;
  case "discard-compute-token":
    replaceOne(
      producers[0].path,
      "ExecuteClaimed(execution)",
      "ExecuteClaimed(_)",
    );
    break;
  case "discard-guest-token":
    replaceOne(
      producers[2].path,
      "ExecuteClaimed(execution)",
      "ExecuteClaimed(_)",
    );
    break;
  case "missing-sync-execution":
    replaceOne(
      producers[1].path,
      ".execute_current_claim(execution",
      ".execute_without_current_claim(execution",
    );
    break;
  case "missing-async-inspection":
    replaceOne(
      producers[3].path,
      ".inspect_current_claim_async_and_publish(&observation",
      ".inspect_without_current_claim(&observation",
    );
    break;
  case "skip-claimed-recovery":
    replaceOne(
      producers[0].path,
      "ProviderCommandObservationKind::Claimed\n                        | ProviderCommandObservationKind::InProgress",
      "ProviderCommandObservationKind::RetryAuthorized\n                        | ProviderCommandObservationKind::InProgress",
    );
    break;
  case "missing-protected-teardown":
    replaceOne(
      protectedTeardown[0][0],
      protectedTeardown[0][1],
      "execute_without_current_claim(execution_claim",
    );
    break;
  case "private-protocol":
    replaceOne(
      protocolPath,
      "pub fn execute_current_claim<T>",
      "pub(crate) fn execute_current_claim<T>",
    );
    break;
  default:
    throw new Error(`unknown NNC8.2 mutation: ${mutation}`);
}

for (const producer of producers) {
  const source = read(producer.path);
  const claims = source.match(/ExecuteClaimed\s*\(\s*execution\s*\)/g) ?? [];
  const discarded = source.match(/ExecuteClaimed\s*\(\s*_\s*\)/g) ?? [];
  if (claims.length !== producer.claimCount || discarded.length !== 0) {
    diagnostic(
      "producer-token",
      `${producer.path} must retain ${producer.claimCount} exact ExecuteClaimed tokens and discard none`,
    );
  }
  const missingMarkers = producer.markers.filter(
    (marker) => !source.includes(marker),
  );
  if (missingMarkers.length !== 0) {
    diagnostic(
      "producer-interval",
      `${producer.path} lacks ${missingMarkers.join(", ")}`,
    );
  }
  for (const state of [
    "ProviderCommandObservationKind::Claimed",
    "ProviderCommandObservationKind::InProgress",
    "ProviderCommandObservationKind::Ambiguous",
  ]) {
    if (!source.includes(state)) {
      diagnostic("decision-matrix", `${producer.path} lacks ${state}`);
      break;
    }
  }
  const adoptedRecovery =
    /matches!\(\s*observation\.kind\(\),\s*ProviderCommandObservationKind::Claimed\s*\|\s*ProviderCommandObservationKind::InProgress\s*\|\s*ProviderCommandObservationKind::Ambiguous\s*\)/m;
  const adoptedRecoveryMatches =
    source.match(new RegExp(adoptedRecovery.source, "gm")) ?? [];
  if (adoptedRecoveryMatches.length !== producer.adoptedRecoveryCount) {
    diagnostic(
      "decision-matrix",
      `${producer.path} must recover adopted Claimed, InProgress, and Ambiguous observations in ${producer.adoptedRecoveryCount} locked inspection branches`,
    );
  }
  if (source.includes("record_effect")) {
    diagnostic(
      "duplicate-publication",
      `${producer.path} retains effect-before-record composition`,
    );
  }
}

const protocol = read(protocolPath);
const missingProtocol = protocolMarkers.filter(
  (marker) => !protocol.includes(marker),
);
if (missingProtocol.length !== 0) {
  diagnostic(
    "protocol-surface",
    `current-claim protocol lacks ${missingProtocol.join(", ")}`,
  );
}

for (const [relative, marker] of protectedTeardown) {
  if (!read(relative).includes(marker)) {
    diagnostic("protected-teardown", `${relative} no longer retains ${marker}`);
  }
}

if (diagnostics.length !== 0) {
  process.stderr.write(`${diagnostics.join("\n")}\n`);
  process.exit(1);
}

process.stdout.write("NNC8.2 provider-command current-claim contract: pass\n");
