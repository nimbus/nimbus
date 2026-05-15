use std::future::Future;
use std::path::PathBuf;
use std::pin::Pin;

use nimbus_core::Error;
use nimbus_machine::{MachineConfigRecord, MachineStateRecord, MachineVolume};

pub type MachineLifecycleFuture<'a> =
    Pin<Box<dyn Future<Output = Result<MachineLifecycleSnapshot, Error>> + Send + 'a>>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineLifecycleSnapshot {
    pub config: MachineConfigRecord,
    pub state: MachineStateRecord,
}

impl MachineLifecycleSnapshot {
    pub fn new(config: MachineConfigRecord, state: MachineStateRecord) -> Self {
        Self { config, state }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineCreateRequest {
    pub name: String,
    pub cpus: Option<u8>,
    pub memory_mib: Option<u32>,
    pub disk_gib: Option<u32>,
    pub image: Option<String>,
    pub ssh_identity: Option<PathBuf>,
    pub ignition_file: Option<PathBuf>,
    pub bootc_native: bool,
    pub efi_store: Option<PathBuf>,
    pub volumes: Vec<MachineVolume>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MachineUpdateRequest {
    pub name: String,
    pub cpus: Option<u8>,
    pub memory_mib: Option<u32>,
    pub disk_gib: Option<u32>,
}

pub trait MachineLifecycleManager: Send + Sync + 'static {
    fn create_machine<'a>(&'a self, request: MachineCreateRequest) -> MachineLifecycleFuture<'a>;

    fn start_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a>;

    fn stop_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a>;

    fn restart_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a> {
        Box::pin(async move {
            self.stop_machine(name).await?;
            self.start_machine(name).await
        })
    }

    fn update_machine<'a>(&'a self, request: MachineUpdateRequest) -> MachineLifecycleFuture<'a>;

    fn delete_machine<'a>(&'a self, name: &'a str) -> MachineLifecycleFuture<'a>;
}
