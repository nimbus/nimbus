use std::io;
use std::net::SocketAddr;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::net::TcpStream;
use tokio::time;

use crate::body::{BODY_STREAM_CHUNK_BYTES, timeout_io};

pub(crate) async fn splice_connect(
    mut client: TcpStream,
    mut upstream: TcpStream,
    buffered_client_bytes: &[u8],
    io_timeout: Duration,
) -> io::Result<(u64, u64)> {
    timeout_io(
        io_timeout,
        client.write_all(b"HTTP/1.1 200 Connection Established\r\nConnection: close\r\n\r\n"),
    )
    .await?;
    if !buffered_client_bytes.is_empty() {
        timeout_io(io_timeout, upstream.write_all(buffered_client_bytes)).await?;
    }

    let (mut client_reader, mut client_writer) = tokio::io::split(client);
    let (mut upstream_reader, mut upstream_writer) = tokio::io::split(upstream);
    let client_to_upstream = copy_half_with_idle_timeout(
        &mut client_reader,
        &mut upstream_writer,
        io_timeout,
        "client-to-upstream tunnel timed out",
    );
    let upstream_to_client = copy_half_with_idle_timeout(
        &mut upstream_reader,
        &mut client_writer,
        io_timeout,
        "upstream-to-client tunnel timed out",
    );
    let (to_upstream, to_client) = tokio::try_join!(client_to_upstream, upstream_to_client)?;
    // The buffered preamble already went to the upstream above; attribute it.
    Ok((to_upstream + buffered_client_bytes.len() as u64, to_client))
}

pub(crate) async fn connect_upstream(
    upstream_addr: SocketAddr,
    connect_timeout: Duration,
) -> io::Result<TcpStream> {
    time::timeout(connect_timeout, TcpStream::connect(upstream_addr))
        .await
        .map_err(|_| io::Error::new(io::ErrorKind::TimedOut, "upstream connect timed out"))?
}

async fn copy_half_with_idle_timeout<R, W>(
    reader: &mut R,
    writer: &mut W,
    io_timeout: Duration,
    timeout_message: &'static str,
) -> io::Result<u64>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut copied = 0_u64;
    let mut chunk = [0_u8; BODY_STREAM_CHUNK_BYTES];
    loop {
        let read = match time::timeout(io_timeout, reader.read(&mut chunk)).await {
            Ok(result) => result?,
            Err(_) => return Err(io::Error::new(io::ErrorKind::TimedOut, timeout_message)),
        };
        if read == 0 {
            let _ = timeout_io(io_timeout, writer.shutdown()).await;
            return Ok(copied);
        }
        timeout_io(io_timeout, writer.write_all(&chunk[..read])).await?;
        copied += read as u64;
    }
}
