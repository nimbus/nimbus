use std::future::Future;
use std::io;

use nimbus_server::ServerShutdownHandle;

#[cfg(unix)]
pub(super) struct ProcessShutdownSignals {
    interrupt: tokio::signal::unix::Signal,
    terminate: tokio::signal::unix::Signal,
}

#[cfg(unix)]
impl ProcessShutdownSignals {
    pub(super) fn install() -> io::Result<Self> {
        use tokio::signal::unix::{SignalKind, signal};

        Ok(Self {
            interrupt: signal(SignalKind::interrupt())?,
            terminate: signal(SignalKind::terminate())?,
        })
    }
}

#[cfg(unix)]
impl ShutdownSignalSource for ProcessShutdownSignals {
    async fn wait(&mut self) -> &'static str {
        tokio::select! {
            _ = self.interrupt.recv() => "SIGINT",
            _ = self.terminate.recv() => "SIGTERM",
        }
    }
}

#[cfg(windows)]
pub(super) struct ProcessShutdownSignals {
    ctrl_break: tokio::signal::windows::CtrlBreak,
    ctrl_c: tokio::signal::windows::CtrlC,
}

#[cfg(windows)]
impl ProcessShutdownSignals {
    pub(super) fn install() -> io::Result<Self> {
        Ok(Self {
            ctrl_break: tokio::signal::windows::ctrl_break()?,
            ctrl_c: tokio::signal::windows::ctrl_c()?,
        })
    }
}

#[cfg(windows)]
impl ShutdownSignalSource for ProcessShutdownSignals {
    async fn wait(&mut self) -> &'static str {
        tokio::select! {
            _ = self.ctrl_break.recv() => "CTRL_BREAK",
            _ = self.ctrl_c.recv() => "CTRL_C",
        }
    }
}

#[cfg(not(any(unix, windows)))]
pub(super) struct ProcessShutdownSignals;

#[cfg(not(any(unix, windows)))]
impl ProcessShutdownSignals {
    pub(super) fn install() -> io::Result<Self> {
        Ok(Self)
    }
}

#[cfg(not(any(unix, windows)))]
impl ShutdownSignalSource for ProcessShutdownSignals {
    async fn wait(&mut self) -> &'static str {
        let _ = tokio::signal::ctrl_c().await;
        "interrupt"
    }
}

trait ShutdownSignalSource {
    async fn wait(&mut self) -> &'static str;
}

pub(super) async fn serve_until_shutdown<Server>(
    server: Server,
    signals: ProcessShutdownSignals,
    shutdown: ServerShutdownHandle,
) -> io::Result<()>
where
    Server: Future<Output = io::Result<()>>,
{
    serve_until_shutdown_with_signals(server, signals, shutdown).await
}

async fn serve_until_shutdown_with_signals<Server, Signals>(
    server: Server,
    mut signals: Signals,
    shutdown: ServerShutdownHandle,
) -> io::Result<()>
where
    Server: Future<Output = io::Result<()>>,
    Signals: ShutdownSignalSource,
{
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result,
        signal = signals.wait() => {
            tracing::info!(signal, "process shutdown requested");
            shutdown.request_shutdown();
            tokio::select! {
                result = &mut server => result,
                second_signal = signals.wait() => {
                    tracing::warn!(signal, second_signal, "forcing stalled process shutdown");
                    Err(io::Error::new(
                        io::ErrorKind::Interrupted,
                        format!(
                            "received {second_signal} while graceful shutdown after {signal} was still pending"
                        ),
                    ))
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use std::sync::Arc;
    use std::time::Duration;

    use super::*;

    struct TestSignals {
        receiver: tokio::sync::mpsc::UnboundedReceiver<&'static str>,
    }

    impl ShutdownSignalSource for TestSignals {
        async fn wait(&mut self) -> &'static str {
            self.receiver
                .recv()
                .await
                .expect("test signal sender should remain live")
        }
    }

    #[tokio::test]
    async fn second_signal_interrupts_a_stalled_lifecycle_cleanup() {
        let temp = tempfile::tempdir().expect("temporary root should build");
        let engine = Arc::new(
            nimbus_engine::Engine::new(temp.path().join("data"))
                .expect("test engine should initialize"),
        );
        let options = nimbus_server::ServeOptions::reconstruct_direct(Arc::clone(&engine))
            .expect("test server network authority should reconstruct once");
        let shutdown = options.shutdown_handle();
        drop(options);
        let (signal_tx, signal_rx) = tokio::sync::mpsc::unbounded_channel();
        let task = tokio::spawn(serve_until_shutdown_with_signals(
            std::future::pending::<io::Result<()>>(),
            TestSignals {
                receiver: signal_rx,
            },
            shutdown,
        ));

        signal_tx
            .send("SIGTERM")
            .expect("first test signal should send");
        tokio::time::sleep(Duration::from_millis(20)).await;
        assert!(
            !task.is_finished(),
            "the first signal should leave lifecycle cleanup in progress"
        );
        signal_tx
            .send("SIGINT")
            .expect("second test signal should send");
        let error = tokio::time::timeout(Duration::from_secs(1), task)
            .await
            .expect("second signal should stop the stalled drain")
            .expect("shutdown task should join")
            .expect_err("forced shutdown should report interruption");
        assert_eq!(error.kind(), io::ErrorKind::Interrupted);
        assert!(
            error.to_string().contains("SIGINT") && error.to_string().contains("SIGTERM"),
            "forced shutdown error should identify both signals: {error}"
        );
        engine.quiesce().await;
    }
}
