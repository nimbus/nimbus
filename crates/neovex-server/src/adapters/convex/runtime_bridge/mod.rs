use super::dispatch::{
    check_host_cancellation, dispatch_convex_mutation_cancellable, dispatch_mutation,
    encode_runtime_core_result, ensure_runtime_host_not_cancelled,
    execute_convex_action_cancellable, execute_query_result_cancellable, execute_schedule_command,
    runtime_error_to_core,
};
use super::http_actions::prepare_http_action_response_cancellable;
use super::registry::validate_runtime_http_route;
use super::subscriptions::{
    is_scalar_filter_value, should_replace_lower_bound, should_replace_upper_bound,
};
use super::*;

mod async_bridge;
mod db_ops;
mod function_ops;
mod read_tracking;

impl ConvexRuntimeResponseEnvelope {
    pub(in crate::adapters::convex) fn ok(value: Value) -> Self {
        Self::Ok { value }
    }

    pub(in crate::adapters::convex) fn from_core_error(error: Error) -> Self {
        Self::Error {
            error: ConvexRuntimeEncodedError::from_core_error(error),
        }
    }

    pub(in crate::adapters::convex) fn into_core_result(self) -> Result<Value, Error> {
        match self {
            Self::Ok { value } => Ok(value),
            Self::Error { error } => Err(error.into_core_error()),
        }
    }
}

impl ConvexRuntimeEncodedError {
    pub(in crate::adapters::convex) fn from_core_error(error: Error) -> Self {
        match error {
            Error::Cancelled => Self::Cancelled,
            Error::TenantNotFound(tenant_id) => Self::TenantNotFound {
                tenant_id: tenant_id.to_string(),
            },
            Error::DocumentNotFound(document_id) => Self::DocumentNotFound {
                document_id: document_id.to_string(),
            },
            Error::ScheduledJobNotFound(job_id) => Self::ScheduledJobNotFound {
                job_id: job_id.to_string(),
            },
            Error::AlreadyExists(message) => Self::AlreadyExists { message },
            Error::InvalidInput(message) => Self::InvalidInput { message },
            Error::SchemaValidation(message) => Self::SchemaValidation { message },
            Error::SchemaNotFound(table) => Self::SchemaNotFound {
                table: table.to_string(),
            },
            Error::Storage(message) => Self::Storage { message },
            Error::Serialization(message) => Self::Serialization { message },
            Error::Internal(message) => Self::Internal { message },
        }
    }

    pub(in crate::adapters::convex) fn into_core_error(self) -> Error {
        match self {
            Self::Cancelled => Error::Cancelled,
            Self::TenantNotFound { tenant_id } => TenantId::new(tenant_id)
                .map(Error::TenantNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::DocumentNotFound { document_id } => document_id
                .parse()
                .map(Error::DocumentNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::ScheduledJobNotFound { job_id } => job_id
                .parse()
                .map(Error::ScheduledJobNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::AlreadyExists { message } => Error::AlreadyExists(message),
            Self::InvalidInput { message } => Error::InvalidInput(message),
            Self::SchemaValidation { message } => Error::SchemaValidation(message),
            Self::SchemaNotFound { table } => TableName::new(table)
                .map(Error::SchemaNotFound)
                .unwrap_or_else(|error| Error::Internal(error.to_string())),
            Self::Storage { message } => Error::Storage(message),
            Self::Serialization { message } => Error::Serialization(message),
            Self::Internal { message } => Error::Internal(message),
        }
    }
}

impl ConvexRuntimeBridge {
    pub(in crate::adapters::convex) fn new(
        service: Arc<neovex_engine::Service>,
        registry: Arc<ConvexRegistry>,
        tenant_id: TenantId,
        server_request_id: Option<String>,
    ) -> Self {
        static NEXT_RUNTIME_SESSION_ID: AtomicU64 = AtomicU64::new(1);
        let max_nested_runtime_invocations = registry
            .runtime_policy()
            .limits()
            .max_nested_runtime_invocations;
        Self {
            service,
            registry,
            tenant_id,
            server_request_id,
            session_id: format!(
                "convex-runtime-session-{}",
                NEXT_RUNTIME_SESSION_ID.fetch_add(1, Ordering::Relaxed)
            ),
            max_nested_runtime_invocations,
            remaining_nested_runtime_invocations: Arc::new(AtomicUsize::new(
                max_nested_runtime_invocations,
            )),
            query_builders: Arc::new(Mutex::new(ConvexRuntimeQueryBuilders::default())),
            read_set: Arc::new(Mutex::new(RuntimeReadSet::default())),
        }
    }

    pub(in crate::adapters::convex) fn snapshot_read_set(&self) -> RuntimeReadSet {
        self.read_set
            .lock()
            .expect("convex runtime read set lock should not be poisoned")
            .clone()
    }

    pub(in crate::adapters::convex) fn validate_session(
        &self,
        session_id: Option<&str>,
    ) -> std::result::Result<(), NeovexRuntimeError> {
        if let Some(session_id) = session_id
            && session_id.is_empty()
        {
            return Err(NeovexRuntimeError::Contract(format!(
                "runtime session token must not be empty for tenant {}",
                self.tenant_id
            )));
        }
        Ok(())
    }

    pub(in crate::adapters::convex) fn consume_nested_runtime_invocation_budget(
        &self,
    ) -> Result<(), Error> {
        let mut remaining = self
            .remaining_nested_runtime_invocations
            .load(Ordering::SeqCst);
        loop {
            if remaining == 0 {
                return Err(runtime_error_to_core(
                    NeovexRuntimeError::NestedInvocationLimitExceeded(
                        self.max_nested_runtime_invocations,
                    ),
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
}
