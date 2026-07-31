//! Deterministic read-only evidence collection for startup reconciliation.
//!
//! This module joins portable attachment authority, exact allocator
//! observations, sandbox provider-attempt authority, and untrusted artifact
//! observations. It deliberately cannot classify, quarantine, clean, release,
//! finalize, or reuse any resource.

use std::collections::{BTreeMap, BTreeSet};
use std::io;
use std::path::{Path, PathBuf};

use cap_std::ambient_authority;
use cap_std::fs::{Dir, DirEntry};
use nimbus_core::TenantId;
use nimbus_network::{
    DurableNetworkAttachmentState, NetworkAttachmentId, NetworkAttachmentReservationObservation,
    NetworkReservationClaim,
};

use super::OciNetworkLayout;
use super::ipam::OciAttachmentProviderEvidence;
use crate::error::{Result, SandboxError};

mod readers;
use readers::{
    OciDesiredAttachmentEvidenceReader, OciExactAllocatorEvidenceReader,
    OciProviderAttemptEvidenceReader,
};

mod classifier;
pub(in crate::backends::oci::network) use classifier::{
    OciOrphanDisposition, OciOrphanQuarantineReason, classify_oci_orphan_evidence,
};

/// Durable authority that supplied a claim-qualified allocator observation.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OciAllocatorEvidenceSource {
    DesiredAttachment,
    ProviderAttempt,
}

/// A read-only result from the injected allocator for one exact claim.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciExactAllocatorEvidence {
    source: OciAllocatorEvidenceSource,
    reservation_claim: NetworkReservationClaim,
    observation: std::result::Result<NetworkAttachmentReservationObservation, OciEvidenceUnknown>,
}

impl OciExactAllocatorEvidence {
    pub(crate) fn source(&self) -> OciAllocatorEvidenceSource {
        self.source
    }

    pub(crate) fn reservation_claim(&self) -> &NetworkReservationClaim {
        &self.reservation_claim
    }

    pub(crate) fn observation(
        &self,
    ) -> std::result::Result<&NetworkAttachmentReservationObservation, &OciEvidenceUnknown> {
        self.observation.as_ref()
    }
}

/// Provider artifact kind observed beneath the injected workload root.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub(crate) enum OciArtifactKind {
    Manifest,
    NetworkNamespace,
    Status,
}

/// Tri-state artifact observation. Only `NotFound` becomes `Absent`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OciArtifactObservationState {
    Present,
    Absent,
    Unknown(OciEvidenceUnknown),
}

/// Untrusted observation of one exact or unmatched provider artifact path.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciArtifactObservation {
    kind: OciArtifactKind,
    path: PathBuf,
    state: OciArtifactObservationState,
}

impl OciArtifactObservation {
    pub(crate) fn kind(&self) -> OciArtifactKind {
        self.kind
    }

    pub(crate) fn path(&self) -> &Path {
        &self.path
    }

    pub(crate) fn state(&self) -> &OciArtifactObservationState {
        &self.state
    }
}

impl OciArtifactKind {
    pub(crate) const fn as_str(self) -> &'static str {
        match self {
            Self::Manifest => "manifest artifact",
            Self::NetworkNamespace => "network namespace artifact",
            Self::Status => "provider status artifact",
        }
    }
}

/// Typed unknown evidence retained instead of being flattened or downgraded.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciEvidenceUnknown {
    operation: &'static str,
    path: Option<PathBuf>,
    error_kind: String,
    message: String,
}

impl OciEvidenceUnknown {
    pub(crate) fn operation(&self) -> &'static str {
        self.operation
    }

    pub(crate) fn path(&self) -> Option<&Path> {
        self.path.as_deref()
    }

    pub(crate) fn error_kind(&self) -> &str {
        &self.error_kind
    }

    pub(crate) fn message(&self) -> &str {
        &self.message
    }

    fn io(operation: &'static str, path: &Path, error: io::Error) -> Self {
        Self {
            operation,
            path: Some(path.to_path_buf()),
            error_kind: format!("{:?}", error.kind()),
            message: error.to_string(),
        }
    }

    fn domain(operation: &'static str, message: impl Into<String>) -> Self {
        Self {
            operation,
            path: None,
            error_kind: "Domain".to_owned(),
            message: message.into(),
        }
    }

    fn unexpected_type(operation: &'static str, path: &Path, expected: &str) -> Self {
        Self {
            operation,
            path: Some(path.to_path_buf()),
            error_kind: "UnexpectedFileType".to_owned(),
            message: format!("expected {expected} at {}", path.display()),
        }
    }
}

