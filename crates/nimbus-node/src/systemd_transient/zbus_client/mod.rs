//! Live [`SystemdDbusClient`] backed by `lucab/zbus_systemd` over a real
//! D-Bus connection. Compiled only under the `systemd-dbus` feature; the
//! crate otherwise keeps its fail-closed [`UnavailableSystemdDbusClient`]
//! default.
//!
//! NDB2 lands the skeleton: a cached [`zbus::Connection`] + `ManagerProxy`,
//! [`BusKind`] selection, and a capability probe run **once in the async
//! constructor** — the trait's [`SystemdDbusClient::capabilities`] is
//! synchronous and cannot perform D-Bus I/O, so the probe result is cached
//! and the accessor returns the cached copy. The trait methods are stubbed
//! here; signal-correlated job completion (NDB3), the error taxonomy (NDB4),
//! and the Linux integration tests (NDB5) build on top of this module.
//!
//! [`UnavailableSystemdDbusClient`]: super::UnavailableSystemdDbusClient

use std::time::Duration;

use nimbus_core::{Error, Result};
use zbus::Connection;
use zbus::fdo::PropertiesProxy;
use zbus::names::InterfaceName;
use zbus::zvariant::{OwnedObjectPath, OwnedValue};
use zbus_systemd::systemd1::{JobProxy, ManagerProxy, ServiceProxy};

use super::{
    SystemdDbusClient, SystemdInspectUnitRequest, SystemdStartTransientUnitRequest,
    SystemdStartTransientUnitResponse, SystemdStopUnitRequest, SystemdStopUnitResponse,
    SystemdStopUnitSubmission, SystemdTransientCapabilities, SystemdUnitJobStatus,
    SystemdUnitStatus,
};
use crate::HostLifecycleFuture;
use crate::host_lifecycle::HostActivationFence;

mod error;
mod properties;
mod signals;

use error::map_zbus;
use signals::{DEFAULT_SYSTEMD_JOB_COMPLETION_TIMEOUT, JobOutcome};

const SYSTEMD_UNIT_INTERFACE: &str = "org.freedesktop.systemd1.Unit";

#[derive(Debug, PartialEq, Eq)]
struct UnitLifecycleSnapshot {
    active_state: String,
    sub_state: String,
    job_id: u32,
    job_path: OwnedObjectPath,
}

impl UnitLifecycleSnapshot {
    fn from_properties(
        mut properties: std::collections::HashMap<String, OwnedValue>,
    ) -> Result<Self> {
        let active_state = String::try_from(required_property(&mut properties, "ActiveState")?)
            .map_err(|error| invalid_unit_property("ActiveState", error))?;
        let sub_state = String::try_from(required_property(&mut properties, "SubState")?)
            .map_err(|error| invalid_unit_property("SubState", error))?;
        let (job_id, job_path) =
            <(u32, OwnedObjectPath)>::try_from(required_property(&mut properties, "Job")?)
                .map_err(|error| invalid_unit_property("Job", error))?;
        Ok(Self {
            active_state,
            sub_state,
            job_id,
            job_path,
        })
    }
}

fn required_property(
    properties: &mut std::collections::HashMap<String, OwnedValue>,
    name: &'static str,
) -> Result<OwnedValue> {
    properties.remove(name).ok_or_else(|| {
        Error::InvalidInput(format!(
            "systemd Unit property snapshot omitted required property `{name}`"
        ))
    })
}

fn invalid_unit_property(name: &'static str, error: zbus::zvariant::Error) -> Error {
    Error::InvalidInput(format!(
        "systemd Unit property `{name}` has an invalid value: {error}"
    ))
}

/// Which systemd D-Bus instance the client speaks to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BusKind {
    /// The system bus — PID 1's systemd. Production. Requires uid 0 or a
    /// polkit rule granting `org.freedesktop.systemd1.manage-units` on the
    /// `Manager` interface.
    System,
    /// The session bus — the per-user `systemctl --user` manager. Used by the
    /// Linux integration tests; needs no privilege.
    Session,
}

