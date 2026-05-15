use std::path::Path;

use nimbus::Error;
use nimbus_server::LocalServerPaths;
use serde::Serialize;

use crate::local_server_client::LocalServerHttpClient;

use super::command::{
    MachineInitCommand, MachineRmCommand, MachineSetCommand, MachineStartCommand,
    MachineStopCommand, MachineSubcommand,
};
use super::handlers::emit_machine_stdout;
use super::record::{MachineRootLayout, MachineVolume};
use super::render::{MachineCommandResult, render_machine_action_view};

pub(super) async fn try_run_lifecycle_command_via_live_server(
    command: &MachineSubcommand,
    roots: &MachineRootLayout,
) -> Result<bool, Error> {
    let paths = LocalServerPaths::resolve_for_current_platform().map_err(|error| {
        Error::Internal(format!("failed to resolve local server paths: {error}"))
    })?;
    try_run_lifecycle_command_via_live_server_with_paths(
        command,
        roots,
        &paths,
        reqwest::Client::new(),
    )
    .await
}

async fn try_run_lifecycle_command_via_live_server_with_paths(
    command: &MachineSubcommand,
    roots: &MachineRootLayout,
    paths: &LocalServerPaths,
    http_client: reqwest::Client,
) -> Result<bool, Error> {
    if !is_lifecycle_command(command) {
        return Ok(false);
    }
    let Some(client) = LocalServerHttpClient::discover(paths, http_client)? else {
        return Ok(false);
    };
    run_lifecycle_command_on_live_server(command, roots, &client).await?;
    Ok(true)
}

fn is_lifecycle_command(command: &MachineSubcommand) -> bool {
    matches!(
        command,
        MachineSubcommand::Init(_)
            | MachineSubcommand::Start(_)
            | MachineSubcommand::Stop(_)
            | MachineSubcommand::Set(_)
            | MachineSubcommand::Rm(_)
    )
}

async fn run_lifecycle_command_on_live_server(
    command: &MachineSubcommand,
    roots: &MachineRootLayout,
    client: &LocalServerHttpClient,
) -> Result<(), Error> {
    match command {
        MachineSubcommand::Init(command) => run_init_on_live_server(command, roots, client).await,
        MachineSubcommand::Start(command) => run_start_on_live_server(command, roots, client).await,
        MachineSubcommand::Stop(command) => run_stop_on_live_server(command, roots, client).await,
        MachineSubcommand::Set(command) => run_set_on_live_server(command, roots, client).await,
        MachineSubcommand::Rm(command) => run_rm_on_live_server(command, roots, client).await,
        _ => Ok(()),
    }
}

async fn run_init_on_live_server(
    command: &MachineInitCommand,
    roots: &MachineRootLayout,
    client: &LocalServerHttpClient,
) -> Result<(), Error> {
    let name = command.name();
    let body = MachineCreateBody::from_init(command);
    let _: serde_json::Value = client
        .post_json(&format!("/api/machines/{name}/create"), &body)
        .await?;
    if command.now {
        let _: serde_json::Value = client
            .post_empty(&format!("/api/machines/{name}/start"))
            .await?;
    }
    render_lifecycle_action(
        roots,
        name,
        if command.now {
            MachineCommandResult::InitializedAndStarted
        } else {
            MachineCommandResult::Initialized
        },
    )
}

async fn run_start_on_live_server(
    command: &MachineStartCommand,
    roots: &MachineRootLayout,
    client: &LocalServerHttpClient,
) -> Result<(), Error> {
    let name = command.name();
    let created = !roots.paths(name).config_path.exists();
    if command.has_create_overrides() {
        let init = command.clone().into_init_command();
        let body = MachineCreateBody::from_init(&init);
        let _: serde_json::Value = client
            .post_json(&format!("/api/machines/{name}/create"), &body)
            .await?;
    }
    let _: serde_json::Value = client
        .post_empty(&format!("/api/machines/{name}/start"))
        .await?;
    render_lifecycle_action(
        roots,
        name,
        if created {
            MachineCommandResult::InitializedAndStarted
        } else {
            MachineCommandResult::Started
        },
    )
}