/// One tenant-qualified union candidate. No field is a disposition.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciOrphanEvidenceCandidate {
    tenant_id: TenantId,
    attachment_id: NetworkAttachmentId,
    desired: Option<DurableNetworkAttachmentState>,
    provider: Option<OciAttachmentProviderEvidence>,
    allocator: Vec<OciExactAllocatorEvidence>,
    artifacts: Vec<OciArtifactObservation>,
}

impl OciOrphanEvidenceCandidate {
    pub(crate) fn tenant_id(&self) -> &TenantId {
        &self.tenant_id
    }

    pub(crate) fn attachment_id(&self) -> &NetworkAttachmentId {
        &self.attachment_id
    }

    pub(crate) fn desired(&self) -> Option<&DurableNetworkAttachmentState> {
        self.desired.as_ref()
    }

    pub(crate) fn provider(&self) -> Option<&OciAttachmentProviderEvidence> {
        self.provider.as_ref()
    }

    pub(crate) fn allocator(&self) -> &[OciExactAllocatorEvidence] {
        &self.allocator
    }

    pub(crate) fn artifacts(&self) -> &[OciArtifactObservation] {
        &self.artifacts
    }
}

/// Why durable provider evidence could not join the current artifact realm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum OciProviderRealmObservation {
    DifferentRealm,
    Unknown(OciEvidenceUnknown),
}

/// Durable IPAM evidence retained outside the current-root candidate union.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciUnmatchedProviderEvidence {
    evidence: OciAttachmentProviderEvidence,
    realm: OciProviderRealmObservation,
}

impl OciUnmatchedProviderEvidence {
    pub(crate) fn evidence(&self) -> &OciAttachmentProviderEvidence {
        &self.evidence
    }

    pub(crate) fn realm(&self) -> &OciProviderRealmObservation {
        &self.realm
    }
}

/// Complete deterministic read-only snapshot for one injected artifact realm.
#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct OciOrphanEvidenceReport {
    candidates: Vec<OciOrphanEvidenceCandidate>,
    unmatched_provider_evidence: Vec<OciUnmatchedProviderEvidence>,
    unmatched_artifacts: Vec<OciArtifactObservation>,
    artifact_scan_unknowns: Vec<OciEvidenceUnknown>,
}

impl OciOrphanEvidenceReport {
    pub(crate) fn candidates(&self) -> &[OciOrphanEvidenceCandidate] {
        &self.candidates
    }

    pub(crate) fn unmatched_provider_evidence(&self) -> &[OciUnmatchedProviderEvidence] {
        &self.unmatched_provider_evidence
    }

    pub(crate) fn unmatched_artifacts(&self) -> &[OciArtifactObservation] {
        &self.unmatched_artifacts
    }

    pub(crate) fn artifact_scan_unknowns(&self) -> &[OciEvidenceUnknown] {
        &self.artifact_scan_unknowns
    }
}

#[derive(Default)]
struct CandidateBuilder {
    desired: Option<DurableNetworkAttachmentState>,
    provider: Option<OciAttachmentProviderEvidence>,
}