impl BusKind {
    async fn connect(self) -> zbus::Result<Connection> {
        match self {
            BusKind::System => Connection::system().await,
            BusKind::Session => Connection::session().await,
        }
    }
}

/// Live systemd D-Bus client implementing [`SystemdDbusClient`] on top of the
/// generated `zbus_systemd` `Manager` proxy.
pub struct ZbusSystemdClient {
    // Retained so the proxy's underlying connection stays open; the signal
    // subscription and stream wiring in NDB3 read from it directly.
    #[allow(dead_code)]
    connection: Connection,
    manager: ManagerProxy<'static>,
    capabilities: SystemdTransientCapabilities,
    job_completion_timeout: Duration,
}

impl std::fmt::Debug for ZbusSystemdClient {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("ZbusSystemdClient")
            .field("capabilities", &self.capabilities)
            .field("job_completion_timeout", &self.job_completion_timeout)
            .finish_non_exhaustive()
    }
}

impl ZbusSystemdClient {
    /// Open a connection to `bus`, cache a `ManagerProxy`, and probe
    /// capabilities once.
    ///
    /// Fails only when the bus connection itself cannot be opened. A
    /// *reachable but degraded* daemon (e.g. an interface missing the methods
    /// we need) yields a client whose [`capabilities`](SystemdDbusClient::capabilities)
    /// report the degradation, so [`SystemdTransientUnitBackend`] fails closed
    /// rather than panicking.
    ///
    /// [`SystemdTransientUnitBackend`]: super::SystemdTransientUnitBackend
    pub async fn new(bus: BusKind) -> Result<Self> {
        let connection = bus.connect().await.map_err(|err| {
            Error::ResourceExhausted(format!("systemd D-Bus {bus:?} connection failed: {err}"))
        })?;
        Self::from_connection(connection).await
    }

    async fn from_connection(connection: Connection) -> Result<Self> {
        let manager = ManagerProxy::new(&connection).await.map_err(|err| {
            Error::ResourceExhausted(format!("systemd Manager proxy unavailable: {err}"))
        })?;
        let capabilities = probe_capabilities(&manager).await;
        Ok(Self {
            connection,
            manager,
            capabilities,
            job_completion_timeout: DEFAULT_SYSTEMD_JOB_COMPLETION_TIMEOUT,
        })
    }

    pub fn with_job_completion_timeout(mut self, timeout: Duration) -> Result<Self> {
        if timeout.is_zero() {
            return Err(Error::InvalidInput(
                "systemd job completion timeout must be greater than 0".to_string(),
            ));
        }
        self.job_completion_timeout = timeout;
        Ok(self)
    }

