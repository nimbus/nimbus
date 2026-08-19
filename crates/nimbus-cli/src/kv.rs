use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::io::{self, Write};
use std::net::{Ipv4Addr, SocketAddr};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use clap::Args;
use nimbus::TenantId;
use nimbus_kv::{
    CredentialRegistry, NimbusKvConfig, NimbusKvListener, NimbusKvListenerConfig, NimbusKvStore,
    TieringConfig, bind_listener, serve_listener,
};
use nimbus_network::{
    LocalNetworkAuthority, LocalNetworkManager, LocalNetworkManagerError,
    NetworkCapabilityRegistry, NetworkCapabilityRegistryError,
};
use nimbus_operator::LocalNodeNetworkRoot;

#[derive(Debug, Args)]
#[command(
    help_template = crate::cli_ux::COMMAND_HELP_TEMPLATE,
    after_help = "Examples:\n  nimbus kv --tenant demo\n  nimbus kv --bind 127.0.0.1:6380 --tenant demo --password local-secret\n"
)]
pub(crate) struct KvCommand {
    /// Address for the RESP listener. Must be loopback.
    #[arg(long, default_value = "127.0.0.1:6380")]
    bind: SocketAddr,
    /// Tenant id bound to the generated or supplied dev credential.
    #[arg(long, default_value = "demo")]
    tenant: String,
    /// Username for AUTH. Defaults to the tenant id.
    #[arg(long)]
    username: Option<String>,
    /// Password for AUTH. Defaults to NIMBUS_KV_PASSWORD or a generated value.
    #[arg(long)]
    password: Option<String>,
    /// redb tenant file for durable mode.
    #[arg(long)]
    data: Option<PathBuf>,
    /// Host-local root for process-wide network allocation authority.
    #[arg(long)]
    network_state_dir: Option<PathBuf>,
    /// Keep state in memory only.
    #[arg(long)]
    no_disk: bool,
    /// Disable the read-through cache.
    #[arg(long)]
    no_cache: bool,
    /// Approximate cache byte budget.
    #[arg(long)]
    maxmemory: Option<usize>,
}

pub(crate) async fn run_kv_command(command: KvCommand) -> Result<(), Box<dyn Error>> {
    let prepared = {
        let stdout = io::stdout();
        let mut output = stdout.lock();
        prepare_and_announce_kv_command(command, &mut output).await?
    };
    prepared.serve().await?;
    Ok(())
}

async fn prepare_and_announce_kv_command(
    command: KvCommand,
    output: &mut (impl Write + ?Sized),
) -> Result<BoundStandaloneKvCommand, Box<dyn Error>> {
    let prepared = prepare_kv_command(command).await?;
    if let Err(error) = prepared.startup.write_to(output) {
        return Err(prepared.close_after_output_error(error));
    }
    Ok(prepared)
}

async fn prepare_kv_command(
    command: KvCommand,
) -> Result<BoundStandaloneKvCommand, Box<dyn Error>> {
    let tenant = TenantId::new(command.tenant.clone())?;
    let username = command
        .username
        .clone()
        .unwrap_or_else(|| tenant.as_str().to_owned());
    let password = command
        .password
        .clone()
        .or_else(|| std::env::var("NIMBUS_KV_PASSWORD").ok());

    let (credentials, password) = match password {
        Some(password) => (
            CredentialRegistry::new().bind(username.clone(), password.clone(), tenant.clone()),
            password,
        ),
        None => {
            let (credentials, generated) =
                CredentialRegistry::generated_dev_for(username.clone(), tenant.clone());
            (credentials, generated.password)
        }
    };

    validate_kv_command(&command)?;
    let network = prepare_standalone_kv_network(command.network_state_dir.as_deref())?;
    let store = kv_store_for_command(&command, &tenant)?;
    let listener = NimbusKvListenerConfig::from_network_authority(network.authority());
    let config = NimbusKvConfig::new(command.bind, credentials, listener).with_store(store);
    let listener = bind_listener(&config).await?;
    let bound_addr = match listener.local_addr() {
        Ok(bound_addr) => bound_addr,
        Err(primary) => {
            return Err(match listener.close_and_settle() {
                Ok(()) => Box::new(primary),
                Err(cleanup) => Box::new(io::Error::new(
                    primary.kind(),
                    format!(
                        "{primary}; failed to settle standalone KV listener after address observation failed: {cleanup}"
                    ),
                )),
            });
        }
    };
    let startup = KvStartupObservation {
        bound_addr,
        username,
        password,
        tenant,
    };
    Ok(BoundStandaloneKvCommand {
        network,
        listener,
        config,
        startup,
    })
}