/// Collect exact durable and observed evidence without selecting a winner.
pub(in crate::backends::oci::network) fn collect_oci_orphan_evidence<Allocator>(
    workload_state_root: &Path,
    attachments: &dyn OciDesiredAttachmentEvidenceReader,
    ipam: &dyn OciProviderAttemptEvidenceReader,
    allocator: &Allocator,
) -> Result<OciOrphanEvidenceReport>
where
    Allocator: OciExactAllocatorEvidenceReader + ?Sized,
{
    let artifact_realm = PinnedArtifactRealm::open(workload_state_root);
    let mut builders = BTreeMap::<String, CandidateBuilder>::new();
    for desired in attachments.list_desired_attachment_evidence()? {
        let attachment_id = desired
            .attachment_id()
            .map_err(attachment_state_error)?
            .clone();
        let key = candidate_key(desired.tenant_id(), &attachment_id);
        let slot = builders.entry(key).or_default();
        if slot.desired.replace(desired).is_some() {
            return Err(corrupt_evidence(
                "attachment authority returned a duplicate tenant-qualified record",
            ));
        }
    }

    let mut unmatched_provider_evidence = Vec::new();
    for provider in ipam.list_provider_attempt_evidence()? {
        match artifact_realm.authenticates_provider(&provider) {
            Ok(true) => {
                let key = candidate_key(provider.tenant_id(), provider.attachment_id());
                let slot = builders.entry(key).or_default();
                if slot.provider.replace(provider).is_some() {
                    return Err(corrupt_evidence(
                        "IPAM authority returned a duplicate tenant-qualified provider record",
                    ));
                }
            }
            Ok(false) => unmatched_provider_evidence.push(OciUnmatchedProviderEvidence {
                evidence: provider,
                realm: OciProviderRealmObservation::DifferentRealm,
            }),
            Err(error) => unmatched_provider_evidence.push(OciUnmatchedProviderEvidence {
                evidence: provider,
                realm: OciProviderRealmObservation::Unknown(OciEvidenceUnknown::domain(
                    "authenticate artifact realm",
                    error.to_string(),
                )),
            }),
        }
    }

    let mut expected_artifacts = BTreeSet::new();
    let mut candidates = Vec::with_capacity(builders.len());
    for (_, builder) in builders {
        let (tenant_id, attachment_id) = candidate_identity(&builder)?;
        let mut allocator_evidence = Vec::new();
        if let Some(desired) = &builder.desired {
            allocator_evidence.push(inspect_allocator(
                allocator,
                OciAllocatorEvidenceSource::DesiredAttachment,
                &tenant_id,
                &attachment_id,
                desired.association().reservation_claim(),
            ));
        }
        if let Some(provider) = &builder.provider {
            allocator_evidence.push(inspect_allocator(
                allocator,
                OciAllocatorEvidenceSource::ProviderAttempt,
                &tenant_id,
                &attachment_id,
                provider.reservation_claim(),
            ));
        }

        let artifacts = if let Some(provider) = &builder.provider {
            let layout = OciNetworkLayout::with_roots(
                workload_state_root,
                ipam.network_state_root(),
                &tenant_id,
                provider.sandbox_id(),
            );
            let paths = [
                (
                    OciArtifactKind::Manifest,
                    crate::artifact_paths::manifest_path(
                        workload_state_root,
                        &tenant_id,
                        provider.sandbox_id(),
                    ),
                ),
                (OciArtifactKind::NetworkNamespace, layout.netns_path.clone()),
                (OciArtifactKind::Status, layout.status_path.clone()),
            ];
            paths
                .into_iter()
                .map(|(kind, path)| {
                    expected_artifacts.insert((kind, path.clone()));
                    artifact_realm.observe(kind, path)
                })
                .collect()
        } else {
            Vec::new()
        };
        candidates.push(OciOrphanEvidenceCandidate {
            tenant_id,
            attachment_id,
            desired: builder.desired,
            provider: builder.provider,
            allocator: allocator_evidence,
            artifacts,
        });
    }
    candidates.sort_by(|left, right| {
        (left.tenant_id.as_str(), left.attachment_id.as_str())
            .cmp(&(right.tenant_id.as_str(), right.attachment_id.as_str()))
    });

    let (observed_artifacts, mut artifact_scan_unknowns) =
        scan_current_root_artifacts(workload_state_root, &artifact_realm);
    let mut unmatched_artifacts = observed_artifacts
        .into_iter()
        .filter(|artifact| !expected_artifacts.contains(&(artifact.kind, artifact.path.clone())))
        .collect::<Vec<_>>();
    unmatched_artifacts
        .sort_by(|left, right| (left.kind, &left.path).cmp(&(right.kind, &right.path)));
    artifact_scan_unknowns.sort_by(|left, right| {
        (
            left.path.as_deref(),
            left.operation,
            left.error_kind.as_str(),
            left.message.as_str(),
        )
            .cmp(&(
                right.path.as_deref(),
                right.operation,
                right.error_kind.as_str(),
                right.message.as_str(),
            ))
    });
    unmatched_provider_evidence.sort_by(|left, right| {
        (
            left.evidence.tenant_id().as_str(),
            left.evidence.attachment_id().as_str(),
        )
            .cmp(&(
                right.evidence.tenant_id().as_str(),
                right.evidence.attachment_id().as_str(),
            ))
    });

    Ok(OciOrphanEvidenceReport {
        candidates,
        unmatched_provider_evidence,
        unmatched_artifacts,
        artifact_scan_unknowns,
    })
}

