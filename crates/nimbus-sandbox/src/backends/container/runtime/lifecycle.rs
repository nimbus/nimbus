use super::support::*;

use std::io::{Read, Write};
use std::net::{Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
    mpsc,
};
use std::thread;
use std::time::Duration;

use tempfile::TempDir;

use crate::backends::oci::command::CommandSpec;
use crate::backends::oci::network::{
    FixedOciEgressPinProvider, MachinePortPreparationReleaseAuthority, OciEgressPinProvider,
    OciSegmentAllocator, RecordingSegmentAllocator, SegmentAllocatorOperation,
    default_network_attachment_id, panicking_machine_port_proxy_for_test,
};
use nimbus_network::{
    NetworkProviderHandle, NetworkProviderId, NetworkReservationClaim, PortLeasePhase,
};

#[path = "tests/absent_runtime_projection.rs"]
mod absent_runtime_projection;
#[path = "tests/attachment_readiness.rs"]
mod attachment_readiness;
#[path = "tests/creator_persistence.rs"]
mod creator_persistence;
#[path = "tests/execute_inspection.rs"]
mod execute_inspection;
mod launch_cleanup;
#[path = "tests/machine_forwarded_readiness.rs"]
mod machine_forwarded_readiness;
#[path = "tests/machine_port_batch_recovery.rs"]
mod machine_port_batch_recovery;
#[path = "tests/machine_proxy_activation.rs"]
mod machine_proxy_activation;
#[path = "tests/machine_proxy_concurrency.rs"]
mod machine_proxy_concurrency;
#[path = "tests/machine_proxy_recovery.rs"]
mod machine_proxy_recovery;
#[path = "tests/network_configuration.rs"]
mod network_configuration;
#[path = "tests/plan_only_inspection.rs"]
mod plan_only_inspection;
#[path = "tests/provider_cleanup.rs"]
mod provider_cleanup;
#[path = "tests/restart_policy.rs"]
mod restart_policy;
#[path = "tests/runner_recovery.rs"]
mod runner_recovery;
#[path = "tests/runner_reliability.rs"]
mod runner_reliability;
#[path = "tests/status_callbacks.rs"]
mod status_callbacks;
#[path = "tests/terminal_finality.rs"]
mod terminal_finality;

fn unused_loopback_port() -> u16 {
    TcpListener::bind("127.0.0.1:0")
        .expect("ephemeral loopback listener should bind")
        .local_addr()
        .expect("ephemeral listener should expose address")
        .port()
}