    /// Borrow the cached `Manager` proxy. Used by NDB3's signal wiring.
    #[allow(dead_code)]
    pub(crate) fn manager(&self) -> &ManagerProxy<'static> {
        &self.manager
    }

    /// Test-only constructor that injects a pre-built connection — e.g. an
    /// in-process p2p connection wired to a mock `Manager`. Lets the
    /// constructor + probe paths be exercised without a real systemd daemon.
    #[cfg(feature = "systemd-dbus-test-bus")]
    pub async fn from_connection_for_test(connection: Connection) -> Result<Self> {
        Self::from_connection(connection).await
    }

    async fn submit_stop_unit(
        &self,
        request: SystemdStopUnitRequest,
    ) -> Result<SystemdStopUnitSubmission> {
        let name = request.unit_name().as_str().to_string();
        let mode = request.mode().as_dbus_str().to_string();
        Ok(
            match signals::stop_unit_and_wait(
                &self.manager,
                name,
                mode,
                self.job_completion_timeout,
            )
            .await
            {
                signals::StopUnitSubmission::PreCallFailure(error) => {
                    SystemdStopUnitSubmission::pre_call_failure(error.to_string())
                }
                signals::StopUnitSubmission::UnknownSubmission(error) => {
                    SystemdStopUnitSubmission::unknown_submission(error.to_string())
                }
                signals::StopUnitSubmission::AcceptedJobIncomplete { job_path, error } => {
                    SystemdStopUnitSubmission::accepted_job_incomplete(
                        job_path.as_str().to_string(),
                        error.to_string(),
                    )?
                }
                signals::StopUnitSubmission::Terminal { job_path, outcome }
                    if outcome.succeeded() =>
                {
                    let status = SystemdUnitStatus::new(
                        request.execution_id().clone(),
                        request.unit_name().clone(),
                        "inactive",
                        "dead",
                    )?
                    .with_job_path(job_path.as_str().to_string())?;
                    SystemdStopUnitSubmission::Terminal(Box::new(SystemdStopUnitResponse::new(
                        job_path.as_str().to_string(),
                        status,
                    )?))
                }
                signals::StopUnitSubmission::Terminal { job_path, outcome } => {
                    SystemdStopUnitSubmission::terminal_failure(
                        job_path.as_str().to_string(),
                        outcome.label(),
                    )?
                }
            },
        )
    }

    async fn unit_lifecycle_snapshot(
        &self,
        unit_path: &OwnedObjectPath,
    ) -> Result<UnitLifecycleSnapshot> {
        let properties = PropertiesProxy::builder(&self.connection)
            .destination("org.freedesktop.systemd1")
            .map_err(map_zbus)?
            .path(unit_path.clone())
            .map_err(map_zbus)?
            .build()
            .await
            .map_err(map_zbus)?;
        let interface = InterfaceName::try_from(SYSTEMD_UNIT_INTERFACE)
            .expect("the static systemd Unit interface name must be valid");
        let properties = properties
            .get_all(interface)
            .await
            .map_err(zbus::Error::from)
            .map_err(map_zbus)?;
        UnitLifecycleSnapshot::from_properties(properties)
    }
}

/// Probe the daemon once with a cheap `GetUnit("init.scope")` round-trip:
/// - transport failure (no bus / disconnected) → `dbus_available = false`
/// - unknown method/interface → `transient_units = false`
/// - any answer at all (success, or a method error such as `NoSuchUnit`) →
///   the `Manager` interface replied, so it is reachable and usable
async fn probe_capabilities(manager: &ManagerProxy<'static>) -> SystemdTransientCapabilities {
    match manager.get_unit("init.scope".to_string()).await {
        Ok(_) => SystemdTransientCapabilities::available(),
        Err(err) => classify_probe_error(&err),
    }
}

/// Map a probe error to a capability set. D-Bus error *replies* arrive as
/// [`zbus::Error::MethodError`] keyed by error name; only zbus-internal
/// failures use [`zbus::Error::FDO`]. Both shapes are handled.
fn classify_probe_error(err: &zbus::Error) -> SystemdTransientCapabilities {
    use zbus::fdo::Error as Fdo;
    match err {
        // Transport / connection down: the bus itself is unreachable.
        zbus::Error::InputOutput(_) | zbus::Error::Address(_) | zbus::Error::Handshake(_) => {
            SystemdTransientCapabilities::unavailable()
        }
        // A named D-Bus error reply — classify by the error name.
        zbus::Error::MethodError(name, _, _) => match name.as_str() {
            "org.freedesktop.DBus.Error.Disconnected"
            | "org.freedesktop.DBus.Error.NoServer"
            | "org.freedesktop.DBus.Error.NoNetwork" => SystemdTransientCapabilities::unavailable(),
            "org.freedesktop.DBus.Error.UnknownMethod"
            | "org.freedesktop.DBus.Error.UnknownInterface"
            | "org.freedesktop.DBus.Error.ServiceUnknown" => {
                SystemdTransientCapabilities::available().without_transient_units()
            }
            // Any other reply (e.g. systemd `NoSuchUnit`) means the Manager
            // interface answered → reachable.
            _ => SystemdTransientCapabilities::available(),
        },
        // zbus-internal fdo errors (uncommon for an outgoing method call).
        zbus::Error::FDO(fdo) => match fdo.as_ref() {
            Fdo::Disconnected(_) | Fdo::NoServer(_) | Fdo::NoNetwork(_) => {
                SystemdTransientCapabilities::unavailable()
            }
            Fdo::UnknownMethod(_) | Fdo::UnknownInterface(_) | Fdo::ServiceUnknown(_) => {
                SystemdTransientCapabilities::available().without_transient_units()
            }
            _ => SystemdTransientCapabilities::available(),
        },
        zbus::Error::InterfaceNotFound => {
            SystemdTransientCapabilities::available().without_transient_units()
        }
        // Anything else: the daemon answered, so treat it as reachable.
        _ => SystemdTransientCapabilities::available(),
    }
}

