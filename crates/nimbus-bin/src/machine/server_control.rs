use std::sync::Arc;

use nimbus::Error;
use nimbus_machine::MachineRootLayout;
use nimbus_server::{
    MachineCreateRequest, MachineLifecycleFuture, MachineLifecycleManager,
    MachineLifecycleSnapshot, MachineUpdateRequest,
};

use super::command::{MachineInitCommand, MachineSetCommand};
use super::handlers::{
    create_machine_with_layout, delete_machine_with_layout, restart_machine_with_layout,
    start_machine_with_layout, stop_machine_with_layout, update_machine_with_layout,
};

pub(crate) fn host_machine_lifecycle_manager() -> Result<Arc<dyn MachineLifecycleManager>, Error> {
    Ok(Arc::new(HostMachineLifecycleManager {
        roots: MachineRootLayout::resolve()?,
    }))
}

struct HostMachineLifecycleManager {
    roots: MachineRootLayout,
}

impl MachineLifecycleManager for HostMachineLifecycleManager {
    fn create_machine<'a>(&'a self, request: MachineCreateRequest) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        Box::pin(async move {
            run_machine_lifecycle_blocking(move || {
                create_machine_with_layout(
                    MachineInitCommand {
                        cpus: request.cpus.unwrap_or(super::DEFAULT_MACHINE_CPUS),
                        memory_mib: request
                            .memory_mib
                            .unwrap_or(super::DEFAULT_MACHINE_MEMORY_MIB),
                        disk_gib: request.disk_gib.unwrap_or(super::DEFAULT_MACHINE_DISK_GIB),
                        image: request.image.unwrap_or_else(super::default_machine_image),
                        ssh_identity: request.ssh_identity,
                        ignition_file: request.ignition_file,
                        bootc_native: request.bootc_native,
                        efi_store: request.efi_store,
                        volumes: request.volumes,
                        now: false,
                        name: Some(request.name),
                    },
                    &roots,
                )
            })
            .await
        })
    }

    fn start_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let name = name.to_owned();
        Box::pin(async move {
            run_machine_lifecycle_blocking(move || start_machine_with_layout(&name, &roots)).await
        })
    }

    fn stop_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let name = name.to_owned();
        Box::pin(async move {
            run_machine_lifecycle_blocking(move || {
                stop_machine_with_layout(&name, &roots)
                    .map(|(_paths, config, state)| (config, state))
            })
            .await
        })
    }

    fn restart_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let name = name.to_owned();
        Box::pin(async move {
            run_machine_lifecycle_blocking(move || restart_machine_with_layout(&name, &roots)).await
        })
    }

    fn update_machine<'a>(&'a self, request: MachineUpdateRequest) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        Box::pin(async move {
            run_machine_lifecycle_blocking(move || {
                update_machine_with_layout(
                    MachineSetCommand {
                        cpus: request.cpus,
                        memory_mib: request.memory_mib,
                        disk_gib: request.disk_gib,
                        name: Some(request.name),
                    },
                    &roots,
                )
            })
            .await
        })
    }

    fn delete_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        let roots = self.roots.clone();
        let name = name.to_owned();
        Box::pin(async move {
            run_machine_lifecycle_blocking(move || delete_machine_with_layout(&name, &roots)).await
        })
    }
}

async fn run_machine_lifecycle_blocking(
    operation: impl FnOnce() -> Result<
        (
            nimbus_machine::MachineConfigRecord,
            nimbus_machine::MachineStateRecord,
        ),
        Error,
    > + Send
    + 'static,
) -> Result<MachineLifecycleSnapshot, Error> {
    let (config, state) = tokio::task::spawn_blocking(operation)
        .await
        .map_err(|error| Error::Internal(format!("machine lifecycle worker failed: {error}")))??;
    Ok(MachineLifecycleSnapshot::new(config, state))
}
