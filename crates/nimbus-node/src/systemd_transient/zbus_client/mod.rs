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
use zbus_systemd::systemd1::{ManagerProxy, ServiceProxy, UnitProxy};

use super::{
    SystemdDbusClient, SystemdInspectUnitRequest, SystemdStartTransientUnitRequest,
    SystemdStartTransientUnitResponse, SystemdStopUnitRequest, SystemdStopUnitResponse,
    SystemdTransientCapabilities, SystemdUnitStatus,
};
use crate::HostLifecycleFuture;
use crate::host_lifecycle::HostActivationFence;

mod error;
mod properties;
mod signals;

use error::map_zbus;
use signals::{DEFAULT_SYSTEMD_JOB_COMPLETION_TIMEOUT, JobOutcome};

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
            let name = request.unit_name().as_str().to_string();
            let mode = request.mode().as_dbus_str().to_string();
            let (job_path, outcome) =
                signals::stop_unit_and_wait(&self.manager, name, mode, self.job_completion_timeout)
                    .await?;
            if !outcome.succeeded() {
                return Err(job_failed_error(
                    "StopUnit",
                    request.unit_name().as_str(),
                    &outcome,
                ));
            }
            // After a successful stop the transient unit is inactive/dead (and
            // typically garbage-collected). Report that terminal status.
            let status = SystemdUnitStatus::new(
                request.execution_id().clone(),
                request.unit_name().clone(),
                "inactive",
                "dead",
            )?
            .with_job_path(job_path.as_str().to_string())?;
            SystemdStopUnitResponse::new(job_path.as_str().to_string(), status)
        })
    }

    fn inspect_unit<'a>(
        &'a self,
        request: SystemdInspectUnitRequest,
    ) -> HostLifecycleFuture<'a, SystemdUnitStatus> {
        Box::pin(async move {
            let unit_name = request.unit_name().clone();
            let execution_id = request.execution_id().clone();
            let unit_path = match self.manager.get_unit(unit_name.as_str().to_string()).await {
                Ok(path) => path,
                // An unloaded unit (never started, or already GC'd after stop)
                // is reported as inactive/dead rather than an error.
                Err(err) if is_no_such_unit(&err) => {
                    return SystemdUnitStatus::explicitly_absent(execution_id, unit_name);
                }
                Err(err) => return Err(map_zbus(err)),
            };
            let unit = UnitProxy::builder(&self.connection)
                .path(unit_path.clone())
                .map_err(map_zbus)?
                .build()
                .await
                .map_err(map_zbus)?;
            let active_state = unit.active_state().await.map_err(map_zbus)?;
            let sub_state = unit.sub_state().await.map_err(map_zbus)?;
            let service = ServiceProxy::builder(&self.connection)
                .path(unit_path)
                .map_err(map_zbus)?
                .build()
                .await
                .map_err(map_zbus)?;
            let main_pid = service.main_pid().await.map_err(map_zbus)?;
            let activation_fence = HostActivationFence::from_log_extra_fields(
                &service.log_extra_fields().await.map_err(map_zbus)?,
            )?;
            let mut status =
                SystemdUnitStatus::new(execution_id, unit_name, active_state, sub_state)?;
            if main_pid != 0 {
                status = status.with_main_pid(main_pid);
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
