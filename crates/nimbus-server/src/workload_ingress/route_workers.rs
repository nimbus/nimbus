//! Process-bound TCP route and transitive worker ownership.
//!
//! One route owns its listener worker, every connection worker, every
//! bidirectional-copy worker, and the active lease that fences those effects.
//! Terminal callers can therefore prove a transitive join even if the
//! listener worker exits or panics before it performs its own cleanup.

use std::io::{self, Read, Write};
use std::net::{Shutdown, SocketAddr};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::thread::{self, JoinHandle};

use super::{ACCEPT_RETRY_DELAY, CONNECTION_IO_TIMEOUT, ExpectedRoute, UPSTREAM_CONNECT_TIMEOUT};
use crate::listener_lease::{ActiveServerListenerLease, RestartStoppingServerListener};

pub(super) struct RunningIngressRoute {
    pub(super) expected: ExpectedRoute,
    pub(super) bound_addr: SocketAddr,
    pub(super) stop: Arc<AtomicBool>,
    pub(super) failed: Arc<AtomicBool>,
    #[cfg(test)]
    pub(super) active_connections: Arc<AtomicUsize>,
    #[cfg(test)]
    pub(super) peak_connections: Arc<AtomicUsize>,
    #[cfg(test)]
    pub(super) rejected_connections: Arc<AtomicUsize>,
    pub(super) worker: Option<JoinHandle<()>>,
    pub(super) connections: Arc<Mutex<Vec<JoinHandle<()>>>>,
    pub(super) lease: Option<ActiveServerListenerLease>,
    #[cfg(test)]
    pub(super) final_join_failure: Arc<AtomicBool>,
}

