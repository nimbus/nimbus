//! Internal node-local workload executor for `NodeWorkloadReconciler`,
//! lifting the TSB14 deferral ("no production node/control-plane caller
//! that starts, stops, or inspects tenant workloads through
//! `HostLifecycleBackend`").
//!
//! Scope is deliberately the node side only: an internal caller describes
//! one assigned workload, the workload is admitted through the real
//! tenant-isolation authority (`TenantIsolationContext::operator`), and the
//! reconciler drives it to its desired state against the NDB systemd transient
//! unit backend in a converge loop. User-facing workload commands live above
//! this mechanism.

use std::error::Error;
use std::path::PathBuf;

use clap::{Args, ValueEnum};
use nimbus_server::local_enforcement::{
    HostExecutable, HostLifecycleBackendKind, HostLifecycleFuture, HostLifecycleRequest,
    StatusEvidenceWrite, StatusEvidenceWriter, TenantWorkloadSpec,
};
use nimbus_server::{TenantIsolationContext, TenantIsolationPolicyInput, WorkloadAttributes};

use crate::cli_ux;

#[derive(Debug, Clone, Copy, PartialEq, Eq, ValueEnum)]
pub(crate) enum NodeWorkloadExecutorBus {
    /// Per-user systemd manager (`systemctl --user`).
    Session,
    /// System systemd manager.
    System,
}

#[derive(Debug, Args)]
#[command(help_template = cli_ux::COMMAND_HELP_TEMPLATE)]
pub(crate) struct NodeWorkloadExecutorCommand {
    /// Tenant the workload belongs to.
    #[arg(long)]
    pub(crate) tenant: String,

    /// Workload name (becomes part of the transient unit identity).
    #[arg(long)]
    pub(crate) workload: String,

    /// Absolute path of the host executable to run.
    #[arg(long)]
    pub(crate) exec: String,

    /// Argument for the executable (repeatable, in order).
    #[arg(long = "arg", value_name = "ARG")]
    pub(crate) args: Vec<String>,

    /// Node identity recorded in the admission decision.
    #[arg(long, default_value = "local-node")]
    pub(crate) node_id: String,

    /// systemd bus to drive (production units use the system bus; the
    /// integration lane uses the session bus).
    #[arg(long, value_enum, default_value_t = NodeWorkloadExecutorBus::System)]
    pub(crate) bus: NodeWorkloadExecutorBus,

    /// JSONL file the reconciler appends status evidence to.
    #[arg(long)]
    pub(crate) status_path: PathBuf,

    /// Reconcile cadence in seconds.
    #[arg(long, default_value_t = 10)]
    pub(crate) interval_secs: u64,

    /// Run a single reconcile pass and exit.
    #[arg(long, default_value_t = false)]
    pub(crate) once: bool,

    /// Converge the workload to stopped instead of running.
    #[arg(long, default_value_t = false)]
    pub(crate) stop: bool,
}

/// Append-only JSONL evidence writer: one line per reconcile pass with
/// the projection and observed status, so node-side workload history is
/// inspectable without a control plane.
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
pub(crate) struct JsonlStatusWriter {
    path: PathBuf,
}

impl JsonlStatusWriter {
    #[cfg_attr(not(target_os = "linux"), allow(dead_code))]
    pub(crate) fn new(path: PathBuf) -> Self {
        Self { path }
    }
}

impl StatusEvidenceWriter for JsonlStatusWriter {
    fn write_status<'a>(&'a self, write: StatusEvidenceWrite<'a>) -> HostLifecycleFuture<'a, ()> {
        Box::pin(async move {
            let projection = write.projection();
            let line = serde_json::json!({
                "projection": {
                    "decision_id": projection.decision_id(),
                    "tenant_id": projection.tenant_id(),
                    "surface": projection.surface(),
                    "authority_class": projection.authority_class(),
                    "workload_uid": projection.workload_uid(),
                    "workload_subject": projection.workload_subject(),
                    "generation": projection.generation(),
                },
                "status": write.status(),
            });
            let encoded = format!("{line}\n");
            if let Some(parent) = self.path.parent()
                && !parent.as_os_str().is_empty()
            {
                std::fs::create_dir_all(parent).map_err(|error| {
                    nimbus::Error::Internal(format!(
                        "failed to create status evidence directory {}: {error}",
                        parent.display()
                    ))
                })?;
            }
            use std::io::Write as _;
            let mut file = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(&self.path)
                .map_err(|error| {
                    nimbus::Error::Internal(format!(
                        "failed to open status evidence file {}: {error}",
                        self.path.display()
                    ))
                })?;
            file.write_all(encoded.as_bytes()).map_err(|error| {
                nimbus::Error::Internal(format!(
                    "failed to append status evidence to {}: {error}",
                    self.path.display()
                ))
            })?;
            Ok(())
        })
    }
}