#[derive(Debug)]
struct PreparedStandaloneKvNetwork {
    manager: Arc<LocalNetworkManager>,
}

impl PreparedStandaloneKvNetwork {
    fn authority(&self) -> LocalNetworkAuthority {
        self.manager.authority()
    }

    #[cfg(test)]
    fn manager(&self) -> &Arc<LocalNetworkManager> {
        &self.manager
    }
}

#[derive(Debug)]
enum StandaloneKvNetworkError {
    Root(io::Error),
    Manager(LocalNetworkManagerError),
    Registry(NetworkCapabilityRegistryError),
}

impl Display for StandaloneKvNetworkError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Root(error) => {
                write!(
                    formatter,
                    "failed to resolve standalone KV network root: {error}"
                )
            }
            Self::Manager(error) => write!(formatter, "{error}"),
            Self::Registry(error) => write!(
                formatter,
                "failed to freeze standalone KV network capabilities: {error}"
            ),
        }
    }
}

impl Error for StandaloneKvNetworkError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Root(error) => Some(error),
            Self::Manager(error) => Some(error),
            Self::Registry(error) => Some(error),
        }
    }
}

#[cfg(test)]
fn resolve_kv_network_root(command: &KvCommand) -> io::Result<LocalNodeNetworkRoot> {
    LocalNodeNetworkRoot::resolve_for_current_platform(command.network_state_dir.as_deref())
}

fn prepare_standalone_kv_network(
    explicit_root: Option<&Path>,
) -> Result<PreparedStandaloneKvNetwork, StandaloneKvNetworkError> {
    let root = LocalNodeNetworkRoot::resolve_for_current_platform(explicit_root)
        .map_err(StandaloneKvNetworkError::Root)?;
    let bootstrap = LocalNetworkManager::bootstrap(root.as_path())
        .map_err(StandaloneKvNetworkError::Manager)?;
    let registry =
        NetworkCapabilityRegistry::new(Vec::new()).map_err(StandaloneKvNetworkError::Registry)?;
    Ok(PreparedStandaloneKvNetwork {
        manager: bootstrap.freeze(registry),
    })
}

struct KvStartupObservation {
    bound_addr: SocketAddr,
    username: String,
    password: String,
    tenant: TenantId,
}

impl KvStartupObservation {
    fn write_to(&self, output: &mut (impl Write + ?Sized)) -> io::Result<()> {
        writeln!(output, "Nimbus KV listening on {}", self.bound_addr)?;
        writeln!(
            output,
            "Dev credential: AUTH {} {} (tenant {})",
            self.username,
            self.password,
            self.tenant.as_str()
        )?;
        output.flush()
    }
}

struct BoundStandaloneKvCommand {
    network: PreparedStandaloneKvNetwork,
    listener: NimbusKvListener,
    config: NimbusKvConfig,
    startup: KvStartupObservation,
}

impl BoundStandaloneKvCommand {
    async fn serve(self) -> Result<(), nimbus_kv::KvError> {
        let Self {
            network,
            listener,
            config,
            startup: _,
        } = self;
        let result = serve_listener(listener, config).await;
        drop(network);
        result
    }

    fn close_after_output_error(self, primary: io::Error) -> Box<dyn Error> {
        let Self {
            network: _,
            listener,
            config: _,
            startup: _,
        } = self;
        match listener.close_and_settle() {
            Ok(()) => Box::new(primary),
            Err(cleanup) => Box::new(io::Error::new(
                primary.kind(),
                format!(
                    "{primary}; failed to settle standalone KV listener after output failure: {cleanup}"
                ),
            )),
        }
    }

    #[cfg(test)]
    fn local_addr(&self) -> io::Result<SocketAddr> {
        self.listener.local_addr()
    }

    #[cfg(test)]
    fn close_and_settle(self) -> Result<(), nimbus_network::PortLeaseError> {
        self.listener.close_and_settle()
    }
}