fn candidate_identity(builder: &CandidateBuilder) -> Result<(TenantId, NetworkAttachmentId)> {
    match (&builder.desired, &builder.provider) {
        (Some(desired), Some(provider)) => {
            let desired_attachment = desired.attachment_id().map_err(attachment_state_error)?;
            if desired.tenant_id() != provider.tenant_id()
                || desired_attachment != provider.attachment_id()
            {
                return Err(corrupt_evidence(
                    "candidate builder combined different tenant-qualified identities",
                ));
            }
            Ok((desired.tenant_id().clone(), desired_attachment.clone()))
        }
        (Some(desired), None) => Ok((
            desired.tenant_id().clone(),
            desired
                .attachment_id()
                .map_err(attachment_state_error)?
                .clone(),
        )),
        (None, Some(provider)) => Ok((
            provider.tenant_id().clone(),
            provider.attachment_id().clone(),
        )),
        (None, None) => Err(corrupt_evidence(
            "candidate builder has no durable authority source",
        )),
    }
}

fn inspect_allocator<Allocator>(
    allocator: &Allocator,
    source: OciAllocatorEvidenceSource,
    tenant_id: &TenantId,
    attachment_id: &NetworkAttachmentId,
    reservation_claim: &NetworkReservationClaim,
) -> OciExactAllocatorEvidence
where
    Allocator: OciExactAllocatorEvidenceReader + ?Sized,
{
    let observation = allocator
        .inspect_exact_attachment_reservation(tenant_id, attachment_id, reservation_claim)
        .map_err(|error| {
            OciEvidenceUnknown::domain("inspect exact allocator reservation", error.to_string())
        });
    OciExactAllocatorEvidence {
        source,
        reservation_claim: reservation_claim.clone(),
        observation,
    }
}

struct PinnedArtifactRealm {
    root_path: PathBuf,
    root: std::result::Result<Option<Dir>, OciEvidenceUnknown>,
}

impl PinnedArtifactRealm {
    fn open(root_path: &Path) -> Self {
        let root = match Dir::open_ambient_dir(root_path, ambient_authority()) {
            Ok(root) => Ok(Some(root)),
            Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
            Err(error) => Err(OciEvidenceUnknown::io(
                "open authenticated artifact realm",
                root_path,
                error,
            )),
        };
        Self {
            root_path: root_path.to_path_buf(),
            root,
        }
    }

    fn observe(&self, kind: OciArtifactKind, path: PathBuf) -> OciArtifactObservation {
        let state = match &self.root {
            Ok(Some(root)) => {
                let relative = match path.strip_prefix(&self.root_path) {
                    Ok(relative) => relative,
                    Err(_) => {
                        return OciArtifactObservation {
                            kind,
                            state: OciArtifactObservationState::Unknown(
                                OciEvidenceUnknown::domain(
                                    "inspect exact artifact",
                                    format!(
                                        "artifact {} is outside authenticated root {}",
                                        path.display(),
                                        self.root_path.display()
                                    ),
                                ),
                            ),
                            path,
                        };
                    }
                };
                observe_relative_artifact(root, relative, &path, "inspect exact artifact")
            }
            Ok(None) => OciArtifactObservationState::Absent,
            Err(error) => OciArtifactObservationState::Unknown(error.clone()),
        };
        OciArtifactObservation { kind, path, state }
    }

    fn authenticates_provider(&self, provider: &OciAttachmentProviderEvidence) -> Result<bool> {
        let root = match self.root.as_ref() {
            Ok(Some(root)) => root,
            Ok(None) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to authenticate pinned OCI artifact realm {}: directory is absent",
                        self.root_path.display()
                    ),
                });
            }
            Err(error) => {
                return Err(SandboxError::OperationFailed {
                    message: format!(
                        "failed to authenticate pinned OCI artifact realm {}: {}",
                        self.root_path.display(),
                        error.message()
                    ),
                });
            }
        };
        provider.authenticates_open_directory(root)
    }
}