/// Admit the operator-described workload through the real tenant
/// isolation authority and materialize the reconciler spec from the
/// decision — the same decision→spec pipeline the server-side
/// enforcement uses; this command invents no parallel path.
pub(crate) fn admit_workload_spec(
    tenant: &str,
    workload: &str,
    node_id: &str,
    stop: bool,
) -> Result<TenantWorkloadSpec, Box<dyn Error>> {
    let tenant_id = nimbus::TenantId::new(tenant)?;
    let context = TenantIsolationContext::operator(tenant_id, "node.workload_executor")
        .with_workload_location(nimbus_server::WorkloadLocation::new().with_node_id(node_id));
    let decision = context.admit_decision(TenantIsolationPolicyInput::new(
        WorkloadAttributes::service(workload),
    ))?;
    let spec = TenantWorkloadSpec::from_decision(&decision)?;
    Ok(if stop {
        spec.mark_deleting_server_owned([])
    } else {
        spec
    })
}

pub(crate) fn lifecycle_request(
    exec: &str,
    args: &[String],
) -> Result<HostLifecycleRequest, Box<dyn Error>> {
    Ok(HostLifecycleRequest::new(
        HostLifecycleBackendKind::SystemdTransientUnit,
        HostExecutable::trusted(exec)?,
    )
    .with_args(args.iter().cloned())?)
}

#[cfg(target_os = "linux")]
pub(crate) async fn run_node_workload_executor_command(
    command: NodeWorkloadExecutorCommand,
) -> Result<(), Box<dyn Error>> {
    use nimbus_server::local_enforcement::{
        BusKind, NodeWorkloadReconciler, SystemdTransientUnitBackend, ZbusSystemdClient,
    };

    let bus = match command.bus {
        NodeWorkloadExecutorBus::Session => BusKind::Session,
        NodeWorkloadExecutorBus::System => BusKind::System,
    };
    let client = ZbusSystemdClient::new(bus).await.map_err(|error| {
        format!(
            "nimbus node-workload-executor requires a reachable systemd D-Bus manager \
             ({:?} bus): {error}",
            command.bus
        )
    })?;
    let backend = SystemdTransientUnitBackend::new(client);
    let writer = JsonlStatusWriter::new(command.status_path.clone());
    let reconciler = NodeWorkloadReconciler::new(backend, writer);

    let spec = admit_workload_spec(
        &command.tenant,
        &command.workload,
        &command.node_id,
        command.stop,
    )?;
    let request = lifecycle_request(&command.exec, &command.args)?;

    loop {
        let outcome = reconciler
            .reconcile_spec(spec.clone(), request.clone())
            .await?;
        emit_node_workload_executor_info(format!(
            "workload {} desired={:?} action={:?}",
            outcome.workload_id().as_str(),
            outcome.desired_state(),
            outcome.action(),
        ));
        if command.once {
            return Ok(());
        }
        tokio::select! {
            _ = tokio::time::sleep(std::time::Duration::from_secs(command.interval_secs.max(1))) => {}
            result = tokio::signal::ctrl_c() => {
                result?;
                emit_node_workload_executor_info("shutting down node reconcile loop");
                return Ok(());
            }
        }
    }
}

#[cfg(not(target_os = "linux"))]
pub(crate) async fn run_node_workload_executor_command(
    command: NodeWorkloadExecutorCommand,
) -> Result<(), Box<dyn Error>> {
    // Validate the inputs so configuration errors surface on any platform,
    // then fail actionably: the reconciler drives systemd transient units.
    let _spec = admit_workload_spec(
        &command.tenant,
        &command.workload,
        &command.node_id,
        command.stop,
    )?;
    let _request = lifecycle_request(&command.exec, &command.args)?;
    Err(
        "nimbus node-workload-executor drives systemd transient units and requires a Linux systemd host"
            .to_string()
            .into(),
    )
}

#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
fn emit_node_workload_executor_info(message: impl AsRef<str>) {
    if cli_ux::info_output_enabled() {
        let _ = cli_ux::write_stderr_prefixed_line("info:", message.as_ref());
    } else {
        println!("{}", message.as_ref());
    }
}

/// NDB-lane live test: the hidden node-workload-executor path converges a
/// real session-systemd transient unit to running, observes idempotency,
/// then converges it to stopped — with JSONL evidence for every pass.
/// Same no-silent-skip posture as `zbus_systemd_live`: when the gate is
/// on, an unreachable session bus is a FAILURE.
#[cfg(all(
    test,
    target_os = "linux",
    feature = "node-workload-executor-integration-tests"
))]
mod live_tests {
    use super::*;

