use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

use crate::context::RuntimeInvocationContext;

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct RuntimeRequestCorrelationSnapshot {
    pub invocation_id: u64,
    pub server_request_id: String,
    pub tenant_label: Option<String>,
    pub function_name: String,
    pub kind: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct RuntimeRequestCorrelation {
    invocation_id: u64,
    server_request_id: String,
    tenant_label: Option<String>,
    function_name: String,
    kind: &'static str,
}

#[derive(Debug, Default)]
pub(super) struct RuntimeRequestCorrelationLog {
    entries: Mutex<VecDeque<RuntimeRequestCorrelation>>,
}

impl RuntimeRequestCorrelationLog {
    pub(super) fn record(&self, context: &RuntimeInvocationContext) {
        let Some(server_request_id) = context.server_request_id.clone() else {
            return;
        };
        const MAX_RECENT_REQUEST_CORRELATIONS: usize = 128;

        let mut entries = self
            .entries
            .lock()
            .expect("runtime request correlations lock should not be poisoned");
        if entries.len() == MAX_RECENT_REQUEST_CORRELATIONS {
            entries.pop_front();
        }
        entries.push_back(RuntimeRequestCorrelation {
            invocation_id: context.invocation_id,
            server_request_id,
            tenant_label: context.tenant_label.clone(),
            function_name: context.function_name.clone(),
            kind: context.kind,
        });
    }

    pub(super) fn snapshot(&self) -> Vec<RuntimeRequestCorrelationSnapshot> {
        self.entries
            .lock()
            .expect("runtime request correlations lock should not be poisoned")
            .iter()
            .map(|correlation| RuntimeRequestCorrelationSnapshot {
                invocation_id: correlation.invocation_id,
                server_request_id: correlation.server_request_id.clone(),
                tenant_label: correlation.tenant_label.clone(),
                function_name: correlation.function_name.clone(),
                kind: correlation.kind.to_string(),
            })
            .collect()
    }
}