async fn run_stop_on_live_server(
    command: &MachineStopCommand,
    roots: &MachineRootLayout,
    client: &LocalServerHttpClient,
) -> Result<(), Error> {
    let name = command.name();
    let _: serde_json::Value = client
        .post_empty(&format!("/api/machines/{name}/stop"))
        .await?;
    render_lifecycle_action(roots, name, MachineCommandResult::Stopped)
}

async fn run_set_on_live_server(
    command: &MachineSetCommand,
    roots: &MachineRootLayout,
    client: &LocalServerHttpClient,
) -> Result<(), Error> {
    let name = command.name();
    let body = MachineUpdateBody {
        cpus: command.cpus,
        memory_mib: command.memory_mib,
        disk_gib: command.disk_gib,
    };
    let _: serde_json::Value = client
        .patch_json(&format!("/api/machines/{name}"), &body)
        .await?;
    render_lifecycle_action(roots, name, MachineCommandResult::Updated)
}

async fn run_rm_on_live_server(
    command: &MachineRmCommand,
    roots: &MachineRootLayout,
    client: &LocalServerHttpClient,
) -> Result<(), Error> {
    let name = command.name();
    let _: serde_json::Value = client
        .delete_empty(&format!("/api/machines/{name}"))
        .await?;
    render_lifecycle_action(roots, name, MachineCommandResult::Removed)
}

fn render_lifecycle_action(
    roots: &MachineRootLayout,
    name: &str,
    result: MachineCommandResult,
) -> Result<(), Error> {
    let paths = roots.paths(name);
    emit_machine_stdout(&render_machine_action_view(result, &paths)?)
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineCreateBody<'a> {
    cpus: Option<u8>,
    #[serde(rename = "memoryMiB")]
    memory_mib: Option<u32>,
    #[serde(rename = "diskGiB")]
    disk_gib: Option<u32>,
    image: Option<&'a str>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ssh_identity: Option<&'a Path>,
    #[serde(skip_serializing_if = "Option::is_none")]
    ignition_file: Option<&'a Path>,
    bootc_native: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    efi_store: Option<&'a Path>,
    volumes: &'a [MachineVolume],
}

impl<'a> MachineCreateBody<'a> {
    fn from_init(command: &'a MachineInitCommand) -> Self {
        Self {
            cpus: Some(command.cpus),
            memory_mib: Some(command.memory_mib),
            disk_gib: Some(command.disk_gib),
            image: Some(command.image.as_str()),
            ssh_identity: command.ssh_identity.as_deref(),
            ignition_file: command.ignition_file.as_deref(),
            bootc_native: command.bootc_native,
            efi_store: command.efi_store.as_deref(),
            volumes: command.volumes.as_slice(),
        }
    }
}

#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
struct MachineUpdateBody {
    cpus: Option<u8>,
    #[serde(rename = "memoryMiB")]
    memory_mib: Option<u32>,
    #[serde(rename = "diskGiB")]
    disk_gib: Option<u32>,
}

#[cfg(test)]
mod tests {
    use std::net::Ipv4Addr;
    use std::sync::{Arc, Mutex};
    use std::time::Duration;

    use nimbus::Service;
    use nimbus_server::{
        LocalServerSecurityState, MachineCreateRequest, MachineLifecycleFuture,
        MachineLifecycleManager, MachineLifecycleSnapshot, MachineUpdateRequest, ServeOptions,
        ServerDiscoveryLease, load_or_create_local_admin_token, serve_with_options,
    };
    use nimbus_testing::wait_for_condition;
    use tempfile::tempdir;

    use super::*;

    #[derive(Clone)]
    struct StubMachineLifecycleManager {
        roots: MachineRootLayout,
        calls: Arc<Mutex<Vec<String>>>,
    }

    impl StubMachineLifecycleManager {
        fn new(roots: MachineRootLayout) -> Self {
            Self {
                roots,
                calls: Arc::new(Mutex::new(Vec::new())),
            }
        }

