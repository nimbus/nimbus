use std::fmt;
use std::pin::Pin;
use std::sync::Arc;
use std::task::{Context, Poll};

use async_trait::async_trait;
use pingora_core::protocols::{
    GetProxyDigest, GetSocketDigest, GetTimingDigest, Peek, Shutdown, SocketDigest, Ssl,
    TimingDigest, UniqueID, UniqueIDType,
};
use tokio::io::{AsyncRead, AsyncWrite, AsyncWriteExt, ReadBuf};
use tokio::net::TcpStream;

use crate::decision_log::EgressDecisionLog;
use crate::terminal::ResponseStartedSignal;

use std::sync::atomic::{AtomicBool, Ordering};

#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;

pub(crate) struct PrereadStream {
    inner: TcpStream,
    prefix: Vec<u8>,
    offset: usize,
    socket_digest: Option<Arc<SocketDigest>>,
    response_started_signal: Option<ResponseStartedSignal>,
    response_started_record: Option<EgressDecisionLog>,
    final_response_write_gate: Option<FinalResponseWriteGate>,
    final_head_scanner: FinalHeadWriteScanner,
    response_started_marked: bool,
}

#[derive(Clone, Default)]
pub(crate) struct FinalResponseWriteGate {
    final_response_ready: Arc<AtomicBool>,
}

impl FinalResponseWriteGate {
    pub(crate) fn new() -> Self {
        Self::default()
    }

    pub(crate) fn arm_final_response(&self) {
        self.final_response_ready.store(true, Ordering::SeqCst);
    }

    fn is_final_response_ready(&self) -> bool {
        self.final_response_ready.load(Ordering::SeqCst)
    }
}

/// Attributes confirmed downstream writes to the FINAL response head.
///
/// Pingora filters every task in a drained batch before writing any of
/// them, so the gate alone cannot distinguish "the 1xx head's bytes were
/// written" from "the final head's bytes were written" when a batch holds
/// both. This scanner inspects the bytes actually accepted by the socket
/// and fires only once a written status line carries a terminal response
/// status: either a final (>= 200) response or `101 Switching Protocols`.
/// Soundness: non-101 informational responses have no body, so every byte
/// written before the terminal head belongs to 1xx head lines — arbitrary
/// body content (which could contain look-alike status lines) can only
/// appear after the terminal head, by which point the scanner has fired and
/// stopped.
#[derive(Default)]
struct FinalHeadWriteScanner {
    /// First bytes of the current line, capped at STATUS_PREFIX_MAX —
    /// enough to decide `HTTP/1.x NNN`.
    line_prefix: Vec<u8>,
    /// Whether the current line already overflowed the prefix cap (skip
    /// until the next newline).
    line_overflowed: bool,
    fired: bool,
}

const STATUS_PREFIX_MAX: usize = 12; // "HTTP/1.1 200"

impl FinalHeadWriteScanner {
    /// Feeds the bytes a write call actually accepted; returns true when
    /// the final response head has been observed on the wire.
    fn observe_written(&mut self, written: &[u8]) -> bool {
        if self.fired {
            return true;
        }
        for &byte in written {
            if byte == b'\n' {
                if self.line_is_terminal_status() {
                    self.fired = true;
                    return true;
                }
                self.line_prefix.clear();
                self.line_overflowed = false;
                continue;
            }
            if self.line_overflowed {
                continue;
            }
            if self.line_prefix.len() < STATUS_PREFIX_MAX {
                self.line_prefix.push(byte);
            } else {
                // Long line: it can still be a status line whose prefix we
                // already captured; stop accumulating, decide at newline.
                self.line_overflowed = true;
            }
        }
        // A completed status line without a trailing newline yet cannot be
        // final-confirmed; the newline arrives with the next write.
        false
    }

    fn line_is_terminal_status(&self) -> bool {
        let line = &self.line_prefix;
        if !line.starts_with(b"HTTP/1.") || line.len() < STATUS_PREFIX_MAX {
            return false;
        }
        // "HTTP/1.x NNN" — bytes 9..12 are the status digits.
        let digits = &line[9..12];
        if !digits.iter().all(u8::is_ascii_digit) {
            return false;
        }
        digits[0] >= b'2' || digits == b"101"
    }
}