impl RunningIngressRoute {
    pub(super) fn start(
        expected: ExpectedRoute,
        listener: crate::PreboundServerListener,
        max_active_connections: usize,
    ) -> io::Result<Self> {
        if max_active_connections == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "workload ingress connection limit must be greater than zero",
            ));
        }
        let bound_addr = listener.local_addr()?;
        let (listener, lease, _) = listener.into_std_parts();
        if let Err(error) = listener.set_nonblocking(true) {
            drop(listener);
            return match lease.settle_after_confirmed_local_close() {
                Ok(()) => Err(error),
                Err(cleanup) => Err(io::Error::new(
                    error.kind(),
                    format!("{error}; failed to settle listener: {cleanup}"),
                )),
            };
        }
        let stop = Arc::new(AtomicBool::new(false));
        let failed = Arc::new(AtomicBool::new(false));
        let active_connections = Arc::new(AtomicUsize::new(0));
        let peak_connections = Arc::new(AtomicUsize::new(0));
        let rejected_connections = Arc::new(AtomicUsize::new(0));
        let worker_stop = Arc::clone(&stop);
        let worker_failed = Arc::clone(&failed);
        let worker_active = Arc::clone(&active_connections);
        let worker_peak = Arc::clone(&peak_connections);
        let worker_rejected = Arc::clone(&rejected_connections);
        let connections = Arc::new(Mutex::new(Vec::new()));
        let worker_connections = Arc::clone(&connections);
        let upstream = expected.upstream;
        let name = format!("nimbus-ingress-{}", expected.listener_id);
        let worker = thread::Builder::new().name(name).spawn(move || {
            while !worker_stop.load(Ordering::Acquire) {
                if reap_finished_connections(&worker_connections) {
                    worker_failed.store(true, Ordering::Release);
                }
                match listener.accept() {
                    Ok((stream, _)) => {
                        let Some(permit) = ConnectionPermit::try_acquire(
                            Arc::clone(&worker_active),
                            Arc::clone(&worker_peak),
                            max_active_connections,
                        ) else {
                            worker_rejected.fetch_add(1, Ordering::AcqRel);
                            drop(stream);
                            continue;
                        };
                        let connection_stop = Arc::clone(&worker_stop);
                        let connection_registry = Arc::clone(&worker_connections);
                        match thread::Builder::new()
                            .name("nimbus-ingress-connection".to_owned())
                            .spawn(move || {
                                proxy_connection(
                                    stream,
                                    upstream,
                                    &connection_stop,
                                    &connection_registry,
                                    permit,
                                );
                            }) {
                            Ok(connection) => {
                                register_connection_worker(&worker_connections, connection);
                            }
                            Err(_) => {
                                worker_failed.store(true, Ordering::Release);
                                break;
                            }
                        }
                    }
                    Err(error) if error.kind() == io::ErrorKind::WouldBlock => {
                        thread::sleep(ACCEPT_RETRY_DELAY);
                    }
                    Err(_) => {
                        worker_failed.store(true, Ordering::Release);
                        break;
                    }
                }
            }
            worker_stop.store(true, Ordering::Release);
            if join_all_connection_workers(&worker_connections).is_err() {
                worker_failed.store(true, Ordering::Release);
            }
        })?;
        Ok(Self {
            expected,
            bound_addr,
            stop,
            failed,
            #[cfg(test)]
            active_connections,
            #[cfg(test)]
            peak_connections,
            #[cfg(test)]
            rejected_connections,
            worker: Some(worker),
            connections,
            lease: Some(lease),
            #[cfg(test)]
            final_join_failure: Arc::new(AtomicBool::new(false)),
        })
    }

    pub(super) fn is_healthy(&self) -> bool {
        !self.failed.load(Ordering::Acquire)
            && self
                .worker
                .as_ref()
                .is_some_and(|worker| !worker.is_finished())
            && self.lease.is_some()
    }

    pub(super) fn take_for_restart(&mut self) -> Option<RestartStoppingServerListener> {
        let lease = self.lease.take()?;
        let worker = self.worker.take()?;
        let stop = Arc::clone(&self.stop);
        let connections = Arc::clone(&self.connections);
        Some(RestartStoppingServerListener::new(lease, move || {
            cancel_and_join_ingress_workers(
                &stop,
                worker,
                &connections,
                "workload ingress listener worker panicked during restart stop",
            )
        }))
    }

    fn stop_and_settle(&mut self) {
        self.stop.store(true, Ordering::Release);
        let listener_result = self.worker.take().map_or(Ok(()), |worker| {
            worker.join().map_err(|_| {
                io::Error::other("workload ingress listener worker panicked during drop")
            })
        });
        let connections_result = join_all_connection_workers(&self.connections);
        match listener_result.and(connections_result) {
            Ok(()) => {
                if let Some(lease) = self.lease.take()
                    && let Err(error) = lease.settle_after_confirmed_local_close()
                {
                    tracing::error!(%error, "failed to settle workload ingress listener");
                }
            }
            Err(error) => {
                tracing::error!(
                    %error,
                    "workload ingress listener drop could not prove transitive worker stop"
                );
                drop(self.lease.take());
            }
        }
    }
}

struct ConnectionPermit {
    active: Arc<AtomicUsize>,
}

impl ConnectionPermit {
    fn try_acquire(
        active: Arc<AtomicUsize>,
        peak: Arc<AtomicUsize>,
        limit: usize,
    ) -> Option<Arc<Self>> {
        let observed = active
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |count| {
                (count < limit).then_some(count + 1)
            })
            .ok()?;
        peak.fetch_max(observed + 1, Ordering::AcqRel);
        Some(Arc::new(Self { active }))
    }
}

impl Drop for ConnectionPermit {
    fn drop(&mut self) {
        self.active.fetch_sub(1, Ordering::AcqRel);
    }
}

fn reap_finished_connections(connections: &Mutex<Vec<JoinHandle<()>>>) -> bool {
    let mut connections = match connections.lock() {
        Ok(connections) => connections,
        Err(poisoned) => poisoned.into_inner(),
    };
    let mut panicked = false;
    let mut index = 0;
    while index < connections.len() {
        if connections[index].is_finished() {
            panicked |= connections.swap_remove(index).join().is_err();
        } else {
            index += 1;
        }
    }
    panicked
}