        fn calls(&self) -> Vec<String> {
            self.calls
                .lock()
                .expect("calls lock should not poison")
                .clone()
        }
    }

    impl MachineLifecycleManager for StubMachineLifecycleManager {
        fn create_machine<'a>(
            &'a self,
            request: MachineCreateRequest,
        ) -> MachineLifecycleFuture<'a> {
            let roots = self.roots.clone();
            let calls = self.calls.clone();
            Box::pin(async move {
                calls
                    .lock()
                    .expect("calls lock should not poison")
                    .push(format!("create:{}:{}", request.name, request.bootc_native));
                Ok(snapshot_for(
                    &request.name,
                    roots,
                    nimbus_machine::MachineLifecycle::Stopped,
                ))
            })
        }

        fn start_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
            let roots = self.roots.clone();
            let calls = self.calls.clone();
            let name = name.to_owned();
            Box::pin(async move {
                calls
                    .lock()
                    .expect("calls lock should not poison")
                    .push(format!("start:{name}"));
                Ok(snapshot_for(
                    &name,
                    roots,
                    nimbus_machine::MachineLifecycle::Running,
                ))
            })
        }

        fn stop_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
            let roots = self.roots.clone();
            let calls = self.calls.clone();
            let name = name.to_owned();
            Box::pin(async move {
                calls
                    .lock()
                    .expect("calls lock should not poison")
                    .push(format!("stop:{name}"));
                Ok(snapshot_for(
                    &name,
                    roots,
                    nimbus_machine::MachineLifecycle::Stopped,
                ))
            })
        }

        fn update_machine<'a>(
            &'a self,
            request: MachineUpdateRequest,
        ) -> MachineLifecycleFuture<'a> {
            let roots = self.roots.clone();
            let calls = self.calls.clone();
            Box::pin(async move {
                calls
                    .lock()
                    .expect("calls lock should not poison")
                    .push(format!("update:{}", request.name));
                Ok(snapshot_for(
                    &request.name,
                    roots,
                    nimbus_machine::MachineLifecycle::Stopped,
                ))
            })
        }

        fn delete_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
            let roots = self.roots.clone();
            let calls = self.calls.clone();
            let name = name.to_owned();
            Box::pin(async move {
                calls
                    .lock()
                    .expect("calls lock should not poison")
                    .push(format!("delete:{name}"));
                Ok(snapshot_for(
                    &name,
                    roots,
                    nimbus_machine::MachineLifecycle::Stopped,
                ))
            })
        }
    }

    #[tokio::test]
    async fn lifecycle_command_prefers_running_local_server() {
        let temp = tempdir().expect("tempdir should build");
        let local_paths = sample_paths(temp.path());
        let token =
            load_or_create_local_admin_token(&local_paths).expect("local admin token should exist");
        let roots = MachineRootLayout::new(
            temp.path().join("machine-config"),
            temp.path().join("machine-state"),
            temp.path().join("run"),
        );
        let manager = StubMachineLifecycleManager::new(roots.clone());
        let service =
            Arc::new(Service::new(temp.path().join("data")).expect("service should create"));
        let listener = tokio::net::TcpListener::bind((Ipv4Addr::LOCALHOST, 0))
            .await
            .expect("listener should bind");
        let address = listener
            .local_addr()
            .expect("listener address should resolve");
        let _lease = ServerDiscoveryLease::acquire(&local_paths, address)
            .expect("server discovery should be recorded");
        let server_task = tokio::spawn(serve_with_options(
            listener,
            service,
            ServeOptions::default()
                .with_local_server_security(Arc::new(LocalServerSecurityState::new(
                    local_paths.clone(),
                    token,
                )))
                .with_machine_lifecycle_manager(Arc::new(manager.clone())),
        ));
        wait_for_server(&address.to_string()).await;

        let command = MachineSubcommand::Start(MachineStartCommand {
            cpus: Some(6),
            memory_mib: Some(8192),
            disk_gib: Some(80),
            bootc_native: true,
            name: Some("team-a".to_owned()),
            ..MachineStartCommand::default()
        });
        let handled = try_run_lifecycle_command_via_live_server_with_paths(
            &command,
            &roots,
            &local_paths,
            reqwest::Client::new(),
        )
        .await
        .expect("live server machine command should succeed");

        assert!(handled);
        assert_eq!(manager.calls(), vec!["create:team-a:true", "start:team-a"]);

        server_task.abort();
        let _ = server_task.await;
    }

    #[tokio::test]
    async fn lifecycle_command_falls_back_when_no_server_is_running() {
        let temp = tempdir().expect("tempdir should build");
        let local_paths = sample_paths(temp.path());
        let roots = MachineRootLayout::new(
            temp.path().join("machine-config"),
            temp.path().join("machine-state"),
            temp.path().join("run"),
        );
        let command = MachineSubcommand::Stop(MachineStopCommand {
            name: Some("team-a".to_owned()),
        });

        let handled = try_run_lifecycle_command_via_live_server_with_paths(
            &command,
            &roots,
            &local_paths,
            reqwest::Client::new(),
        )
        .await
        .expect("missing local server should not fail");

        assert!(!handled);
    }

    fn sample_paths(root: &std::path::Path) -> LocalServerPaths {
        LocalServerPaths {
            auth_token_path: root.join("auth").join("token"),
            server_discovery_path: root.join("run").join("server.json"),
            audit_log_path: root.join("logs").join("access.jsonl"),
        }
    }

    async fn wait_for_server(address: &str) {
        let client = reqwest::Client::new();
        wait_for_condition(
            "local machine lifecycle server should answer health checks",
            Duration::from_secs(5),
            Duration::from_millis(50),
            || {
                let client = client.clone();
                let address = address.to_owned();
                async move {
                    client
                        .get(format!("http://{address}/health"))
                        .send()
                        .await
                        .map(|response| response.status().is_success())
                        .unwrap_or(false)
                }
            },
        )
        .await;
    }

    fn snapshot_for(
        name: &str,
        roots: MachineRootLayout,
        lifecycle: nimbus_machine::MachineLifecycle,
    ) -> MachineLifecycleSnapshot {
        let config = nimbus_machine::MachineConfigRecord {
            version: nimbus_machine::CURRENT_MACHINE_CONFIG_VERSION,
            name: name.to_owned(),
            provider: nimbus_machine::MachineProvider::Krunkit,
            guest: nimbus_machine::MachineGuestConfig {
                image_source: nimbus_machine::MachineImageSource::OciReference {
                    reference: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_owned(),
                },
                provisioning: nimbus_machine::MachineGuestProvisioning::BootcMachineConfig,
                ssh_user: "nimbus".to_owned(),
                ssh_identity_path: None,
                ignition_file_path: None,
                efi_variable_store_path: None,
            },
            resources: nimbus_machine::MachineResources {
                cpus: 4,
                memory_mib: 4096,
                disk_gib: 50,
            },
            volumes: Vec::new(),
            roots: roots.clone(),
        };
        let paths = roots.paths(name);
        let state = nimbus_machine::MachineStateRecord {
            version: nimbus_machine::CURRENT_MACHINE_STATE_VERSION,
            lifecycle,
            manager: if matches!(lifecycle, nimbus_machine::MachineLifecycle::Running) {
                nimbus_machine::MachineManagerState::Ready
            } else {
                nimbus_machine::MachineManagerState::HelpersResolved
            },
            runtime: Some(nimbus_machine::MachineRuntimeState {
                helper_binaries: nimbus_machine::MachineHelperBinaryPaths {
                    krunkit: paths.runtime_dir.join("krunkit"),
                    gvproxy: paths.runtime_dir.join("gvproxy"),
                },
                image_path: paths.materialized_image_path,
                efi_variable_store_path: paths.efi_variable_store_path,
                machine_image_source: "docker://ghcr.io/nimbus/machine-os:v0.1.31".to_owned(),
                ssh_port: 10022,
                rest_uri: format!("unix://{}", paths.api_socket_path.display()),
                ready_vsock_port: 1025,
            }),
            last_error: None,
        };
        MachineLifecycleSnapshot::new(config, state)
    }
}
