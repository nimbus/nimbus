use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use neovex_core::{Cursor, DocumentId, Filter, Query, TableName, TenantId};
use neovex_runtime::NeovexRuntimeError;

use crate::runtime::read_tracking::{RuntimeIndexRangeRead, RuntimeReadSet};

#[derive(Clone)]
pub(crate) struct RuntimeHostState {
    server_request_id: Option<String>,
    session_id: String,
    max_nested_runtime_invocations: usize,
    remaining_nested_runtime_invocations: Arc<AtomicUsize>,
    read_set: Arc<Mutex<RuntimeReadSet>>,
}

impl RuntimeHostState {
    pub(crate) fn new(
        session_prefix: &str,
        server_request_id: Option<String>,
        max_nested_runtime_invocations: usize,
    ) -> Self {
        static NEXT_RUNTIME_SESSION_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            server_request_id,
            session_id: format!(
                "{session_prefix}-{}",
                NEXT_RUNTIME_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            ),
            max_nested_runtime_invocations,
            remaining_nested_runtime_invocations: Arc::new(AtomicUsize::new(
                max_nested_runtime_invocations,
            )),
            read_set: Arc::new(Mutex::new(RuntimeReadSet::default())),
        }
    }

    pub(crate) fn server_request_id(&self) -> Option<&str> {
        self.server_request_id.as_deref()
    }

    pub(crate) fn session_id(&self) -> &str {
        &self.session_id
    }

    pub(crate) fn snapshot_read_set(&self) -> RuntimeReadSet {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .clone()
    }

    pub(crate) fn validate_session(
        &self,
        tenant_id: &TenantId,
        session_id: Option<&str>,
    ) -> std::result::Result<(), NeovexRuntimeError> {
        if let Some(session_id) = session_id
            && session_id.is_empty()
        {
            return Err(NeovexRuntimeError::Contract(format!(
                "runtime session token must not be empty for tenant {}",
                tenant_id
            )));
        }
        Ok(())
    }

    pub(crate) fn consume_nested_runtime_invocation_budget(
        &self,
    ) -> std::result::Result<(), NeovexRuntimeError> {
        let mut remaining = self
            .remaining_nested_runtime_invocations
            .load(Ordering::SeqCst);
        loop {
            if remaining == 0 {
                return Err(NeovexRuntimeError::NestedInvocationLimitExceeded(
                    self.max_nested_runtime_invocations,
                ));
            }
            match self.remaining_nested_runtime_invocations.compare_exchange(
                remaining,
                remaining - 1,
                Ordering::SeqCst,
                Ordering::SeqCst,
            ) {
                Ok(_) => return Ok(()),
                Err(next_remaining) => remaining = next_remaining,
            }
        }
    }

    pub(crate) fn record_table_read(&self, table: &TableName) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_table(table);
    }

    pub(crate) fn record_document_read(&self, table: &TableName, document_id: &DocumentId) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_document(table, document_id);
    }

    pub(crate) fn record_paginated_window_read(
        &self,
        query: &Query,
        page_size: usize,
        after: Option<&Cursor>,
        page: &neovex_core::Page,
    ) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_paginated_window(query, page_size, after, page);
    }

    pub(crate) fn record_index_read(&self, read: RuntimeIndexRangeRead) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_index_range(read);
    }

    pub(crate) fn record_predicate_read(&self, table: &TableName, filters: &[Filter]) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_predicate(table, filters);
    }
}
