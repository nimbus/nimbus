#!/usr/bin/env node

import fs from "node:fs";
import path from "node:path";

const repoRoot = process.cwd();
const mutation =
  process.env.NIMBUS_NETWORK_VERIFY_TEST_STARTUP_ORPHAN_MUTATION ?? "";

const sourcePaths = {
  network: "crates/nimbus-sandbox/src/backends/oci/network.rs",
  startup:
    "crates/nimbus-sandbox/src/backends/oci/network/startup_reconciliation.rs",
  convergence:
    "crates/nimbus-sandbox/src/backends/oci/network/orphan_convergence.rs",
  containerRoot:
    "crates/nimbus-sandbox/src/backends/container/runtime/network_composition.rs",
  containerCleanup:
    "crates/nimbus-sandbox/src/backends/container/runtime/startup_orphan_convergence.rs",
  krunRoot: "crates/nimbus-sandbox/src/backends/krun/vm.rs",
  krunCleanup:
    "crates/nimbus-sandbox/src/backends/krun/vm/startup_orphan_convergence.rs",
  containerTests:
    "crates/nimbus-sandbox/src/backends/container/runtime/tests/provider_cleanup/startup_fencing.rs",
  krunTests:
    "crates/nimbus-sandbox/src/backends/krun/vm/tests/startup_fencing.rs",
  reaperTests: "crates/nimbus-sandbox/src/backends/oci/network/reaper/tests.rs",
  portableSegment: "crates/nimbus-network/src/segment.rs",
  sandboxSegment: "crates/nimbus-sandbox/src/backends/oci/network/segment.rs",
  cluster: "crates/nimbus-sandbox/src/backends/oci/network/cluster.rs",
  cleanup: "crates/nimbus-sandbox/src/backends/oci/network/segment/cleanup.rs",
  testSupport: "crates/nimbus-sandbox/src/backends/oci/network/test_support.rs",
  reaper: "crates/nimbus-sandbox/src/backends/oci/network/reaper.rs",
};

