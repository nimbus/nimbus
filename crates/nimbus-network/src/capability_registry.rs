//! Exact, transport-free composition of source-owned capability reports.
//!
//! A registry contains only explicitly admitted attachment/ingress bundles.
//! Knowing two provider IDs never makes their pair compatible, and a
//! satisfying alternative is diagnostic evidence rather than fallback
//! authority.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error as StdError;
use std::fmt::{self, Display, Formatter};

use serde::{Deserialize, Serialize};

use crate::{
    NetworkAddressFamily, NetworkAttachmentCapabilitySet, NetworkCapabilityMismatch,
    NetworkCapabilityRequirements, NetworkEndpointCapabilitySet, NetworkForwardingCapabilitySet,
    NetworkIngressCapabilitySet, NetworkLifecycleCapabilitySet, NetworkProviderId,
    NetworkSovereigntyCapabilities, NetworkSovereigntyRequirements,
};

/// Capability role owned by one provider registration.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NetworkCapabilityRole {
    /// Workload-to-network attachment realization.
    Attachment,
    /// Reachable listener and request-ingress realization.
    Ingress,
}

impl Display for NetworkCapabilityRole {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(match self {
            Self::Attachment => "attachment",
            Self::Ingress => "ingress",
        })
    }
}

/// Source-owned capability evidence for one attachment implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkAttachmentProviderRegistration {
    provider_id: NetworkProviderId,
    attachment: NetworkAttachmentCapabilitySet,
    address_families: BTreeSet<NetworkAddressFamily>,
    lifecycle: NetworkLifecycleCapabilitySet,
    sovereignty: NetworkSovereigntyCapabilities,
}

impl NetworkAttachmentProviderRegistration {
    /// Construct a complete attachment-role registration.
    pub fn new(
        provider_id: NetworkProviderId,
        attachment: NetworkAttachmentCapabilitySet,
        address_families: impl IntoIterator<Item = NetworkAddressFamily>,
        lifecycle: NetworkLifecycleCapabilitySet,
        sovereignty: NetworkSovereigntyCapabilities,
    ) -> Self {
        Self {
            provider_id,
            attachment,
            address_families: address_families.into_iter().collect(),
            lifecycle,
            sovereignty,
        }
    }

    /// Stable provider registration identity.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    /// Attachment ownership, shape, and isolation facts.
    pub fn attachment(&self) -> &NetworkAttachmentCapabilitySet {
        &self.attachment
    }

    /// Address families the realized attachment can carry.
    pub fn address_families(&self) -> &BTreeSet<NetworkAddressFamily> {
        &self.address_families
    }

    /// Durable lifecycle operations this attachment owner can prove.
    pub fn lifecycle(&self) -> &NetworkLifecycleCapabilitySet {
        &self.lifecycle
    }

    /// Sovereignty evidence for this attachment owner.
    pub fn sovereignty(&self) -> &NetworkSovereigntyCapabilities {
        &self.sovereignty
    }
}

/// Source-owned capability evidence for one ingress implementation.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkIngressProviderRegistration {
    provider_id: NetworkProviderId,
    endpoint: NetworkEndpointCapabilitySet,
    ingress: NetworkIngressCapabilitySet,
    forwarding: NetworkForwardingCapabilitySet,
    lifecycle: NetworkLifecycleCapabilitySet,
    sovereignty: NetworkSovereigntyCapabilities,
}

impl NetworkIngressProviderRegistration {
    /// Construct a complete ingress-role registration.
    pub fn new(
        provider_id: NetworkProviderId,
        endpoint: NetworkEndpointCapabilitySet,
        ingress: NetworkIngressCapabilitySet,
        forwarding: NetworkForwardingCapabilitySet,
        lifecycle: NetworkLifecycleCapabilitySet,
        sovereignty: NetworkSovereigntyCapabilities,
    ) -> Self {
        Self {
            provider_id,
            endpoint,
            ingress,
            forwarding,
            lifecycle,
            sovereignty,
        }
    }

    /// Stable provider registration identity.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    /// Endpoint transport and exposure facts.
    pub fn endpoint(&self) -> &NetworkEndpointCapabilitySet {
        &self.endpoint
    }