struct PinnedArtifactDirectory {
    path: PathBuf,
    dir: Dir,
}

fn scan_current_root_artifacts(
    workload_state_root: &Path,
    realm: &PinnedArtifactRealm,
) -> (Vec<OciArtifactObservation>, Vec<OciEvidenceUnknown>) {
    let mut artifacts = Vec::new();
    let mut unknowns = Vec::new();
    let root = match &realm.root {
        Ok(Some(root)) => root,
        Ok(None) => return (artifacts, unknowns),
        Err(error) => {
            unknowns.push(error.clone());
            return (artifacts, unknowns);
        }
    };
    let tenants = directories_under(
        root,
        Path::new("tenants"),
        &workload_state_root.join("tenants"),
        "enumerate tenant artifact roots",
        &mut unknowns,
    );
    for tenant_root in tenants {
        for (entry, path) in directory_entries(
            &tenant_root.dir,
            Path::new("networks/netns"),
            &tenant_root.path.join("networks").join("netns"),
            "enumerate persistent network namespaces",
            &mut unknowns,
        ) {
            artifacts.push(observe_artifact_entry(
                OciArtifactKind::NetworkNamespace,
                &entry,
                path,
                "enumerate persistent network namespaces",
            ));
        }
        for container_root in directories_under(
            &tenant_root.dir,
            Path::new("networks/containers"),
            &tenant_root.path.join("networks").join("containers"),
            "enumerate network status owners",
            &mut unknowns,
        ) {
            let path = container_root.path.join("status.json");
            let state = observe_relative_artifact(
                &container_root.dir,
                Path::new("status.json"),
                &path,
                "inspect network status artifact",
            );
            if !matches!(state, OciArtifactObservationState::Absent) {
                artifacts.push(OciArtifactObservation {
                    kind: OciArtifactKind::Status,
                    path,
                    state,
                });
            }
        }
        for sandbox_root in directories_under(
            &tenant_root.dir,
            Path::new("sandboxes"),
            &tenant_root.path.join("sandboxes"),
            "enumerate sandbox artifact owners",
            &mut unknowns,
        ) {
            let Some(sandbox_name) = sandbox_root.path.file_name() else {
                unknowns.push(OciEvidenceUnknown::unexpected_type(
                    "identify sandbox artifact path",
                    &sandbox_root.path,
                    "a named directory",
                ));
                continue;
            };
            let relative_manifest = Path::new("state")
                .join("containers")
                .join(sandbox_name)
                .join("manifest.json");
            let manifest = sandbox_root.path.join(&relative_manifest);
            let state = observe_relative_artifact(
                &sandbox_root.dir,
                &relative_manifest,
                &manifest,
                "inspect sandbox manifest artifact",
            );
            if !matches!(state, OciArtifactObservationState::Absent) {
                artifacts.push(OciArtifactObservation {
                    kind: OciArtifactKind::Manifest,
                    path: manifest,
                    state,
                });
            }
        }
    }
    artifacts.sort_by(|left, right| (left.kind, &left.path).cmp(&(right.kind, &right.path)));
    (artifacts, unknowns)
}

fn directories_under(
    base: &Dir,
    relative_root: &Path,
    display_root: &Path,
    operation: &'static str,
    unknowns: &mut Vec<OciEvidenceUnknown>,
) -> Vec<PinnedArtifactDirectory> {
    directory_entries(base, relative_root, display_root, operation, unknowns)
        .into_iter()
        .filter_map(|(entry, path)| match entry.file_type() {
            Ok(file_type) if file_type.is_dir() && !file_type.is_symlink() => {
                match entry.open_dir() {
                    Ok(dir) => Some(PinnedArtifactDirectory { path, dir }),
                    Err(error) if error.kind() == io::ErrorKind::NotFound => None,
                    Err(error) => {
                        unknowns.push(OciEvidenceUnknown::io(operation, &path, error));
                        None
                    }
                }
            }
            Ok(_) => {
                unknowns.push(OciEvidenceUnknown::unexpected_type(
                    operation,
                    &path,
                    "a non-symlink directory",
                ));
                None
            }
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => {
                unknowns.push(OciEvidenceUnknown::io(operation, &path, error));
                None
            }
        })
        .collect()
}