impl PrereadStream {
    pub(crate) fn new(inner: TcpStream, prefix: Vec<u8>) -> Self {
        let socket_digest = socket_digest(&inner);
        Self {
            inner,
            prefix,
            offset: 0,
            socket_digest,
            response_started_signal: None,
            response_started_record: None,
            final_response_write_gate: None,
            final_head_scanner: FinalHeadWriteScanner::default(),
            response_started_marked: false,
        }
    }

    pub(crate) fn with_response_started_signal(
        mut self,
        signal: ResponseStartedSignal,
        decision_log: EgressDecisionLog,
        final_response_write_gate: FinalResponseWriteGate,
    ) -> Self {
        self.response_started_signal = Some(signal);
        self.response_started_record = Some(decision_log);
        self.final_response_write_gate = Some(final_response_write_gate);
        self
    }

    fn mark_response_started(&mut self, written: &[u8]) {
        if written.is_empty() || self.response_started_marked {
            return;
        }
        // The gate says a final head has been FILTERED; the scanner decides
        // when that head's bytes are actually on the wire — Pingora writes a
        // drained batch (which may hold a 1xx head AND the final head) only
        // after filtering all of it, so gate-armed alone must not count a
        // written non-101 1xx head as response-started.
        if !self
            .final_response_write_gate
            .as_ref()
            .is_some_and(FinalResponseWriteGate::is_final_response_ready)
        {
            return;
        }
        if !self.final_head_scanner.observe_written(written) {
            return;
        }
        self.response_started_marked = true;
        if let (Some(signal), Some(decision_log)) = (
            self.response_started_signal.as_ref(),
            self.response_started_record.as_ref(),
        ) {
            signal.mark_response_started(decision_log.clone());
        }
    }
}

impl fmt::Debug for PrereadStream {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("PrereadStream")
            .field("prefix_len", &self.prefix.len())
            .field("offset", &self.offset)
            .finish_non_exhaustive()
    }
}

impl AsyncRead for PrereadStream {
    fn poll_read(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &mut ReadBuf<'_>,
    ) -> Poll<std::io::Result<()>> {
        if self.offset < self.prefix.len() {
            let available = self.prefix.len() - self.offset;
            let len = available.min(buffer.remaining());
            buffer.put_slice(&self.prefix[self.offset..self.offset + len]);
            self.offset += len;
            return Poll::Ready(Ok(()));
        }
        Pin::new(&mut self.inner).poll_read(cx, buffer)
    }
}

impl AsyncWrite for PrereadStream {
    fn poll_write(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buffer: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write(cx, buffer) {
            Poll::Ready(Ok(written)) => {
                self.mark_response_started(&buffer[..written]);
                Poll::Ready(Ok(written))
            }
            result => result,
        }
    }

    fn poll_flush(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_flush(cx)
    }

    fn poll_shutdown(mut self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Pin::new(&mut self.inner).poll_shutdown(cx)
    }