    /// Request-ingress behavior.
    pub fn ingress(&self) -> &NetworkIngressCapabilitySet {
        &self.ingress
    }

    /// Forwarding behavior owned by this ingress implementation.
    pub fn forwarding(&self) -> &NetworkForwardingCapabilitySet {
        &self.forwarding
    }

    /// Durable lifecycle operations this ingress owner can prove.
    pub fn lifecycle(&self) -> &NetworkLifecycleCapabilitySet {
        &self.lifecycle
    }

    /// Sovereignty evidence for this ingress owner.
    pub fn sovereignty(&self) -> &NetworkSovereigntyCapabilities {
        &self.sovereignty
    }
}

/// Exact provider pair requested by an upper composition owner.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkCapabilitySelection {
    attachment_provider_id: NetworkProviderId,
    ingress_provider_id: NetworkProviderId,
}

impl NetworkCapabilitySelection {
    /// Name both required provider roles explicitly.
    pub fn new(
        attachment_provider_id: NetworkProviderId,
        ingress_provider_id: NetworkProviderId,
    ) -> Self {
        Self {
            attachment_provider_id,
            ingress_provider_id,
        }
    }

    /// Selected attachment registration identity.
    pub fn attachment_provider_id(&self) -> &NetworkProviderId {
        &self.attachment_provider_id
    }

    /// Selected ingress registration identity.
    pub fn ingress_provider_id(&self) -> &NetworkProviderId {
        &self.ingress_provider_id
    }
}

impl Display for NetworkCapabilitySelection {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "attachment={}, ingress={}",
            self.attachment_provider_id, self.ingress_provider_id
        )
    }
}

/// One explicitly admitted compatible provider composition.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkCapabilityBundle {
    attachment: NetworkAttachmentProviderRegistration,
    ingress: NetworkIngressProviderRegistration,
}

impl NetworkCapabilityBundle {
    /// Construct one complete attachment/ingress bundle.
    ///
    /// Compatibility is not inferred by this constructor. Supplying the value
    /// to [`NetworkCapabilityRegistry::new`] is the caller's explicit
    /// admission of this exact pair.
    pub fn new(
        attachment: NetworkAttachmentProviderRegistration,
        ingress: NetworkIngressProviderRegistration,
    ) -> Self {
        Self {
            attachment,
            ingress,
        }
    }

    /// Stable exact selection represented by this bundle.
    pub fn selection(&self) -> NetworkCapabilitySelection {
        NetworkCapabilitySelection::new(
            self.attachment.provider_id.clone(),
            self.ingress.provider_id.clone(),
        )
    }

    /// Attachment-role registration.
    pub fn attachment(&self) -> &NetworkAttachmentProviderRegistration {
        &self.attachment
    }

    /// Ingress-role registration.
    pub fn ingress(&self) -> &NetworkIngressProviderRegistration {
        &self.ingress
    }
}

/// Stable registry-construction failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkCapabilityRegistryError {
    /// One stable provider identity was used for different roles.
    ProviderRoleConflict { provider_id: NetworkProviderId },
    /// One role/provider identity was associated with divergent evidence.
    ProviderReportConflict {
        role: NetworkCapabilityRole,
        provider_id: NetworkProviderId,
    },
}

impl Display for NetworkCapabilityRegistryError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::ProviderRoleConflict { provider_id } => write!(
                formatter,
                "network capability provider `{provider_id}` is registered for both attachment \
                 and ingress roles"
            ),
            Self::ProviderReportConflict { role, provider_id } => write!(
                formatter,
                "network capability {role} provider `{provider_id}` has divergent reports"
            ),
        }
    }
}

impl StdError for NetworkCapabilityRegistryError {}

/// Mismatches attributed to one exact role/provider registration.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(deny_unknown_fields)]
pub struct NetworkCapabilityProviderFailure {
    role: NetworkCapabilityRole,
    provider_id: NetworkProviderId,
    mismatches: Vec<NetworkCapabilityMismatch>,
}

