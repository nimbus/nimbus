//! Engine-backed Compose retirement.
//!
//! This module owns only command-level orchestration and exact durable output.
//! Workload lifecycle authority remains in compute, while the activated local
//! or forwarded profile retains every concrete provider effect.

use std::path::Path;
use std::sync::Arc;

use nimbus::{Engine, Error, TenantId};
use nimbus_compute::state::ComputeError;
use nimbus_compute::workload_saga::WorkloadTeardownCancellationToken;
use nimbus_compute::{ComputeResourceRetirementError, WorkloadTeardownDisposition};
use nimbus_server::EngineWorkloadSagaStore;
use nimbus_tenant::TenantIsolationContext;
use nimbus_workloads::WorkloadExecutionReference;

use crate::cli_ux;

use super::commands::ComposeDownCommand;
use super::discovery::ResolvedComposeSelection;
use super::execution::requested_service_names;
use super::provision::PreparedComposeProvision;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum ComposeServiceRetirementDisposition {
    Recorded,
    SourceFinalized,
}

impl ComposeServiceRetirementDisposition {
    const fn as_str(self) -> &'static str {
        match self {
            Self::Recorded => "recorded",
            Self::SourceFinalized => "source_finalized",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(super) struct ComposeServiceRetirementOutcome {
    tenant_id: TenantId,
    service_name: String,
    disposition: ComposeServiceRetirementDisposition,
    terminal_execution: Option<WorkloadExecutionReference>,
}

impl ComposeServiceRetirementOutcome {
    fn recorded(
        tenant_id: &TenantId,
        service_name: &str,
        terminal_execution: Option<&WorkloadExecutionReference>,
    ) -> Self {
        Self {
            tenant_id: tenant_id.clone(),
            service_name: service_name.to_owned(),
            disposition: ComposeServiceRetirementDisposition::Recorded,
            terminal_execution: terminal_execution.cloned(),
        }
    }

    fn source_finalized(tenant_id: &TenantId, service_name: &str) -> Self {
        Self {
            tenant_id: tenant_id.clone(),
            service_name: service_name.to_owned(),
            disposition: ComposeServiceRetirementDisposition::SourceFinalized,
            terminal_execution: None,
        }
    }

    #[cfg(test)]
    pub(super) const fn disposition(&self) -> ComposeServiceRetirementDisposition {
        self.disposition
    }

    #[cfg(test)]
    pub(super) fn service_name(&self) -> &str {
        &self.service_name
    }

    #[cfg(test)]
    pub(super) fn terminal_execution_reference(&self) -> Option<&WorkloadExecutionReference> {
        self.terminal_execution.as_ref()
    }
}

#[derive(Debug)]
pub(super) struct ComposeRetirementReport {
    project_name: String,
    tenant_id: TenantId,
    outcomes: Vec<ComposeServiceRetirementOutcome>,
}

#[derive(Debug)]
pub(super) enum ComposeRetirementError {
    Setup(Error),
    Service {
        project_name: String,
        tenant_id: TenantId,
        completed: Vec<ComposeServiceRetirementOutcome>,
        failed_service: String,
        source: Error,
    },
}

impl ComposeRetirementError {
    pub(super) fn into_nimbus_error(self) -> Error {
        match self {
            Self::Setup(error) => error,
            Self::Service {
                project_name,
                tenant_id,
                completed,
                failed_service,
                source,
            } => {
                let completed = completed
                    .iter()
                    .map(|outcome| {
                        format!("{}:{}", outcome.service_name, outcome.disposition.as_str())
                    })
                    .collect::<Vec<_>>()
                    .join(", ");
                let context = format!(
                    "Compose down for project {project_name} (tenant {tenant_id}) failed at service {failed_service}; completed=[{completed}]; unissued services retain their durable source authority"
                );
                compose_service_error_with_context(source, &context)
            }
        }
    }

    #[cfg(test)]
    pub(super) fn completed(&self) -> &[ComposeServiceRetirementOutcome] {
        match self {
            Self::Setup(_) => &[],
            Self::Service { completed, .. } => completed,
        }
    }

    #[cfg(test)]
    pub(super) fn failed_service(&self) -> Option<&str> {
        match self {
            Self::Setup(_) => None,
            Self::Service { failed_service, .. } => Some(failed_service),
        }
    }
}

impl ComposeRetirementReport {
    pub(super) fn render(&self) -> String {
        let header = format!(
            "Compose down completed for project {} (tenant {})",
            self.project_name, self.tenant_id
        );
        let detail_lines = self
            .outcomes
            .iter()
            .map(|outcome| {
                let execution = outcome
                    .terminal_execution
                    .as_ref()
                    .map_or("none", |reference| reference.execution_id().as_str());
                format!(
                    "{}: {} (terminal execution {})",
                    outcome.service_name,
                    outcome.disposition.as_str(),
                    execution
                )
            })
            .collect::<Vec<_>>();
        cli_ux::format_action_block(&header, &detail_lines)
    }

    #[cfg(test)]
    pub(super) fn outcomes(&self) -> &[ComposeServiceRetirementOutcome] {
        &self.outcomes
    }
}

pub(super) async fn retire_compose_services(
    command: &ComposeDownCommand,
    selection: &ResolvedComposeSelection,
    control_data_dir: &Path,
    prepared: PreparedComposeProvision,
    engine: Arc<Engine>,
) -> Result<ComposeRetirementReport, ComposeRetirementError> {
    let context = super::load_compose_project_context_for_selection(selection, control_data_dir)
        .map_err(ComposeRetirementError::Setup)?;
    let tenant_id = command
        .tenant
        .clone()
        .unwrap_or_else(|| context.control_plane.local_tenant_id.clone());
    let service_names = requested_service_names(&context, command.service.as_deref())
        .map_err(ComposeRetirementError::Setup)?;
    let saga_store = Arc::new(EngineWorkloadSagaStore::new(Arc::clone(&engine)));
    let runtime = prepared
        .activate(Arc::clone(&engine), Arc::clone(&saga_store))
        .await
        .map_err(ComposeRetirementError::Setup)?;
    let retirer = runtime
        .resource_retirer()
        .map_err(|error| ComposeRetirementError::Setup(compute_error(error)))?;
    let tenant_context = TenantIsolationContext::system(tenant_id.clone(), "compose-down");
    let cancellation = WorkloadTeardownCancellationToken::new();
    let mut outcomes = Vec::with_capacity(service_names.len());

    for service_name in service_names {
        let outcome = match retirer
            .submit_service_teardown_until_terminal(&tenant_context, &service_name, &cancellation)
            .await
        {
            Ok(outcome) => outcome,
            Err(error) => {
                return Err(ComposeRetirementError::Service {
                    project_name: context.control_plane.project_name,
                    tenant_id,
                    completed: outcomes,
                    failed_service: service_name,
                    source: retirement_error(error),
                });
            }
        };
        let outcome = match outcome.disposition() {
            WorkloadTeardownDisposition::Recorded => {
                let execution = outcome.terminal_execution_reference();
                ComposeServiceRetirementOutcome::recorded(&tenant_id, &service_name, execution)
            }
            WorkloadTeardownDisposition::SourceFinalized => {
                ComposeServiceRetirementOutcome::source_finalized(&tenant_id, &service_name)
            }
        };
        outcomes.push(outcome);
    }

    Ok(ComposeRetirementReport {
        project_name: context.control_plane.project_name,
        tenant_id,
        outcomes,
    })
}

#[cfg(test)]
#[path = "retirement/tests.rs"]
mod tests;

fn retirement_error(error: ComputeResourceRetirementError) -> Error {
    match error {
        ComputeResourceRetirementError::Source(source) => source,
        error => Error::Internal(error.to_string()),
    }
}

fn compute_error(error: ComputeError) -> Error {
    match error {
        ComputeError::Core(error) => error,
        ComputeError::Unauthorized(message) | ComputeError::Forbidden(message) => {
            Error::PermissionDenied(message)
        }
        ComputeError::NotFound(message) => Error::NotFound(message),
    }
}

fn compose_service_error_with_context(error: Error, context: &str) -> Error {
    let contextual = |message: String| format!("{context}: {message}");
    match error {
        Error::NotFound(message) => Error::NotFound(contextual(message)),
        Error::AlreadyExists(message) => Error::AlreadyExists(contextual(message)),
        Error::ResourceExhausted(message) => Error::ResourceExhausted(contextual(message)),
        Error::PermissionDenied(message) => Error::PermissionDenied(contextual(message)),
        Error::Conflict {
            message,
            conflicting_sequence,
            retryable,
            attempts,
        } => Error::Conflict {
            message: contextual(message),
            conflicting_sequence,
            retryable,
            attempts,
        },
        Error::Overloaded { message } => Error::Overloaded {
            message: contextual(message),
        },
        Error::CommitterFull { message, capacity } => Error::CommitterFull {
            message: contextual(message),
            capacity,
        },
        Error::RejectedBeforeExecution { message } => Error::RejectedBeforeExecution {
            message: contextual(message),
        },
        Error::RateLimited {
            message,
            retry_after,
        } => Error::RateLimited {
            message: contextual(message),
            retry_after,
        },
        Error::OutOfRetention {
            message,
            minimum_sequence,
        } => Error::OutOfRetention {
            message: contextual(message),
            minimum_sequence,
        },
        Error::PreconditionFailed(message) => Error::PreconditionFailed(contextual(message)),
        Error::InvalidInput(message) => Error::InvalidInput(contextual(message)),
        Error::SchemaValidation(message) => Error::SchemaValidation(contextual(message)),
        Error::Storage { kind, message } => Error::Storage {
            kind,
            message: contextual(message),
        },
        Error::HistoricalRead { kind, message } => Error::HistoricalRead {
            kind,
            message: contextual(message),
        },
        Error::Serialization(message) => Error::Serialization(contextual(message)),
        Error::Transport(message) => Error::Transport(contextual(message)),
        Error::Internal(message) => Error::Internal(contextual(message)),
        error => error,
    }
}