    fn command(
        workload: &str,
        status_path: std::path::PathBuf,
        stop: bool,
    ) -> NodeWorkloadExecutorCommand {
        NodeWorkloadExecutorCommand {
            tenant: "demo".to_string(),
            workload: workload.to_string(),
            exec: "/usr/bin/sleep".to_string(),
            args: vec!["30".to_string()],
            node_id: "ci-node".to_string(),
            bus: NodeWorkloadExecutorBus::Session,
            status_path,
            interval_secs: 1,
            once: true,
            stop,
        }
    }

    #[tokio::test]
    async fn node_workload_executor_converges_transient_unit() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let status_path = temp.path().join("status.jsonl");
        let workload = format!(
            "lr12-live-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .expect("clock after epoch")
                .as_nanos()
        );

        run_node_workload_executor_command(command(&workload, status_path.clone(), false))
            .await
            .expect("first pass must converge the workload to running");
        run_node_workload_executor_command(command(&workload, status_path.clone(), false))
            .await
            .expect("second pass must observe the running unit idempotently");
        run_node_workload_executor_command(command(&workload, status_path.clone(), true))
            .await
            .expect("stop pass must converge the workload to stopped");

        let evidence = std::fs::read_to_string(&status_path).expect("evidence should read");
        let lines: Vec<serde_json::Value> = evidence
            .lines()
            .map(|line| serde_json::from_str(line).expect("evidence line should be JSON"))
            .collect();
        assert_eq!(lines.len(), 3, "one evidence line per reconcile pass");
        for line in &lines {
            assert_eq!(line["projection"]["tenant_id"], "demo");
            assert!(line["status"].is_object());
        }
        let first_phase = lines[0]["status"]["phase"].to_string();
        let last_phase = lines[2]["status"]["phase"].to_string();
        assert_ne!(
            first_phase, last_phase,
            "running and stopped passes must record different phases              (first {first_phase}, last {last_phase})"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn operator_admission_materializes_a_running_spec() {
        let spec = admit_workload_spec("demo", "worker", "node-1", false)
            .expect("operator admission should produce a spec");
        assert_eq!(spec.tenant_id().as_str(), "demo");
        assert_eq!(
            nimbus_server::local_enforcement::NodeWorkloadDesiredState::from_spec(&spec),
            nimbus_server::local_enforcement::NodeWorkloadDesiredState::Running,
        );

        let stopped = admit_workload_spec("demo", "worker", "node-1", true)
            .expect("stop admission should produce a spec");
        assert_eq!(
            nimbus_server::local_enforcement::NodeWorkloadDesiredState::from_spec(&stopped),
            nimbus_server::local_enforcement::NodeWorkloadDesiredState::Stopped,
        );
    }

    #[test]
    fn lifecycle_request_requires_an_absolute_trusted_executable() {
        lifecycle_request("/usr/bin/sleep", &["30".to_string()])
            .expect("absolute executable should build a request");
        let error = lifecycle_request("sleep", &[]).expect_err("relative path must be rejected");
        assert!(error.to_string().contains("absolute path"));
    }

    #[tokio::test]
    async fn jsonl_writer_appends_projection_and_status_lines() {
        let temp = tempfile::tempdir().expect("tempdir should build");
        let path = temp.path().join("evidence/status.jsonl");
        let spec =
            admit_workload_spec("demo", "worker", "node-1", false).expect("spec should admit");
        let binding =
            nimbus_server::local_enforcement::LocalEnforcementBinding::from_spec(spec.clone());
        let projection = binding.system_evidence_projection();
        let plan = nimbus_server::local_enforcement::HostLifecyclePlan::from_binding(
            &binding,
            lifecycle_request("/usr/bin/sleep", &[]).expect("request should build"),
        )
        .expect("plan should build");
        let status = nimbus_server::local_enforcement::HostLifecycleStatus::from_backend_state(
            &plan,
            nimbus_server::local_enforcement::HostBackendObservedState::Stopped,
        )
        .to_workload_status(&plan)
        .expect("status should build");

        let writer = JsonlStatusWriter::new(path.clone());
        let write = StatusEvidenceWrite::new(&projection, &status).expect("write should validate");
        writer
            .write_status(write)
            .await
            .expect("append should succeed");
        let contents = std::fs::read_to_string(&path).expect("evidence file should read");
        let line: serde_json::Value =
            serde_json::from_str(contents.lines().next().expect("one line"))
                .expect("line should be JSON");
        assert!(line["projection"].is_object() && line["status"].is_object());
    }
}