impl NetworkCapabilityProviderFailure {
    fn new(
        role: NetworkCapabilityRole,
        provider_id: NetworkProviderId,
        mismatches: Vec<NetworkCapabilityMismatch>,
    ) -> Self {
        Self {
            role,
            provider_id,
            mismatches,
        }
    }

    /// Capability role that failed.
    pub const fn role(&self) -> NetworkCapabilityRole {
        self.role
    }

    /// Exact provider whose report failed.
    pub fn provider_id(&self) -> &NetworkProviderId {
        &self.provider_id
    }

    /// Mismatches in stable role-scoped dimension order.
    pub fn mismatches(&self) -> &[NetworkCapabilityMismatch] {
        &self.mismatches
    }
}

/// Deterministic exact-selection failure.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NetworkCapabilitySelectionError {
    /// The exact pair was not admitted into this registry snapshot.
    UnregisteredComposition {
        requested: NetworkCapabilitySelection,
        missing_roles: Vec<NetworkCapabilityRole>,
        registered_compositions: Vec<NetworkCapabilitySelection>,
    },
    /// The exact admitted pair lacks required evidence.
    Unsatisfied {
        requested: NetworkCapabilitySelection,
        failures: Vec<NetworkCapabilityProviderFailure>,
        safe_alternatives: Vec<NetworkCapabilitySelection>,
    },
}

impl NetworkCapabilitySelectionError {
    /// Exact selection requested by the caller.
    pub fn requested_selection(&self) -> &NetworkCapabilitySelection {
        match self {
            Self::UnregisteredComposition { requested, .. }
            | Self::Unsatisfied { requested, .. } => requested,
        }
    }

    /// Whether the exact pair was absent rather than capability-incomplete.
    pub const fn is_unregistered_composition(&self) -> bool {
        matches!(self, Self::UnregisteredComposition { .. })
    }

    /// Roles whose individual provider IDs are unknown.
    ///
    /// An empty result on an unregistered composition means both providers
    /// are known but their pair was never admitted.
    pub fn missing_roles(&self) -> &[NetworkCapabilityRole] {
        match self {
            Self::UnregisteredComposition { missing_roles, .. } => missing_roles,
            Self::Unsatisfied { .. } => &[],
        }
    }

    /// Complete registered compositions in stable identity order.
    pub fn registered_compositions(&self) -> &[NetworkCapabilitySelection] {
        match self {
            Self::UnregisteredComposition {
                registered_compositions,
                ..
            } => registered_compositions,
            Self::Unsatisfied { .. } => &[],
        }
    }

    /// Role-attributed capability failures.
    pub fn provider_failures(&self) -> &[NetworkCapabilityProviderFailure] {
        match self {
            Self::Unsatisfied { failures, .. } => failures,
            Self::UnregisteredComposition { .. } => &[],
        }
    }

    /// Other complete registered compositions proven to satisfy.
    ///
    /// These are diagnostic only. The registry never selects one.
    pub fn safe_alternatives(&self) -> &[NetworkCapabilitySelection] {
        match self {
            Self::Unsatisfied {
                safe_alternatives, ..
            } => safe_alternatives,
            Self::UnregisteredComposition { .. } => &[],
        }
    }
}

impl Display for NetworkCapabilitySelectionError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::UnregisteredComposition {
                requested,
                missing_roles,
                registered_compositions,
            } => {
                write!(
                    formatter,
                    "network capability composition ({requested}) is not registered"
                )?;
                formatter.write_str("; missing provider roles: ")?;
                format_roles(formatter, missing_roles)?;
                formatter.write_str("; registered compositions: ")?;
                format_selections(formatter, registered_compositions)
            }
            Self::Unsatisfied {
                requested,
                failures,
                safe_alternatives,
            } => {
                write!(
                    formatter,
                    "network capability composition ({requested}) does not satisfy requirements: "
                )?;
                for (failure_index, failure) in failures.iter().enumerate() {
                    if failure_index != 0 {
                        formatter.write_str("; ")?;
                    }
                    write!(
                        formatter,
                        "{} provider `{}`: ",
                        failure.role, failure.provider_id
                    )?;
                    for (mismatch_index, mismatch) in failure.mismatches.iter().enumerate() {
                        if mismatch_index != 0 {
                            formatter.write_str(", ")?;
                        }
                        write!(formatter, "{mismatch}")?;
                    }
                }
                formatter.write_str("; safe alternatives: ")?;
                format_selections(formatter, safe_alternatives)
            }
        }
    }
}

