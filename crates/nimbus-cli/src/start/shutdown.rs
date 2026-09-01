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

    pub(super) async fn wait(mut self) -> &'static str {
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

    pub(super) async fn wait(mut self) -> &'static str {
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

    pub(super) async fn wait(self) -> &'static str {
        let _ = tokio::signal::ctrl_c().await;
        "interrupt"
    }
}

pub(super) async fn serve_until_shutdown<Server>(
    server: Server,
    signals: ProcessShutdownSignals,
    shutdown: ServerShutdownHandle,
) -> io::Result<()>
where
    Server: Future<Output = io::Result<()>>,
{
    tokio::pin!(server);
    tokio::select! {
        result = &mut server => result,
        signal = signals.wait() => {
            tracing::info!(signal, "process shutdown requested");
            shutdown.request_shutdown();
            server.await
        }
    }
}