fn is_no_such_unit(err: &zbus::Error) -> bool {
    matches!(err, zbus::Error::MethodError(name, _, _) if name.as_str().ends_with(".NoSuchUnit"))
}

fn job_failed_error(operation: &str, unit: &str, outcome: &JobOutcome) -> Error {
    let result = match outcome {
        JobOutcome::Failed(result) => result.as_str(),
        _ => "unknown",
    };
    Error::Internal(format!(
        "systemd {operation} for unit {unit} did not complete: job result `{result}`"
    ))
}

impl SystemdDbusClient for ZbusSystemdClient {
    fn capabilities(&self) -> SystemdTransientCapabilities {
        self.capabilities.clone()
    }

    fn start_transient_unit<'a>(
        &'a self,
        request: SystemdStartTransientUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStartTransientUnitResponse> {
        Box::pin(async move {
            let name = request.unit_name().as_str().to_string();
            let mode = request.mode().as_dbus_str().to_string();
            let properties = properties::encode_start_properties(request.properties())?;
            let (job_path, outcome) = signals::start_transient_unit_and_wait(
                &self.manager,
                name,
                mode,
                properties,
                self.job_completion_timeout,
            )
            .await?;
            if !outcome.succeeded() {
                return Err(job_failed_error(
                    "StartTransientUnit",
                    request.unit_name().as_str(),
                    &outcome,
                ));
            }
            SystemdStartTransientUnitResponse::new(
                request.unit_name().clone(),
                job_path.as_str().to_string(),
            )
        })
    }

    fn stop_unit<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitResponse> {
        Box::pin(async move {
            match self.submit_stop_unit(request).await? {
                SystemdStopUnitSubmission::Terminal(response) => Ok(*response),
                SystemdStopUnitSubmission::PreCallFailure { error }
                | SystemdStopUnitSubmission::UnknownSubmission { error }
                | SystemdStopUnitSubmission::AcceptedJobIncomplete { error, .. } => {
                    Err(Error::Internal(error))
                }
                SystemdStopUnitSubmission::TerminalFailure { job_path, result } => {
                    Err(Error::Internal(format!(
                        "systemd StopUnit job {job_path} did not complete: job result `{result}`"
                    )))
                }
            }
        })
    }

    fn stop_unit_exact<'a>(
        &'a self,
        request: SystemdStopUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdStopUnitSubmission> {
        Box::pin(async move { self.submit_stop_unit(request).await })
    }