fn validate_kv_command(command: &KvCommand) -> Result<(), Box<dyn Error>> {
    if command.no_disk && command.no_cache {
        return Err("nimbus kv cannot combine --no-disk and --no-cache".into());
    }
    nimbus_core::refuse_non_loopback_bind(command.bind)?;
    Ok(())
}

fn kv_store_for_command(
    command: &KvCommand,
    tenant: &TenantId,
) -> Result<NimbusKvStore, Box<dyn Error>> {
    let tiering = match (command.no_disk, command.no_cache) {
        (true, false) => TieringConfig::no_disk(),
        (false, true) => TieringConfig::no_cache(),
        (false, false) => TieringConfig::durable(),
        (true, true) => unreachable!("validated above"),
    };
    let tiering = match command.maxmemory {
        Some(maxmemory) => tiering.with_maxmemory(maxmemory),
        None => tiering,
    };

    if command.no_disk {
        return Ok(NimbusKvStore::no_disk(tiering)?);
    }

    let data = command
        .data
        .clone()
        .unwrap_or_else(|| default_data_path(tenant));
    if let Some(parent) = data.parent() {
        std::fs::create_dir_all(parent)?;
    }
    Ok(NimbusKvStore::durable_at(data, tiering)?)
}

fn default_data_path(tenant: &TenantId) -> PathBuf {
    std::env::current_dir()
        .unwrap_or_else(|_| PathBuf::from("."))
        .join(".nimbus")
        .join("kv")
        .join(format!("{}.redb", tenant.as_str()))
}

impl Default for KvCommand {
    fn default() -> Self {
        Self {
            bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 6380)),
            tenant: "demo".to_owned(),
            username: None,
            password: None,
            data: None,
            network_state_dir: None,
            no_disk: false,
            no_cache: false,
            maxmemory: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use std::io::{self, Write};
    use std::net::TcpListener as StdTcpListener;
    use std::sync::LazyLock;

    use clap::Parser;
    use nimbus_kv::bind_listener;
    use nimbus_network::{
        LocalNetworkManagerError, LocalNetworkStateStore, PortLeaseError, PortLeasePhase,
    };
    use nimbus_process_harness::PortWindow;
    use nimbus_server::PreboundServerListeners;
    use tokio::sync::{Mutex, MutexGuard};

    use super::*;

    static KV_NETWORK_TEST_LOCK: LazyLock<Mutex<()>> = LazyLock::new(|| Mutex::new(()));