impl StdError for NetworkCapabilitySelectionError {}

fn format_roles(formatter: &mut Formatter<'_>, roles: &[NetworkCapabilityRole]) -> fmt::Result {
    if roles.is_empty() {
        return formatter.write_str("none");
    }
    for (index, role) in roles.iter().enumerate() {
        if index != 0 {
            formatter.write_str(", ")?;
        }
        write!(formatter, "{role}")?;
    }
    Ok(())
}

fn format_selections(
    formatter: &mut Formatter<'_>,
    selections: &[NetworkCapabilitySelection],
) -> fmt::Result {
    if selections.is_empty() {
        return formatter.write_str("none");
    }
    for (index, selection) in selections.iter().enumerate() {
        if index != 0 {
            formatter.write_str("; ")?;
        }
        write!(formatter, "({selection})")?;
    }
    Ok(())
}

/// Immutable registry of explicitly compatible capability compositions.
#[derive(Debug, Clone)]
pub struct NetworkCapabilityRegistry {
    bundles: BTreeMap<NetworkCapabilitySelection, NetworkCapabilityBundle>,
    attachment_reports: BTreeMap<NetworkProviderId, NetworkAttachmentProviderRegistration>,
    ingress_reports: BTreeMap<NetworkProviderId, NetworkIngressProviderRegistration>,
}

impl NetworkCapabilityRegistry {
    /// Atomically validate and register complete bundles.
    ///
    /// There is deliberately no empty-role mutation API: a registry snapshot
    /// cannot expose partially registered compositions.
    pub fn new(
        bundles: impl IntoIterator<Item = NetworkCapabilityBundle>,
    ) -> Result<Self, NetworkCapabilityRegistryError> {
        let mut bundles: Vec<_> = bundles.into_iter().collect();
        let attachment_provider_ids: BTreeSet<_> = bundles
            .iter()
            .map(|bundle| bundle.attachment.provider_id.clone())
            .collect();
        let ingress_provider_ids: BTreeSet<_> = bundles
            .iter()
            .map(|bundle| bundle.ingress.provider_id.clone())
            .collect();
        if let Some(provider_id) = attachment_provider_ids
            .intersection(&ingress_provider_ids)
            .next()
        {
            return Err(NetworkCapabilityRegistryError::ProviderRoleConflict {
                provider_id: provider_id.clone(),
            });
        }
        bundles.sort_by_key(NetworkCapabilityBundle::selection);

        let mut registry = Self {
            bundles: BTreeMap::new(),
            attachment_reports: BTreeMap::new(),
            ingress_reports: BTreeMap::new(),
        };
        for bundle in bundles {
            registry.insert_bundle(bundle)?;
        }
        Ok(registry)
    }

    fn insert_bundle(
        &mut self,
        bundle: NetworkCapabilityBundle,
    ) -> Result<(), NetworkCapabilityRegistryError> {
        let attachment_id = bundle.attachment.provider_id.clone();
        let ingress_id = bundle.ingress.provider_id.clone();
        if attachment_id == ingress_id
            || self.ingress_reports.contains_key(&attachment_id)
            || self.attachment_reports.contains_key(&ingress_id)
        {
            let provider_id = if attachment_id == ingress_id
                || self.ingress_reports.contains_key(&attachment_id)
            {
                attachment_id
            } else {
                ingress_id
            };
            return Err(NetworkCapabilityRegistryError::ProviderRoleConflict { provider_id });
        }

        insert_report(
            &mut self.attachment_reports,
            attachment_id,
            bundle.attachment.clone(),
            NetworkCapabilityRole::Attachment,
        )?;
        insert_report(
            &mut self.ingress_reports,
            ingress_id,
            bundle.ingress.clone(),
            NetworkCapabilityRole::Ingress,
        )?;

        let selection = bundle.selection();
        if let Some(existing) = self.bundles.get(&selection) {
            debug_assert_eq!(
                existing, &bundle,
                "role report uniqueness makes an equal selection unique"
            );
            return Ok(());
        }
        self.bundles.insert(selection, bundle);
        Ok(())
    }