    fn inspect_unit<'a>(
        &'a self,
        request: SystemdInspectUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
        Box::pin(async move {
            let execution_id = request.execution_id().clone();
            let unit_name = request.unit_name().clone();
            let unit_path = match self.manager.get_unit(unit_name.as_str().to_string()).await {
                Ok(path) => path,
                // An unloaded unit (never started, or already GC'd after stop)
                // is reported as inactive/dead rather than an error.
                Err(err) if is_no_such_unit(&err) => {
                    return SystemdUnitStatus::absent_for_inspect_request(&request);
                }
                Err(err) => return Err(map_zbus(err)),
            };
            // `ActiveState`, `SubState`, and `Job` form one lifecycle
            // decision. Read them through one Properties.GetAll reply so a
            // completed job cannot be paired with stale terminal state.
            let initial_snapshot = self.unit_lifecycle_snapshot(&unit_path).await?;
            let service = ServiceProxy::builder(&self.connection)
                .path(unit_path.clone())
                .map_err(map_zbus)?
                .build()
                .await
                .map_err(map_zbus)?;
            let main_pid = service.main_pid().await.map_err(map_zbus)?;
            let activation_fence = HostActivationFence::from_log_extra_fields(
                &service.log_extra_fields().await.map_err(map_zbus)?,
            )?;
            let snapshot = self.unit_lifecycle_snapshot(&unit_path).await?;
            if snapshot != initial_snapshot {
                return Err(Error::Internal(
                    "systemd Unit lifecycle changed during inspection; retry the observation"
                        .to_owned(),
                ));
            }
            let mut status = SystemdUnitStatus::new(
                execution_id,
                unit_name,
                snapshot.active_state,
                snapshot.sub_state,
            )?;
            if main_pid != 0 {
                status = status.with_main_pid(main_pid);
            }
            if snapshot.job_id != 0 && snapshot.job_path.as_str() != "/" {
                let job = JobProxy::builder(&self.connection)
                    .path(snapshot.job_path.clone())
                    .map_err(map_zbus)?
                    .build()
                    .await
                    .map_err(map_zbus)?;
                status = status.with_current_job(SystemdUnitJobStatus::new(
                    snapshot.job_id,
                    snapshot.job_path.as_str().to_string(),
                    job.job_type().await.map_err(map_zbus)?,
                    job.state().await.map_err(map_zbus)?,
                )?);
            }
            if let Some(activation_fence) = activation_fence {
                status = status.with_activation_fence(activation_fence);
            }
            Ok(status)
        })
    }
}

#[cfg(all(test, feature = "systemd-dbus-test-bus"))]
mod tests {
    use std::sync::Arc;

    use super::*;

    // ---- classifier arms reachable without a connection ------------------

    #[test]
    fn transport_errors_mark_dbus_unavailable() {
        let io = zbus::Error::InputOutput(Arc::new(std::io::Error::other("no bus")));
        let caps = classify_probe_error(&io);
        assert!(!caps.dbus_available());
        assert!(!caps.transient_units());
    }

    #[test]
    fn internal_disconnected_marks_dbus_unavailable() {
        let err = zbus::Error::FDO(Box::new(zbus::fdo::Error::Disconnected("gone".into())));
        assert!(!classify_probe_error(&err).dbus_available());
    }

    #[test]
    fn interface_not_found_disables_transient_units_but_stays_reachable() {
        let caps = classify_probe_error(&zbus::Error::InterfaceNotFound);
        assert!(caps.dbus_available());
        assert!(!caps.transient_units());
    }

    #[test]
    fn internal_unknown_method_disables_transient_units() {
        let err = zbus::Error::FDO(Box::new(zbus::fdo::Error::UnknownMethod(
            "no GetUnit".into(),
        )));
        let caps = classify_probe_error(&err);
        assert!(caps.dbus_available());
        assert!(!caps.transient_units());
    }

    // The full constructor → probe → `GetUnit` round-trip against a real
    // `Manager` (including the `MethodError`/`NoSuchUnit` reply path, which
    // cannot be synthesized without a live `Message`) is covered by NDB5's
    // Linux session-bus integration test, where `systemctl --user` answers the
    // probe for real. These classifier unit tests cover the decision matrix;
    // `from_connection_for_test` is the seam NDB5 drives.
}
