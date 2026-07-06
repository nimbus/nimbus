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

#[cfg(unix)]
use std::os::unix::io::AsRawFd;
#[cfg(windows)]
use std::os::windows::io::AsRawSocket;

pub(crate) struct PrereadStream {
    inner: TcpStream,
    prefix: Vec<u8>,
    offset: usize,
    socket_digest: Option<Arc<SocketDigest>>,
}

impl PrereadStream {
    pub(crate) fn new(inner: TcpStream, prefix: Vec<u8>) -> Self {
        let socket_digest = socket_digest(&inner);
        Self {
            inner,
            prefix,
            offset: 0,
            socket_digest,
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
        Pin::new(&mut self.inner).poll_write(cx, buffer)
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
        Pin::new(&mut self.inner).poll_write_vectored(cx, bufs)
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