    /// Select one exact admitted composition and prove it satisfies.
    ///
    /// Safe alternatives are computed only after the requested composition
    /// fails. They are returned as diagnostics and are never selected.
    pub fn select_exact(
        &self,
        requested: &NetworkCapabilitySelection,
        requirements: &NetworkCapabilityRequirements,
    ) -> Result<&NetworkCapabilityBundle, NetworkCapabilitySelectionError> {
        let Some(bundle) = self.bundles.get(requested) else {
            let mut missing_roles = Vec::new();
            if !self
                .attachment_reports
                .contains_key(requested.attachment_provider_id())
            {
                missing_roles.push(NetworkCapabilityRole::Attachment);
            }
            if !self
                .ingress_reports
                .contains_key(requested.ingress_provider_id())
            {
                missing_roles.push(NetworkCapabilityRole::Ingress);
            }
            return Err(NetworkCapabilitySelectionError::UnregisteredComposition {
                requested: requested.clone(),
                missing_roles,
                registered_compositions: self.bundles.keys().cloned().collect(),
            });
        };

        let failures = evaluate_bundle(bundle, requirements);
        if failures.is_empty() {
            return Ok(bundle);
        }
        let safe_alternatives = self
            .bundles
            .iter()
            .filter(|(selection, candidate)| {
                *selection != requested && evaluate_bundle(candidate, requirements).is_empty()
            })
            .map(|(selection, _)| selection.clone())
            .collect();
        Err(NetworkCapabilitySelectionError::Unsatisfied {
            requested: requested.clone(),
            failures,
            safe_alternatives,
        })
    }

    /// Complete admitted selections in stable identity order.
    pub fn selections(&self) -> impl ExactSizeIterator<Item = &NetworkCapabilitySelection> {
        self.bundles.keys()
    }
}

fn insert_report<V>(
    reports: &mut BTreeMap<NetworkProviderId, V>,
    provider_id: NetworkProviderId,
    report: V,
    role: NetworkCapabilityRole,
) -> Result<(), NetworkCapabilityRegistryError>
where
    V: PartialEq,
{
    if let Some(existing) = reports.get(&provider_id) {
        if existing == &report {
            return Ok(());
        }
        return Err(NetworkCapabilityRegistryError::ProviderReportConflict { role, provider_id });
    }
    reports.insert(provider_id, report);
    Ok(())
}

fn evaluate_bundle(
    bundle: &NetworkCapabilityBundle,
    requirements: &NetworkCapabilityRequirements,
) -> Vec<NetworkCapabilityProviderFailure> {
    let attachment_mismatches = attachment_mismatches(bundle.attachment(), requirements);
    let ingress_mismatches = ingress_mismatches(bundle.ingress(), requirements);
    let mut failures = Vec::new();
    if !attachment_mismatches.is_empty() {
        failures.push(NetworkCapabilityProviderFailure::new(
            NetworkCapabilityRole::Attachment,
            bundle.attachment.provider_id.clone(),
            attachment_mismatches,
        ));
    }
    if !ingress_mismatches.is_empty() {
        failures.push(NetworkCapabilityProviderFailure::new(
            NetworkCapabilityRole::Ingress,
            bundle.ingress.provider_id.clone(),
            ingress_mismatches,
        ));
    }
    failures
}

