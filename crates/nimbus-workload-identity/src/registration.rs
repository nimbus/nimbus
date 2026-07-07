use nimbus_tenant::WorkloadIdentity;
use serde::Serialize;

use crate::{IdentityTrustConfig, NodeIdentityRecord, TrustConfigError};

const WORKLOAD_SUBJECT_PREFIX: &str = "nimbus-workload:";
const WORKLOAD_SUBJECT_V1_PREFIX: &str = "nimbus-workload:v1/";

const SUBJECT_SELECTOR_DIMENSIONS: [SubjectSelectorDimension; 8] = [
    SubjectSelectorDimension {
        segment: "tenant",
        selector_key: "nimbus:tenant",
    },
    SubjectSelectorDimension {
        segment: "deployment",
        selector_key: "nimbus:deployment",
    },
    SubjectSelectorDimension {
        segment: "surface",
        selector_key: "nimbus:surface",
    },
    SubjectSelectorDimension {
        segment: "kind",
        selector_key: "nimbus:kind",
    },
    SubjectSelectorDimension {
        segment: "name",
        selector_key: "nimbus:name",
    },
    SubjectSelectorDimension {
        segment: "runtime-tier",
        selector_key: "nimbus:runtime-tier",
    },
    SubjectSelectorDimension {
        segment: "runtime-backend",
        selector_key: "nimbus:runtime-backend",
    },
    SubjectSelectorDimension {
        segment: "sandbox-backend",
        selector_key: "nimbus:sandbox-backend",
    },
];

struct SubjectSelectorDimension {
    segment: &'static str,
    selector_key: &'static str,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpiffeRegistrationEntry {
    spiffe_id: String,
    parent_id: String,
    selectors: Vec<SpiffeSelector>,
}

impl SpiffeRegistrationEntry {
    pub fn for_workload(
        trust: &IdentityTrustConfig,
        identity: &WorkloadIdentity,
        node: &NodeIdentityRecord,
    ) -> Result<Self, TrustConfigError> {
        trust.admit_source(node.source())?;

        let subject = identity.subject();
        let spiffe_id =
            workload_subject_to_spiffe_id(trust.trust_domain(), &subject).ok_or_else(|| {
                TrustConfigError::InvalidWorkloadSubject {
                    subject: subject.clone(),
                }
            })?;
        let mut selectors = selectors_for_subject(&subject).ok_or_else(|| {
            TrustConfigError::InvalidWorkloadSubject {
                subject: subject.clone(),
            }
        })?;

        if let Some(node_id) = identity.node_id() {
            selectors.push(SpiffeSelector::new("nimbus:node", node_id));
        }
        if let Some(machine_id) = identity.machine_id() {
            selectors.push(SpiffeSelector::new("nimbus:machine", machine_id));
        }
        if let Some(sandbox_id) = identity.sandbox_id() {
            selectors.push(SpiffeSelector::new("nimbus:sandbox", sandbox_id));
        }
        // invocation_id is per-invocation cardinality, so including it would
        // force registration churn and make selectors unsuitable for workload
        // identity admission. Keep it in audit evidence, not SPIFFE selectors.

        Ok(Self {
            spiffe_id,
            parent_id: format!(
                "spiffe://{}/nimbus/node/{}",
                trust.trust_domain(),
                node.id()
            ),
            selectors,
        })
    }

    pub fn spiffe_id(&self) -> &str {
        &self.spiffe_id
    }

    pub fn parent_id(&self) -> &str {
        &self.parent_id
    }

    pub fn selectors(&self) -> &[SpiffeSelector] {
        &self.selectors
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SpiffeSelector {
    key: &'static str,
    value: String,
}

impl SpiffeSelector {
    fn new(key: &'static str, value: impl Into<String>) -> Self {
        Self {
            key,
            value: value.into(),
        }
    }

    pub fn key(&self) -> &'static str {
        self.key
    }

    pub fn value(&self) -> &str {
        &self.value
    }
}

pub(crate) fn workload_subject_to_spiffe_id(trust_domain: &str, subject: &str) -> Option<String> {
    let suffix = subject.strip_prefix(WORKLOAD_SUBJECT_PREFIX)?;
    if !suffix.starts_with("v1/") {
        return None;
    }
    Some(format!("spiffe://{trust_domain}/nimbus/workload/{suffix}"))
}

fn selectors_for_subject(subject: &str) -> Option<Vec<SpiffeSelector>> {
    let suffix = subject.strip_prefix(WORKLOAD_SUBJECT_V1_PREFIX)?;
    let mut segments = suffix.split('/');
    let mut selectors = Vec::with_capacity(SUBJECT_SELECTOR_DIMENSIONS.len() + 3);
    for dimension in SUBJECT_SELECTOR_DIMENSIONS {
        if segments.next()? != dimension.segment {
            return None;
        }
        selectors.push(SpiffeSelector::new(
            dimension.selector_key,
            segments.next()?,
        ));
    }
    if segments.next().is_some() {
        return None;
    }
    Some(selectors)
}