fn register_connection_worker(connections: &Mutex<Vec<JoinHandle<()>>>, worker: JoinHandle<()>) {
    match connections.lock() {
        Ok(mut connections) => connections.push(worker),
        Err(poisoned) => poisoned.into_inner().push(worker),
    }
}

pub(super) fn join_all_connection_workers(
    connections: &Mutex<Vec<JoinHandle<()>>>,
) -> io::Result<()> {
    let mut panicked = false;
    loop {
        let workers = match connections.lock() {
            Ok(mut connections) => std::mem::take(&mut *connections),
            Err(poisoned) => {
                panicked = true;
                let mut connections = poisoned.into_inner();
                std::mem::take(&mut *connections)
            }
        };
        if workers.is_empty() {
            break;
        }
        for worker in workers {
            panicked |= worker.join().is_err();
        }
    }
    if panicked {
        Err(io::Error::other(
            "one or more workload ingress connection workers panicked",
        ))
    } else {
        Ok(())
    }
}

pub(super) fn cancel_and_join_ingress_workers(
    stop: &AtomicBool,
    listener: JoinHandle<()>,
    connections: &Mutex<Vec<JoinHandle<()>>>,
    listener_panic_message: &'static str,
) -> io::Result<()> {
    stop.store(true, Ordering::Release);
    let listener_result = listener
        .join()
        .map_err(|_| io::Error::other(listener_panic_message));
    let connections_result = join_all_connection_workers(connections);
    listener_result.and(connections_result)
}

impl Drop for RunningIngressRoute {
    fn drop(&mut self) {
        self.stop_and_settle();
    }
}

fn proxy_connection(
    inbound: std::net::TcpStream,
    upstream: SocketAddr,
    stop: &Arc<AtomicBool>,
    connections: &Mutex<Vec<JoinHandle<()>>>,
    permit: Arc<ConnectionPermit>,
) {
    let Ok(outbound) = std::net::TcpStream::connect_timeout(&upstream, UPSTREAM_CONNECT_TIMEOUT)
    else {
        return;
    };
    for stream in [&inbound, &outbound] {
        let _ = stream.set_read_timeout(Some(CONNECTION_IO_TIMEOUT));
        let _ = stream.set_write_timeout(Some(CONNECTION_IO_TIMEOUT));
    }
    let (Ok(inbound_read), Ok(outbound_write)) = (inbound.try_clone(), outbound.try_clone()) else {
        return;
    };
    let request_stop = Arc::clone(stop);
    let forward_permit = Arc::clone(&permit);
    let forward = thread::spawn(move || {
        let _permit = forward_permit;
        copy_until_stopped(inbound_read, outbound_write, &request_stop);
    });
    register_connection_worker(connections, forward);
    let _permit = permit;
    copy_until_stopped(outbound, inbound, stop);
}

fn copy_until_stopped(
    mut reader: std::net::TcpStream,
    mut writer: std::net::TcpStream,
    stop: &AtomicBool,
) {
    let mut buffer = [0_u8; 16 * 1024];
    while !stop.load(Ordering::Acquire) {
        match reader.read(&mut buffer) {
            Ok(0) => break,
            Ok(count) => {
                if !write_all_until_stopped(&mut writer, &buffer[..count], stop) {
                    break;
                }
            }
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => break,
        }
    }
    let _ = writer.shutdown(Shutdown::Write);
}

fn write_all_until_stopped(
    writer: &mut std::net::TcpStream,
    bytes: &[u8],
    stop: &AtomicBool,
) -> bool {
    let mut offset = 0;
    while offset < bytes.len() && !stop.load(Ordering::Acquire) {
        match writer.write(&bytes[offset..]) {
            Ok(0) => return false,
            Ok(count) => offset += count,
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::WouldBlock | io::ErrorKind::TimedOut
                ) => {}
            Err(_) => return false,
        }
    }
    offset == bytes.len()
}