    async fn lock_kv_network_test() -> MutexGuard<'static, ()> {
        KV_NETWORK_TEST_LOCK.lock().await
    }

    #[test]
    fn kv_network_state_dir_is_an_explicit_node_root_and_control_flag_is_removed() {
        let cli = crate::Cli::parse_from([
            "nimbus",
            "kv",
            "--network-state-dir",
            "/node-network",
            "--no-disk",
        ]);
        let crate::Command::Kv(command) = cli.command else {
            panic!("nimbus kv should parse");
        };
        assert_eq!(
            command.network_state_dir.as_deref(),
            Some(std::path::Path::new("/node-network"))
        );
        assert!(
            crate::Cli::try_parse_from(["nimbus", "kv", "--control-data-dir", "/legacy-control"])
                .is_err(),
            "the ambiguous control-data network-root flag must be removed"
        );
        let help = crate::Cli::try_parse_from(["nimbus", "kv", "--help"])
            .expect_err("KV help should short-circuit");
        assert_eq!(help.kind(), clap::error::ErrorKind::DisplayHelp);
        let rendered = help.to_string();
        assert!(rendered.contains("--network-state-dir"));
        assert!(!rendered.contains("--control-data-dir"));
    }

    #[test]
    fn network_state_root_uses_operator_policy_not_data_or_control_fallbacks() {
        let command = KvCommand {
            network_state_dir: Some(PathBuf::from("/flag-root")),
            data: Some(PathBuf::from("/unrelated-kv-data.redb")),
            ..KvCommand::default()
        };
        assert_eq!(
            resolve_kv_network_root(&command)
                .expect("explicit node root should resolve")
                .as_path(),
            std::path::Path::new("/flag-root")
        );
        let removed_control_environment = ["NIMBUS_", "CONTROL_DATA_DIR"].concat();
        assert!(
            !include_str!("kv.rs").contains(&removed_control_environment),
            "KV network composition must not read the control-plane root environment"
        );
        let removed_working_directory_fallback = ["PathBuf::from(\".", "/data\")"].concat();
        assert!(
            !include_str!("kv.rs").contains(&removed_working_directory_fallback),
            "KV network composition must not fall back to a working-directory data root"
        );
    }

    #[tokio::test]
    async fn invalid_kv_inputs_fail_before_network_data_or_output_effects() {
        let _test_lock = lock_kv_network_test().await;
        let fixture = tempfile::tempdir().expect("fixture root should exist");

        for (name, bind, no_disk, no_cache) in [
            (
                "non-loopback",
                SocketAddr::from(([0, 0, 0, 0], 6380)),
                false,
                false,
            ),
            (
                "conflicting-store-modes",
                SocketAddr::from((Ipv4Addr::LOCALHOST, 6380)),
                true,
                true,
            ),
        ] {
            let network_root = fixture.path().join(format!("{name}-network"));
            let data_path = fixture.path().join(format!("{name}.redb"));
            let mut output = Vec::new();
            let error = match prepare_and_announce_kv_command(
                KvCommand {
                    bind,
                    password: Some("test-secret".to_owned()),
                    data: Some(data_path.clone()),
                    network_state_dir: Some(network_root.clone()),
                    no_disk,
                    no_cache,
                    ..KvCommand::default()
                },
                &mut output,
            )
            .await
            {
                Ok(_) => panic!("{name} input must fail"),
                Err(error) => error,
            };
            assert!(
                !error.to_string().is_empty(),
                "{name} should retain a diagnostic"
            );
            assert!(output.is_empty(), "{name} must not emit success output");
            assert!(
                !LocalNetworkStateStore::authority_path_for(&network_root).exists(),
                "{name} must fail before network authority creation"
            );
            assert!(
                !data_path.exists(),
                "{name} must fail before KV data creation"
            );
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn manager_and_store_failures_emit_no_success_and_precede_later_effects() {
        let _test_lock = lock_kv_network_test().await;
        let fixture = tempfile::tempdir().expect("fixture root should exist");
        let active_root = fixture.path().join("active-network");
        let active = prepare_standalone_kv_network(Some(&active_root))
            .expect("active composition should claim the node");
        let canonical_active =
            std::fs::canonicalize(&active_root).expect("active root should canonicalize");

        for (name, attempted_root, divergent) in [
            ("same", active_root.clone(), false),
            ("lexical-alias", active_root.join("."), false),
            ("divergent", fixture.path().join("divergent-network"), true),
        ] {
            let data_path = fixture.path().join(format!("{name}.redb"));
            let mut output = Vec::new();
            let error = match prepare_and_announce_kv_command(
                KvCommand {
                    bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                    password: Some("test-secret".to_owned()),
                    data: Some(data_path.clone()),
                    network_state_dir: Some(attempted_root.clone()),
                    ..KvCommand::default()
                },
                &mut output,
            )
            .await
            {
                Ok(_) => panic!("a second standalone composition must fail"),
                Err(error) => error,
            };
            match error.downcast_ref::<StandaloneKvNetworkError>() {
                Some(StandaloneKvNetworkError::Manager(
                    LocalNetworkManagerError::DuplicateProcessComposition {
                        active_authority_path,
                        attempted_authority_path,
                    },
                )) => {
                    assert_eq!(
                        active_authority_path,
                        &LocalNetworkStateStore::authority_path_for(&canonical_active)
                    );
                    assert_eq!(
                        attempted_authority_path == active_authority_path,
                        !divergent,
                        "{name} should preserve exact active/attempted root evidence"
                    );
                }
                other => panic!("expected typed duplicate composition for {name}, got {other:?}"),
            }
            assert!(output.is_empty(), "{name} must not emit success output");
            assert!(
                !data_path.exists(),
                "{name} must fail before KV data creation"
            );
            if divergent {
                assert!(
                    !LocalNetworkStateStore::authority_path_for(&attempted_root).exists(),
                    "divergent rejection must not mutate the attempted root"
                );
            }
        }
        drop(active);

        let store_network_root = fixture.path().join("store-failure-network");
        let invalid_data_path = fixture.path().join("existing-directory");
        std::fs::create_dir(&invalid_data_path).expect("invalid data directory should exist");
        let mut output = Vec::new();
        let error = match prepare_and_announce_kv_command(
            KvCommand {
                bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                password: Some("test-secret".to_owned()),
                data: Some(invalid_data_path),
                network_state_dir: Some(store_network_root.clone()),
                ..KvCommand::default()
            },
            &mut output,
        )
        .await
        {
            Ok(_) => panic!("opening a directory as a redb file must fail"),
            Err(error) => error,
        };
        assert!(
            !error.to_string().is_empty(),
            "store failure should retain a diagnostic"
        );
        assert!(
            output.is_empty(),
            "KV store failure must not emit listening or credential success"
        );
        let reopened = prepare_standalone_kv_network(Some(&store_network_root))
            .expect("store failure must release the process composition claim");
        assert!(
            reopened
                .manager()
                .port_leases()
                .list()
                .expect("port authority should remain readable")
                .is_empty(),
            "store failure must precede listener reservation and bind"
        );
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn standalone_kv_freezes_empty_registry_and_rejects_divergent_root_before_mutation() {
        let _test_lock = lock_kv_network_test().await;
        let fixture = tempfile::tempdir().expect("fixture root should exist");
        let canonical_fixture =
            std::fs::canonicalize(fixture.path()).expect("fixture root should canonicalize");
        let active_root = fixture.path().join("active");
        let attempted_root = fixture.path().join("attempted");
        let active = prepare_standalone_kv_network(Some(&active_root))
            .expect("first standalone KV composition should claim the node");
        assert_eq!(active.manager().capability_registry().selections().len(), 0);

        let error = match prepare_standalone_kv_network(Some(&attempted_root)) {
            Ok(_) => panic!("a divergent composition must fail"),
            Err(error) => error,
        };
        match error {
            StandaloneKvNetworkError::Manager(
                LocalNetworkManagerError::DuplicateProcessComposition {
                    active_authority_path,
                    attempted_authority_path,
                },
            ) => {
                assert_eq!(
                    active_authority_path,
                    LocalNetworkStateStore::authority_path_for(canonical_fixture.join("active"))
                );
                assert_eq!(
                    attempted_authority_path,
                    LocalNetworkStateStore::authority_path_for(&attempted_root)
                );
            }
            other => panic!("expected typed duplicate composition, got {other:?}"),
        }
        assert!(
            !LocalNetworkStateStore::authority_path_for(&attempted_root).exists(),
            "rejected divergent root must not be mutated"
        );
        drop(active);
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn server_and_kv_conflict_durably_before_the_losing_kv_bind() {
        let _test_lock = lock_kv_network_test().await;
        let fixture = tempfile::tempdir().expect("fixture root should exist");
        let composition = prepare_standalone_kv_network(Some(fixture.path()))
            .expect("standalone composition should initialize");
        // The window holds this port against every other process for the whole
        // case, so the kernel bind at the end can only fail if the losing KV
        // binder really touched the socket. The serial lock above orders the
        // other KV network tests in this process; it says nothing about the
        // rest of the machine.
        let port_window = PortWindow::claim();
        let addr = SocketAddr::from((Ipv4Addr::LOCALHOST, port_window.port(0)));

        let server = PreboundServerListeners::new(composition.authority());
        let prepared_server = server
            .prepare("main", addr)
            .expect("server should own the durable winner");
        let winner_records = composition
            .manager()
            .port_leases()
            .list()
            .expect("durable server winner should be observable");
        assert_eq!(winner_records.len(), 1);
        let winner_owner = format!("{:?}", winner_records[0].request().owner_id());
        let kv_config = NimbusKvConfig::new(
            addr,
            CredentialRegistry::single_dev(
                TenantId::new("tenant-a").expect("tenant should validate"),
                "secret",
            ),
            NimbusKvListenerConfig::from_network_authority_for_incarnation(
                composition.authority(),
                "server-conflict-loser",
            ),
        );
        let loser_owner = format!(
            "{:?}",
            nimbus_network::NetworkResourceId::from(kv_config.listener.listener_id().clone())
        );

        let error = match bind_listener(&kv_config).await {
            Ok(_) => panic!("KV must lose in durable authority before kernel bind"),
            Err(error) => error,
        };
        assert!(
            matches!(
                error,
                nimbus_kv::KvError::Network(PortLeaseError::PortConflict { .. })
            ),
            "expected typed durable port conflict, got {error:?}"
        );
        let diagnostic = error.to_string();
        assert!(
            diagnostic.contains(&winner_owner),
            "winner identity missing: {diagnostic}"
        );
        assert!(
            diagnostic.contains(&loser_owner),
            "loser identity missing: {diagnostic}"
        );

        let kernel = StdTcpListener::bind(addr)
            .expect("rejected KV preparation must not invoke the losing binder");
        prepared_server
            .adopt_std(kernel)
            .expect("durable server winner should adopt the untouched kernel address")
            .close_and_settle()
            .expect("server winner should settle after confirmed close");
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn startup_observation_uses_actual_bound_address_and_prebind_failure_is_silent() {
        let _test_lock = lock_kv_network_test().await;
        let success_root = tempfile::tempdir().expect("success root should exist");
        let mut success_output = Vec::new();
        let prepared = prepare_and_announce_kv_command(
            KvCommand {
                bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                password: Some("test-secret".to_owned()),
                network_state_dir: Some(success_root.path().to_path_buf()),
                no_disk: true,
                ..KvCommand::default()
            },
            &mut success_output,
        )
        .await
        .expect("provider-assigned KV listener should prepare");
        let actual = prepared
            .local_addr()
            .expect("prepared listener should report an address");
        assert_ne!(actual.port(), 0);
        let rendered = String::from_utf8(success_output).expect("startup output should be UTF-8");
        assert!(
            rendered.contains(&format!("Nimbus KV listening on {actual}")),
            "startup output must render the kernel address: {rendered}"
        );
        prepared
            .close_and_settle()
            .expect("test listener should settle after confirmed close");

        let failure_root = tempfile::tempdir().expect("failure root should exist");
        let external =
            StdTcpListener::bind((Ipv4Addr::LOCALHOST, 0)).expect("external listener should bind");
        let occupied = external
            .local_addr()
            .expect("external address should resolve");
        let mut failure_output = Vec::new();
        let error = match prepare_and_announce_kv_command(
            KvCommand {
                bind: occupied,
                password: Some("test-secret".to_owned()),
                network_state_dir: Some(failure_root.path().to_path_buf()),
                no_disk: true,
                ..KvCommand::default()
            },
            &mut failure_output,
        )
        .await
        {
            Ok(_) => panic!("external collision must reject standalone KV"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("address already in use")
                || error.to_string().contains("Address already in use"),
            "expected external bind collision, got {error}"
        );
        assert!(
            failure_output.is_empty(),
            "pre-bind failure must not emit listening or credential success"
        );
    }

    struct FailingWriter;

    impl Write for FailingWriter {
        fn write(&mut self, _buffer: &[u8]) -> io::Result<usize> {
            Err(io::Error::new(
                io::ErrorKind::BrokenPipe,
                "synthetic output failure",
            ))
        }

        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    #[tokio::test]
    #[serial_test::serial]
    async fn output_failure_after_bind_closes_and_settles_the_active_lease() {
        let _test_lock = lock_kv_network_test().await;
        let root = tempfile::tempdir().expect("state root should exist");
        let error = match prepare_and_announce_kv_command(
            KvCommand {
                bind: SocketAddr::from((Ipv4Addr::LOCALHOST, 0)),
                password: Some("test-secret".to_owned()),
                network_state_dir: Some(root.path().to_path_buf()),
                no_disk: true,
                ..KvCommand::default()
            },
            &mut FailingWriter,
        )
        .await
        {
            Ok(_) => panic!("output failure should stop startup"),
            Err(error) => error,
        };
        assert!(
            error.to_string().contains("synthetic output failure"),
            "primary output error should remain visible: {error}"
        );
        let records = nimbus_network::LocalPortLeaseAuthority::open(root.path())
            .expect("authority should reopen")
            .list()
            .expect("records should list");
        assert_eq!(records.len(), 1);
        assert_eq!(records[0].phase(), PortLeasePhase::Released);
    }
}
