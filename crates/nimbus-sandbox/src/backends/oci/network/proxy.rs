//! Runner-owned TCP proxies from machine-published ports into container IPs.

use std::net::{IpAddr, Ipv4Addr, Shutdown, SocketAddr, TcpListener, TcpStream};
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::thread;
use std::time::Duration;

use crate::error::{Result, SandboxError};
use crate::spec::SandboxPortBinding;

use super::{MACHINE_PORT_PROXY_ACCEPT_SLEEP, MACHINE_PORT_PROXY_CONNECT_TIMEOUT};

pub(crate) struct MachinePortProxy {
    bind_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
    join: Option<thread::JoinHandle<()>>,
}

impl MachinePortProxy {
    fn start(binding: &SandboxPortBinding, container_ip: Ipv4Addr) -> Result<Self> {
        let bind_addr = machine_port_proxy_bind_addr(binding);
        let target_addr = SocketAddr::new(IpAddr::V4(container_ip), binding.guest_port);
        let listener =
            TcpListener::bind(bind_addr).map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to bind machine port proxy {} -> {} for {}:{}: {error}",
                    bind_addr, target_addr, binding.host_address, binding.host_port
                ),
            })?;
        listener
            .set_nonblocking(true)
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to configure machine port proxy listener {}: {error}",
                    bind_addr
                ),
            })?;

        let shutdown = Arc::new(AtomicBool::new(false));
        let thread_shutdown = Arc::clone(&shutdown);
        let join = thread::Builder::new()
            .name(format!("nimbus-machine-port-{}", binding.host_port))
            .spawn(move || accept_machine_port_proxy(listener, target_addr, thread_shutdown))
            .map_err(|error| SandboxError::OperationFailed {
                message: format!(
                    "failed to spawn machine port proxy {} -> {}: {error}",
                    bind_addr, target_addr
                ),
            })?;

        Ok(Self {
            bind_addr,
            shutdown,
            join: Some(join),
        })
    }

    fn stop(&mut self) {
        self.shutdown.store(true, Ordering::SeqCst);
        let _ = TcpStream::connect_timeout(
            &machine_port_proxy_wake_addr(self.bind_addr),
            Duration::from_millis(100),
        );
        if let Some(join) = self.join.take() {
            let _ = join.join();
        }
    }
}

impl Drop for MachinePortProxy {
    fn drop(&mut self) {
        self.stop();
    }
}

pub(crate) fn start_machine_port_proxies(
    assigned_ips: &[Ipv4Addr],
    port_bindings: &[SandboxPortBinding],
) -> Result<Vec<MachinePortProxy>> {
    if port_bindings.is_empty() {
        return Ok(Vec::new());
    }
    let Some(container_ip) = assigned_ips.first().copied() else {
        return Err(SandboxError::OperationFailed {
            message: "cannot start machine port proxies without a container IPv4 address"
                .to_owned(),
        });
    };
    port_bindings
        .iter()
        .map(|binding| MachinePortProxy::start(binding, container_ip))
        .collect()
}

fn accept_machine_port_proxy(
    listener: TcpListener,
    target_addr: SocketAddr,
    shutdown: Arc<AtomicBool>,
) {
    while !shutdown.load(Ordering::SeqCst) {
        match listener.accept() {
            Ok((stream, _)) => {
                let _ = thread::Builder::new()
                    .name("nimbus-machine-port-connection".to_owned())
                    .spawn(move || proxy_machine_port_connection(stream, target_addr));
            }
            Err(error) if error.kind() == std::io::ErrorKind::WouldBlock => {
                thread::sleep(MACHINE_PORT_PROXY_ACCEPT_SLEEP);
            }
            Err(_) => break,
        }
    }
}

fn proxy_machine_port_connection(mut inbound: TcpStream, target_addr: SocketAddr) {
    let Ok(mut outbound) =
        TcpStream::connect_timeout(&target_addr, MACHINE_PORT_PROXY_CONNECT_TIMEOUT)
    else {
        return;
    };
    let Ok(mut inbound_reader) = inbound.try_clone() else {
        return;
    };
    let Ok(mut outbound_writer) = outbound.try_clone() else {
        return;
    };
    let client_to_target = thread::spawn(move || {
        let _ = std::io::copy(&mut inbound_reader, &mut outbound_writer);
        let _ = outbound_writer.shutdown(Shutdown::Write);
    });
    let target_to_client = thread::spawn(move || {
        let _ = std::io::copy(&mut outbound, &mut inbound);
        let _ = inbound.shutdown(Shutdown::Write);
    });
    let _ = client_to_target.join();
    let _ = target_to_client.join();
}

pub(super) fn machine_port_proxy_bind_addr(binding: &SandboxPortBinding) -> SocketAddr {
    SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), binding.host_port)
}

fn machine_port_proxy_wake_addr(bind_addr: SocketAddr) -> SocketAddr {
    if bind_addr.ip().is_unspecified() {
        SocketAddr::new(IpAddr::V4(Ipv4Addr::LOCALHOST), bind_addr.port())
    } else {
        bind_addr
    }
}