function syntheticSources() {
  return {
    network: [
      "mod orphan_convergence;",
      "mod startup_reconciliation;",
      "pub(crate) use orphan_convergence::{OciOrphanCleanupContext, OciOrphanCleanupDisposition, OciOrphanCleanupKind, OciOrphanCleanupSubject};",
      "pub(crate) use startup_reconciliation::reconcile_startup_network_state_with_cleanup;",
    ].join("\n"),
    startup: [
      "use nimbus_network::{LocalNetworkAttachmentAuthority, NetworkResourcePhase, NetworkTransitionEvidence};",
      "fn reconcile_startup_network_state_with_cleanup(attachments: &LocalNetworkAttachmentAuthority, allocator: &OciSegmentAllocator, cleanup: &dyn OciOrphanCleanupContext) {",
      "  let report = collect_oci_orphan_evidence();",
      "  for classification in classify_oci_orphan_evidence(&report).candidate_classifications() {",
      "    match classification.disposition() {",
      "      OciOrphanDisposition::Adopt => {}",
      "      OciOrphanDisposition::Quarantine(_) => {",
      "        quarantine_candidate(attachments, allocator);",
      "        attachments.apply_transition(NetworkResourcePhase::CleanupPending, NetworkTransitionEvidence::AmbiguousEffect); allocator.quarantine();",
      "        if let Some(subject) = compile_cleanup_subject() { cleanup.converge_quarantined_orphan(&subject); }",
      "      }",
      "    }",
      "  }",
      "}",
    ].join("\n"),
    convergence: [
      "struct OciOrphanCleanupSubject;",
      "enum OciOrphanCleanupKind { NeverEffected, Effectful, TerminalPublication }",
      "trait OciOrphanCleanupContext { fn converge_quarantined_orphan(&self, subject: &OciOrphanCleanupSubject); }",
      "fn compile_cleanup_subject() { compile_terminal_publication_subject(candidate, reason); compile_effectful_cleanup_subject(candidate, reason); compile_never_effected_cleanup_subject(candidate, reason); }",
      "fn provider_operation_can_resume_effectful_cleanup() { NetavarkProviderOperation::Deleting; OciIpamEvidenceLifecycle::Terminal; }",
      "fn allocator_witnesses() { NetworkAttachmentReservationState::ProviderCleanupPending; NetworkAttachmentReservationState::Absent; }",
      "fn has_complete_effectful_artifacts() { OciArtifactObservationState::Present | OciArtifactObservationState::Absent; }",
      "fn compile_terminal_publication_subject() { OciOrphanQuarantineReason::DesiredAttachmentMissing; has_exact_absent_provider_allocator_witness(); }",
    ].join("\n"),
    containerRoot: "backend.reconcile_container_startup_network_state();",
    containerCleanup: [
      "impl OciOrphanCleanupContext for ContainerSandboxBackend {",
      "fn converge_quarantined_orphan(&self, subject: &OciOrphanCleanupSubject) {",
      "runner::lock_current_provision_lifecycle_for_backend(); read_exact_startup_manifest(); authenticate_container_orphan_subject();",
      "if subject.kind() == OciOrphanCleanupKind::TerminalPublication { write_existing_workload_manifest(); if subject.desired().is_none() { retire_terminal_container_ipam_release(); } }",
      "release_reserved_network_launch_after_ports_with_terminal_publication(); detach_host_managed(); AttachmentAuxiliaryDisposition::Unknown;",
      "}}",
      "reconcile_startup_manifest_publications(); retained_startup_manifest_paths();",
      "reconcile_startup_network_state_with_cleanup(&self.config.workload_state_root, attachments, &self.ipam_authority, self.segment_allocator.as_ref(), &retained_desired_manifests, self);",
    ].join("\n"),
    krunRoot: "backend.reconcile_krun_startup_network_state();",
    krunCleanup: [
      "impl OciOrphanCleanupContext for KrunSandboxBackend {",
      "fn converge_quarantined_orphan(&self, subject: &OciOrphanCleanupSubject) {",
      "lock_launch_lifecycle_for(); read_exact_manifest(); authenticate_krun_orphan_subject();",
      "if subject.kind() == OciOrphanCleanupKind::TerminalPublication { write_manifest(); if subject.desired().is_none() { retire_terminal_container_ipam_release(); } }",
      "release_reserved_network_launch_after_ports_with_terminal_publication(); detach_host_managed(); AttachmentAuxiliaryDisposition::Unknown;",
      "}}",
      "retained_krun_startup_manifest_paths();",
      "reconcile_startup_network_state_with_cleanup(&self.config.workload_state_root, attachments, &self.ipam_authority, self.segment_allocator.as_ref(), &retained_manifests, self);",
    ].join("\n"),
    containerTests: [
      "begin_host_managed_teardown_without_ack_for_test();",
      "fn nnc8_3_no_effect_terminal_publication_resumes_before_ipam_retirement() { terminal_container_ipam_release_is_absent_for_test(); }",
    ].join("\n"),
    krunTests: [
      'config.netavark_path = "/usr/bin/true".into();',
      "segment_allocator.quarantine();",
      "fn nnc8_3_krun_no_effect_terminal_publication_resumes_before_ipam_retirement() { terminal_container_ipam_release_is_absent_for_test(); }",
    ].join("\n"),
    reaperTests: [
      '#[ignore = "spawned only by the NNC8.3 segment-release crash parent"]',
      'fn nnc8_3_segment_release_crash_child() { std::env::var(SEGMENT_CRASH_ROOT).expect("crash child root should be set"); }',
      '.args(["--exact", SEGMENT_CRASH_CHILD, "--ignored", "--nocapture"]);',
    ].join("\n"),
    portableSegment: "pub trait NetworkSegmentAllocator {}",
    sandboxSegment: "impl NetworkSegmentAllocator for Allocator {}",
    cluster: "impl NetworkSegmentAllocator for ClusterAllocator {}",
    cleanup: "impl CleanupAllocator {}",
    testSupport: "impl NetworkSegmentAllocator for RecordingAllocator {}",
    reaper: [
      "fn reap_bridge_interface() {}",
      "fn release_reserved_network_launch_after_ports_with_terminal_publication() {}",
      "fn publish_then_retire_terminal_ipam() { publish_terminal(); retire_terminal_container_ipam_release(); }",
    ].join("\n"),
  };
}