fn directory_entries(
    base: &Dir,
    relative_root: &Path,
    display_root: &Path,
    operation: &'static str,
    unknowns: &mut Vec<OciEvidenceUnknown>,
) -> Vec<(DirEntry, PathBuf)> {
    let directory = match open_nonsymlink_directory(base, relative_root, display_root, operation) {
        Ok(Some(directory)) => directory,
        Ok(None) => return Vec::new(),
        Err(unknown) => {
            unknowns.push(unknown);
            return Vec::new();
        }
    };
    let entries = match directory.entries() {
        Ok(entries) => entries,
        Err(error) if error.kind() == io::ErrorKind::NotFound => return Vec::new(),
        Err(error) => {
            unknowns.push(OciEvidenceUnknown::io(operation, display_root, error));
            return Vec::new();
        }
    };
    let mut entries_with_paths = Vec::new();
    for entry in entries {
        match entry {
            Ok(entry) => {
                let path = display_root.join(entry.file_name());
                entries_with_paths.push((entry, path));
            }
            Err(error) => unknowns.push(OciEvidenceUnknown::io(operation, display_root, error)),
        }
    }
    entries_with_paths.sort_by(|left, right| left.1.cmp(&right.1));
    entries_with_paths
}

fn open_nonsymlink_directory(
    base: &Dir,
    relative_path: &Path,
    display_path: &Path,
    operation: &'static str,
) -> std::result::Result<Option<Dir>, OciEvidenceUnknown> {
    match base.symlink_metadata(relative_path) {
        Ok(metadata) if metadata.file_type().is_dir() && !metadata.file_type().is_symlink() => base
            .open_dir(relative_path)
            .map(Some)
            .map_err(|error| OciEvidenceUnknown::io(operation, display_path, error)),
        Ok(_) => Err(OciEvidenceUnknown::unexpected_type(
            operation,
            display_path,
            "a non-symlink directory",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(error) => Err(OciEvidenceUnknown::io(operation, display_path, error)),
    }
}

fn observe_relative_artifact(
    base: &Dir,
    relative_path: &Path,
    display_path: &Path,
    operation: &'static str,
) -> OciArtifactObservationState {
    match base.symlink_metadata(relative_path) {
        Ok(metadata) if metadata.file_type().is_file() => OciArtifactObservationState::Present,
        Ok(_) => OciArtifactObservationState::Unknown(OciEvidenceUnknown::unexpected_type(
            operation,
            display_path,
            "a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            OciArtifactObservationState::Absent
        }
        Err(error) => OciArtifactObservationState::Unknown(OciEvidenceUnknown::io(
            operation,
            display_path,
            error,
        )),
    }
}

fn observe_artifact_entry(
    kind: OciArtifactKind,
    entry: &DirEntry,
    path: PathBuf,
    operation: &'static str,
) -> OciArtifactObservation {
    let state = match entry.file_type() {
        Ok(file_type) if file_type.is_file() => OciArtifactObservationState::Present,
        Ok(_) => OciArtifactObservationState::Unknown(OciEvidenceUnknown::unexpected_type(
            operation,
            &path,
            "a regular file",
        )),
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            OciArtifactObservationState::Absent
        }
        Err(error) => {
            OciArtifactObservationState::Unknown(OciEvidenceUnknown::io(operation, &path, error))
        }
    };
    OciArtifactObservation { kind, path, state }
}

fn candidate_key(tenant_id: &TenantId, attachment_id: &NetworkAttachmentId) -> String {
    format!("{}\0{}", tenant_id.as_str(), attachment_id.as_str())
}

fn attachment_state_error(error: impl std::fmt::Display) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("OCI attachment evidence authority failed: {error}"),
    }
}

fn corrupt_evidence(reason: impl Into<String>) -> SandboxError {
    SandboxError::OperationFailed {
        message: format!("OCI orphan evidence is corrupt: {}", reason.into()),
    }
}

#[cfg(test)]
pub(in crate::backends::oci::network) mod test_support;
#[cfg(test)]
mod tests;