fn attachment_mismatches(
    offered: &NetworkAttachmentProviderRegistration,
    requirements: &NetworkCapabilityRequirements,
) -> Vec<NetworkCapabilityMismatch> {
    let mut mismatches = Vec::new();
    if offered.attachment.management_mode() != requirements.attachment().management_mode() {
        mismatches.push(NetworkCapabilityMismatch::ManagementMode {
            required: requirements.attachment().management_mode(),
            offered: offered.attachment.management_mode(),
        });
    }
    for required in requirements
        .attachment()
        .attachment_modes()
        .difference(offered.attachment.attachment_modes())
    {
        mismatches.push(NetworkCapabilityMismatch::AttachmentMode {
            required: *required,
        });
    }
    for required in requirements
        .attachment()
        .isolation_modes()
        .difference(offered.attachment.isolation_modes())
    {
        mismatches.push(NetworkCapabilityMismatch::IsolationMode {
            required: *required,
        });
    }
    for required in requirements
        .endpoint()
        .address_families()
        .difference(&offered.address_families)
    {
        mismatches.push(NetworkCapabilityMismatch::AddressFamily {
            required: *required,
        });
    }
    lifecycle_mismatches(
        &mut mismatches,
        &offered.lifecycle,
        requirements.lifecycle(),
    );
    sovereignty_mismatches(
        &mut mismatches,
        &offered.sovereignty,
        requirements.sovereignty(),
    );
    mismatches
}

fn ingress_mismatches(
    offered: &NetworkIngressProviderRegistration,
    requirements: &NetworkCapabilityRequirements,
) -> Vec<NetworkCapabilityMismatch> {
    let mut mismatches = Vec::new();
    for required in requirements
        .endpoint()
        .address_families()
        .difference(offered.endpoint.address_families())
    {
        mismatches.push(NetworkCapabilityMismatch::AddressFamily {
            required: *required,
        });
    }
    for required in requirements
        .endpoint()
        .bind_realms()
        .difference(offered.endpoint.bind_realms())
    {
        mismatches.push(NetworkCapabilityMismatch::BindRealm {
            required: *required,
        });
    }
    for required in requirements
        .endpoint()
        .exposures()
        .difference(offered.endpoint.exposures())
    {
        mismatches.push(NetworkCapabilityMismatch::Exposure {
            required: *required,
        });
    }
    for required in requirements
        .endpoint()
        .protocols()
        .difference(offered.endpoint.protocols())
    {
        mismatches.push(NetworkCapabilityMismatch::Protocol {
            required: *required,
        });
    }
    for required in requirements
        .endpoint()
        .port_assignment_modes()
        .difference(offered.endpoint.port_assignment_modes())
    {
        mismatches.push(NetworkCapabilityMismatch::PortAssignment {
            required: *required,
        });
    }
    for required in requirements
        .ingress()
        .features()
        .difference(offered.ingress.features())
    {
        mismatches.push(NetworkCapabilityMismatch::IngressFeature {
            required: *required,
        });
    }
    for required in requirements
        .forwarding()
        .features()
        .difference(offered.forwarding.features())
    {
        mismatches.push(NetworkCapabilityMismatch::ForwardingFeature {
            required: *required,
        });
    }
    lifecycle_mismatches(
        &mut mismatches,
        &offered.lifecycle,
        requirements.lifecycle(),
    );
    sovereignty_mismatches(
        &mut mismatches,
        &offered.sovereignty,
        requirements.sovereignty(),
    );
    mismatches
}

fn lifecycle_mismatches(
    mismatches: &mut Vec<NetworkCapabilityMismatch>,
    offered: &NetworkLifecycleCapabilitySet,
    required: &NetworkLifecycleCapabilitySet,
) {
    for required in required.features().difference(offered.features()) {
        mismatches.push(NetworkCapabilityMismatch::LifecycleFeature {
            required: *required,
        });
    }
}

fn sovereignty_mismatches(
    mismatches: &mut Vec<NetworkCapabilityMismatch>,
    offered: &NetworkSovereigntyCapabilities,
    required: &NetworkSovereigntyRequirements,
) {
    if offered.control_plane_locality() > required.maximum_control_plane_locality() {
        mismatches.push(NetworkCapabilityMismatch::ControlPlaneLocality {
            maximum_allowed: required.maximum_control_plane_locality(),
            offered: offered.control_plane_locality(),
        });
    }
    for dependency in offered
        .required_external_dependencies()
        .difference(required.allowed_external_dependencies())
    {
        mismatches.push(NetworkCapabilityMismatch::ExternalDependency {
            disallowed: *dependency,
        });
    }
    if required.offline_restart_required() && !offered.offline_restart_supported() {
        mismatches.push(NetworkCapabilityMismatch::OfflineRestart {
            required: true,
            offered: false,
        });
    }
}

#[cfg(test)]
mod tests;
