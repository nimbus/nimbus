use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

use nimbus_core::{Cursor, DocumentId, Filter, Query, TableId, TableName, TenantId};
use nimbus_runtime::NimbusRuntimeError;

use super::read_tracking::{RuntimeIndexRangeRead, RuntimeReadSet};

#[derive(Clone)]
pub struct RuntimeHostState {
    server_request_id: Option<String>,
    host_call_session_id: String,
    max_nested_runtime_invocations: usize,
    remaining_nested_runtime_invocations: Arc<AtomicUsize>,
    read_set: Arc<Mutex<RuntimeReadSet>>,
}

impl RuntimeHostState {
    pub fn new(
        host_call_session_prefix: &str,
        server_request_id: Option<String>,
        max_nested_runtime_invocations: usize,
    ) -> Self {
        static NEXT_HOST_CALL_SESSION_ID: AtomicU64 = AtomicU64::new(1);
        Self {
            server_request_id,
            host_call_session_id: format!(
                "{host_call_session_prefix}-{}",
                NEXT_HOST_CALL_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            ),
            max_nested_runtime_invocations,
            remaining_nested_runtime_invocations: Arc::new(AtomicUsize::new(
                max_nested_runtime_invocations,
            )),
            read_set: Arc::new(Mutex::new(RuntimeReadSet::default())),
        }
    }

    pub fn server_request_id(&self) -> Option<&str> {
        self.server_request_id.as_deref()
    }

    pub fn host_call_session_id(&self) -> &str {
        &self.host_call_session_id
    }

    pub fn snapshot_read_set(&self) -> RuntimeReadSet {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .clone()
    }

    pub fn validate_host_call_session(
        &self,
        tenant_id: &TenantId,
        host_call_session_id: Option<&str>,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        if let Some(host_call_session_id) = host_call_session_id
            && host_call_session_id.is_empty()
        {
            return Err(NimbusRuntimeError::Contract(format!(
                "runtime host-call token must not be empty for tenant {}",
                tenant_id
            )));
        }
        Ok(())
    }

    pub fn consume_nested_runtime_invocation_budget(
        &self,
    ) -> std::result::Result<(), NimbusRuntimeError> {
        let mut remaining = self
            .remaining_nested_runtime_invocations
            .load(Ordering::SeqCst);
        loop {
            if remaining == 0 {
                return Err(NimbusRuntimeError::NestedInvocationLimitExceeded(
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

    pub fn record_table_read(&self, table: &TableName, table_id: Option<&TableId>) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_table(table, table_id);
    }

    pub fn record_document_read(
        &self,
        table: &TableName,
        table_id: Option<&TableId>,
        document_id: &DocumentId,
    ) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_document(table, table_id, document_id);
    }

    pub fn record_paginated_window_read(
        &self,
        query: &Query,
        table_id: Option<&TableId>,
        page_size: usize,
        after: Option<&Cursor>,
        page: &nimbus_core::Page,
    ) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_paginated_window(query, table_id, page_size, after, page);
    }

    pub fn record_index_read(&self, read: RuntimeIndexRangeRead) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_index_range(read);
    }

    pub fn record_predicate_read(
        &self,
        table: &TableName,
        table_id: Option<&TableId>,
        filters: &[Filter],
    ) {
        self.read_set
            .lock()
            .expect("runtime host read set lock should not be poisoned")
            .record_predicate(table, table_id, filters);
    }
}