    fn poll_write_vectored(
        mut self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        bufs: &[std::io::IoSlice<'_>],
    ) -> Poll<std::io::Result<usize>> {
        match Pin::new(&mut self.inner).poll_write_vectored(cx, bufs) {
            Poll::Ready(Ok(written)) => {
                // Feed the scanner exactly the bytes the socket accepted,
                // in order, across the vectored slices.
                let mut remaining = written;
                for buf in bufs {
                    if remaining == 0 {
                        break;
                    }
                    let take = remaining.min(buf.len());
                    self.mark_response_started(&buf[..take]);
                    remaining -= take;
                }
                Poll::Ready(Ok(written))
            }
            result => result,
        }
    }

    fn is_write_vectored(&self) -> bool {
        self.inner.is_write_vectored()
    }
}

#[async_trait]
impl Shutdown for PrereadStream {
    async fn shutdown(&mut self) -> () {
        let _ = AsyncWriteExt::shutdown(&mut self.inner).await;
    }
}

impl UniqueID for PrereadStream {
    fn id(&self) -> UniqueIDType {
        unique_id(&self.inner)
    }
}

impl Ssl for PrereadStream {}

impl GetTimingDigest for PrereadStream {
    fn get_timing_digest(&self) -> Vec<Option<TimingDigest>> {
        vec![]
    }
}

impl GetProxyDigest for PrereadStream {
    fn get_proxy_digest(&self) -> Option<Arc<pingora_core::protocols::raw_connect::ProxyDigest>> {
        None
    }
}

impl GetSocketDigest for PrereadStream {
    fn get_socket_digest(&self) -> Option<Arc<SocketDigest>> {
        self.socket_digest.clone()
    }
}

impl Peek for PrereadStream {}

#[cfg(unix)]
fn unique_id(stream: &TcpStream) -> UniqueIDType {
    stream.as_raw_fd()
}

#[cfg(windows)]
fn unique_id(stream: &TcpStream) -> UniqueIDType {
    // Pingora's UniqueIDType is usize on Windows; RawSocket is u64.
    stream.as_raw_socket() as UniqueIDType
}

#[cfg(not(any(unix, windows)))]
fn unique_id(_stream: &TcpStream) -> UniqueIDType {
    0
}

#[cfg(unix)]
fn socket_digest(stream: &TcpStream) -> Option<Arc<SocketDigest>> {
    Some(Arc::new(SocketDigest::from_raw_fd(stream.as_raw_fd())))
}

#[cfg(windows)]
fn socket_digest(stream: &TcpStream) -> Option<Arc<SocketDigest>> {
    Some(Arc::new(SocketDigest::from_raw_socket(
        stream.as_raw_socket(),
    )))
}

#[cfg(not(any(unix, windows)))]
fn socket_digest(_stream: &TcpStream) -> Option<Arc<SocketDigest>> {
    None
}

#[cfg(test)]
mod tests {
    use super::FinalHeadWriteScanner;

    #[test]
    fn scanner_ignores_informational_head_and_fires_on_final_head() {
        let mut scanner = FinalHeadWriteScanner::default();
        assert!(!scanner.observe_written(b"HTTP/1.1 100 Continue\r\n\r\n"));
        assert!(scanner.observe_written(b"HTTP/1.1 200 OK\r\nx: y\r\n\r\n"));
    }

    #[test]
    fn scanner_handles_batched_informational_plus_final_in_one_write() {
        let mut scanner = FinalHeadWriteScanner::default();
        // A single accepted write containing only the 1xx portion of a
        // filtered batch must NOT count as final-response-started.
        assert!(!scanner.observe_written(b"HTTP/1.1 100 Continue\r\n\r\nHTTP/1.1 2"));
        // The final head completes in a later write.
        assert!(scanner.observe_written(b"00 OK\r\n\r\n"));
    }

    #[test]
    fn scanner_handles_status_line_split_across_writes() {
        let mut scanner = FinalHeadWriteScanner::default();
        assert!(!scanner.observe_written(b"HTTP/"));
        assert!(!scanner.observe_written(b"1.1 20"));
        // The status digits are complete but the line has no newline yet.
        assert!(!scanner.observe_written(b"0 OK"));
        assert!(scanner.observe_written(b"\r\n"));
    }

    #[test]
    fn scanner_fires_on_switching_protocols() {
        let mut scanner = FinalHeadWriteScanner::default();
        assert!(scanner.observe_written(
            b"HTTP/1.1 101 Switching Protocols\r\nUpgrade: h2c\r\nX-Long-Header-Value: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\n"
        ));
    }

    #[test]
    fn scanner_does_not_fire_on_header_lines_or_long_lines() {
        let mut scanner = FinalHeadWriteScanner::default();
        assert!(
            !scanner
                .observe_written(b"X-Long-Header-Value: aaaaaaaaaaaaaaaaaaaaaaaaaaaaaa\r\n\r\n")
        );
    }

    #[test]
    fn scanner_stays_fired_after_final_head() {
        let mut scanner = FinalHeadWriteScanner::default();
        assert!(scanner.observe_written(b"HTTP/1.1 204 No Content\r\n\r\n"));
        // Body/other bytes after firing keep reporting started.
        assert!(scanner.observe_written(b"arbitrary body bytes"));
    }
}