function loadSources() {
  if (mutation) {
    const sources = syntheticSources();
    switch (mutation) {
      case "legacy-live-set":
        sources.portableSegment +=
          "\nfn reconcile_orphans(live: &BTreeSet<(TenantId, NetworkAttachmentId)>) {}";
        break;
      case "missing-container-injection":
        sources.containerRoot =
          "fn construct_container_without_reconciliation() {}";
        break;
      case "missing-krun-injection":
        sources.krunRoot = "fn construct_krun_without_reconciliation() {}";
        break;
      case "cleanup-capability":
        sources.startup += "\nrelease_network_segment_hold();";
        break;
      case "missing-exact-quarantine":
        sources.startup = sources.startup
          .replace("quarantine_candidate", "inspect_candidate")
          .replace("attachments.apply_transition", "inspect_only")
          .replace("allocator.quarantine", "inspect_allocator");
        break;
      case "cleanup-before-quarantine":
        sources.startup = sources.startup.replace(
          "quarantine_candidate(attachments, allocator);",
          "compile_cleanup_subject(); quarantine_candidate(attachments, allocator);",
        );
        break;
      case "missing-cleanup-subject":
        sources.convergence = "trait UnrelatedCapability {}";
        break;
      case "missing-container-context":
        sources.containerCleanup = "impl ContainerSandboxBackend {}";
        break;
      case "missing-krun-context":
        sources.krunCleanup = "impl KrunSandboxBackend {}";
        break;
      case "generic-effect-capability":
        sources.convergence += "\nstd::fs::remove_file(path);";
        break;
      case "missing-deleting-resume":
        sources.convergence = sources.convergence.replace(
          "NetavarkProviderOperation::Deleting",
          "NetavarkProviderOperation::Ready",
        );
        break;
      case "missing-terminal-resume":
        sources.convergence = sources.convergence.replace(
          "OciIpamEvidenceLifecycle::Terminal",
          "OciIpamEvidenceLifecycle::Live",
        );
        break;
      case "absent-only-effectful-artifacts":
        sources.convergence = sources.convergence.replace(
          "OciArtifactObservationState::Present | OciArtifactObservationState::Absent",
          "OciArtifactObservationState::Absent",
        );
        break;
      case "terminal-after-effectful":
        sources.convergence = sources.convergence.replace(
          "compile_terminal_publication_subject(candidate, reason); compile_effectful_cleanup_subject(candidate, reason);",
          "compile_effectful_cleanup_subject(candidate, reason); compile_terminal_publication_subject(candidate, reason);",
        );
        break;
      case "retire-before-publication":
        sources.reaper = sources.reaper.replace(
          "publish_terminal(); retire_terminal_container_ipam_release();",
          "retire_terminal_container_ipam_release(); publish_terminal();",
        );
        break;
      case "missing-publication-cut-proof":
        sources.containerTests = sources.containerTests.replace(
          "terminal_container_ipam_release_is_absent_for_test",
          "terminal_ipam_retirement_is_not_checked",
        );
        sources.krunTests = sources.krunTests.replace(
          "terminal_container_ipam_release_is_absent_for_test",
          "terminal_ipam_retirement_is_not_checked",
        );
        break;
      case "no-op-crash-child":
        sources.reaperTests = sources.reaperTests
          .replace(/#\[ignore[^\n]*\]\n/, "")
          .replace('.expect("crash child root should be set")', ".ok()");
        break;
      default:
        throw new Error(`unknown NNCV018 mutation: ${mutation}`);
    }
    return sources;
  }

  const sources = {};
  const errors = [];
  for (const [key, relative] of Object.entries(sourcePaths)) {
    const absolute = path.join(repoRoot, relative);
    if (!fs.existsSync(absolute)) {
      errors.push(`missing required source: ${relative}`);
      continue;
    }
    sources[key] = fs.readFileSync(absolute, "utf8");
  }
  return { sources, errors };
}

function verify(input) {
  const errors = Array.isArray(input.errors) ? [...input.errors] : [];
  const sources = input.sources ?? input;

  const legacyScan = [
    "portableSegment",
    "sandboxSegment",
    "cluster",
    "cleanup",
    "testSupport",
    "reaper",
    "network",
  ]
    .map((key) => sources[key] ?? "")
    .join("\n");
  for (const legacy of [
    "fn reconcile_orphans",
    "reconcile_network_segment_orphans",
    "live_netns_holds",
  ]) {
    if (legacyScan.includes(legacy)) {
      errors.push(`legacy filename/live-set authority remains: ${legacy}`);
    }
  }

  const network = sources.network ?? "";
  const startup = sources.startup ?? "";
  for (const ownedModule of [
    "mod orphan_convergence",
    "mod startup_reconciliation",
  ]) {
    if (!network.includes(ownedModule)) {
      errors.push(`OCI network composition does not own ${ownedModule}`);
    }
  }
  if (
    !/pub\s*\(\s*crate\s*\)\s+use\s+startup_reconciliation\s*::\s*reconcile_startup_network_state_with_cleanup\b/s.test(
      network,
    )
  ) {
    errors.push(
      "OCI network composition does not export cleanup-aware startup reconciliation",
    );
  }
  for (const cleanupType of [
    "OciOrphanCleanupContext",
    "OciOrphanCleanupDisposition",
    "OciOrphanCleanupKind",
    "OciOrphanCleanupSubject",
  ]) {
    if (!network.includes(cleanupType)) {
      errors.push(`OCI network composition does not export ${cleanupType}`);
    }
  }

  for (const token of [
    "LocalNetworkAttachmentAuthority",
    "collect_oci_orphan_evidence",
    "classify_oci_orphan_evidence",
    "OciOrphanDisposition::Adopt",
    "OciOrphanDisposition::Quarantine",
    "quarantine_candidate",
    "NetworkResourcePhase::CleanupPending",
    "NetworkTransitionEvidence::AmbiguousEffect",
    ".apply_transition",
    ".quarantine",
    "compile_cleanup_subject",
    "converge_quarantined_orphan",
  ]) {
    if (!startup.includes(token)) {
      errors.push(`startup reconciler lacks required exact seam: ${token}`);
    }
  }

  const reconciliationBodyStart = startup.indexOf(
    "let report = collect_oci_orphan_evidence",
  );
  const reconciliationBody = startup.slice(reconciliationBodyStart);
  const quarantineIndex = reconciliationBody.indexOf("quarantine_candidate");
  const cleanupSubjectIndex = reconciliationBody.indexOf(
    "compile_cleanup_subject",
  );
  const cleanupEffectIndex = reconciliationBody.indexOf(
    "converge_quarantined_orphan",
  );
  if (
    reconciliationBodyStart < 0 ||
    quarantineIndex < 0 ||
    cleanupSubjectIndex < quarantineIndex ||
    cleanupEffectIndex < cleanupSubjectIndex
  ) {
    errors.push(
      "startup cleanup selection or execution can precede durable quarantine",
    );
  }

  for (const forbidden of [
    "reconcile_terminal_container_ipam_releases",
    "release_network_segment_hold",
    "finalize_release",
    "remove_persistent_network_namespace",
    "teardown_container_network",
    "reap_bridge_interface",
    ".release(",
  ]) {
    if (startup.includes(forbidden)) {
      errors.push(
        `startup reconciler gained forbidden cleanup capability: ${forbidden}`,
      );
    }
  }

  const convergence = sources.convergence ?? "";
  for (const token of [
    "OciOrphanCleanupSubject",
    "OciOrphanCleanupKind",
    "NeverEffected",
    "Effectful",
    "TerminalPublication",
    "OciOrphanCleanupContext",
    "converge_quarantined_orphan",
  ]) {
    if (!convergence.includes(token)) {
      errors.push(`generic cleanup seam lacks ${token}`);
    }
  }
  for (const forbidden of [
    "std::fs",
    "std::process",
    "detach_host_managed",
    "release_network_segment_hold",
    "release_reserved_launch",
    "reap_bridge_interface",
  ]) {
    if (convergence.includes(forbidden)) {
      errors.push(
        `generic cleanup seam gained provider effect capability: ${forbidden}`,
      );
    }
  }

  for (const recoveryToken of [
    "NetavarkProviderOperation::Deleting",
    "OciIpamEvidenceLifecycle::Terminal",
    "NetworkAttachmentReservationState::ProviderCleanupPending",
    "NetworkAttachmentReservationState::Absent",
    "has_complete_effectful_artifacts",
    "OciArtifactObservationState::Present | OciArtifactObservationState::Absent",
    "OciOrphanQuarantineReason::DesiredAttachmentMissing",
    "has_exact_absent_provider_allocator_witness",
  ]) {
    if (!convergence.includes(recoveryToken)) {
      errors.push(
        `generic cleanup compiler lacks recovery row: ${recoveryToken}`,
      );
    }
  }
  const compilerStart = convergence.indexOf("fn compile_cleanup_subject");
  const compilerEnd = convergence.indexOf(
    "fn compile_effectful_cleanup_subject",
    compilerStart,
  );
  const compilerBody = convergence.slice(
    compilerStart,
    compilerEnd < 0 ? undefined : compilerEnd,
  );
  if (
    compilerStart < 0 ||
    compilerBody.indexOf("compile_terminal_publication_subject") < 0 ||
    compilerBody.indexOf("compile_effectful_cleanup_subject") <
      compilerBody.indexOf("compile_terminal_publication_subject")
  ) {
    errors.push(
      "terminal manifest publication is not selected before effectful cleanup",
    );
  }

  const reaper = sources.reaper ?? "";
  if (
    !reaper.includes(
      "release_reserved_network_launch_after_ports_with_terminal_publication",
    )
  ) {
    errors.push("never-effected release lacks terminal publication callback");
  }
  const publishOwnerStart = reaper.indexOf(
    "fn publish_then_retire_terminal_ipam",
  );
  const publishOwner = reaper.slice(publishOwnerStart);
  if (
    publishOwnerStart < 0 ||
    publishOwner.indexOf("publish_terminal") < 0 ||
    publishOwner.indexOf("retire_terminal_container_ipam_release") <
      publishOwner.indexOf("publish_terminal")
  ) {
    errors.push(
      "terminal IPAM evidence can retire before manifest publication",
    );
  }

  const backendContracts = [
    {
      label: "container",
      root: sources.containerRoot ?? "",
      rootCall: "reconcile_container_startup_network_state",
      adapter: sources.containerCleanup ?? "",
      terminalPublicationCall: "write_existing_workload_manifest",
      required: [
        "impl OciOrphanCleanupContext for ContainerSandboxBackend",
        "reconcile_startup_manifest_publications",
        "retained_startup_manifest_paths",
        "reconcile_startup_network_state_with_cleanup",
        "lock_current_provision_lifecycle_for_backend",
        "read_exact_startup_manifest",
        "authenticate_container_orphan_subject",
        "release_reserved_network_launch_after_ports_with_terminal_publication",
        "detach_host_managed",
        "AttachmentAuxiliaryDisposition::Unknown",
      ],
    },
    {
      label: "krun",
      root: sources.krunRoot ?? "",
      rootCall: "reconcile_krun_startup_network_state",
      adapter: sources.krunCleanup ?? "",
      terminalPublicationCall: "write_manifest",
      required: [
        "impl OciOrphanCleanupContext for KrunSandboxBackend",
        "retained_krun_startup_manifest_paths",
        "reconcile_startup_network_state_with_cleanup",
        "lock_launch_lifecycle_for",
        "read_exact_manifest",
        "authenticate_krun_orphan_subject",
        "release_reserved_network_launch_after_ports_with_terminal_publication",
        "detach_host_managed",
        "AttachmentAuxiliaryDisposition::Unknown",
      ],
    },
  ];
  for (const contract of backendContracts) {
    if (!contract.root.includes(contract.rootCall)) {
      errors.push(
        `${contract.label} composition root does not invoke cleanup-aware startup reconciliation`,
      );
    }
    for (const token of contract.required) {
      if (!contract.adapter.includes(token)) {
        errors.push(
          `${contract.label} cleanup context lacks required seam: ${token}`,
        );
      }
    }
    const convergeStart = contract.adapter.indexOf(
      "fn converge_quarantined_orphan",
    );
    const terminalStart = contract.adapter.indexOf(
      "if subject.kind() == OciOrphanCleanupKind::TerminalPublication",
      convergeStart,
    );
    const terminalEnd = contract.adapter.indexOf(
      "let reservation =",
      terminalStart,
    );
    const terminalBody = contract.adapter.slice(
      terminalStart,
      terminalEnd < 0 ? undefined : terminalEnd,
    );
    const publication = terminalBody.indexOf(contract.terminalPublicationCall);
    const noDesired = terminalBody.indexOf("subject.desired().is_none()");
    const retirement = terminalBody.indexOf(
      "retire_terminal_container_ipam_release",
    );
    if (
      convergeStart < 0 ||
      terminalStart < 0 ||
      publication < 0 ||
      noDesired < 0 ||
      retirement < publication ||
      retirement < noDesired
    ) {
      errors.push(
        `${contract.label} terminal-publication recovery does not publish before same-process no-desired IPAM retirement`,
      );
    }
  }

  const containerTests = sources.containerTests ?? "";
  for (const token of [
    "begin_host_managed_teardown_without_ack_for_test",
    "nnc8_3_no_effect_terminal_publication_resumes_before_ipam_retirement",
    "terminal_container_ipam_release_is_absent_for_test",
  ]) {
    if (!containerTests.includes(token)) {
      errors.push(`container correction proof lacks ${token}`);
    }
  }
  const krunTests = sources.krunTests ?? "";
  for (const token of [
    'netavark_path = "/usr/bin/true"',
    ".quarantine(",
    "nnc8_3_krun_no_effect_terminal_publication_resumes_before_ipam_retirement",
    "terminal_container_ipam_release_is_absent_for_test",
  ]) {
    if (!krunTests.includes(token)) {
      errors.push(`krun correction proof lacks ${token}`);
    }
  }
  const reaperTests = sources.reaperTests ?? "";
  if (
    !/#\[ignore\s*=\s*"spawned only by the NNC8\.3 segment-release crash parent"\]\s*fn nnc8_3_segment_release_crash_child/s.test(
      reaperTests,
    ) ||
    !reaperTests.includes('"--ignored"') ||
    !reaperTests.includes('.expect("crash child root should be set")')
  ) {
    errors.push(
      "segment crash child is not ignored, explicitly invoked, and fail-closed",
    );
  }

  return errors;
}

let loaded;
try {
  loaded = loadSources();
} catch (error) {
  process.stderr.write(`${error.message}\n`);
  process.exit(1);
}
const errors = verify(loaded);
if (errors.length > 0) {
  process.stderr.write(`${errors.join("\n")}\n`);
  process.exit(1);
}
