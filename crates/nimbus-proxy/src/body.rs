use std::io;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::time;

use crate::response::HttpProxyResponse;

pub(crate) const BODY_STREAM_CHUNK_BYTES: usize = 8 * 1024;
pub(crate) const BODY_PREALLOC_CLAMP_BYTES: usize = 64 * 1024;

pub(crate) async fn read_exact_body_into_buffer<R>(
    reader: &mut R,
    buffer: &mut Vec<u8>,
    body_offset: usize,
    content_length: usize,
    io_timeout: Duration,
) -> std::result::Result<Vec<u8>, HttpProxyResponse>
where
    R: AsyncRead + Unpin,
{
    while buffer.len().saturating_sub(body_offset) < content_length {
        let mut chunk = [0_u8; BODY_STREAM_CHUNK_BYTES];
        let read_len = (content_length - buffer.len().saturating_sub(body_offset)).min(chunk.len());
        let read = timeout_io(io_timeout, reader.read(&mut chunk[..read_len]))
            .await
            .map_err(|_| {
                HttpProxyResponse::forbidden("DLP inspection input unavailable while reading body")
            })?;
        if read == 0 {
            return Err(HttpProxyResponse::forbidden(
                "DLP inspection input unavailable: client closed early",
            ));
        }
        buffer.extend_from_slice(&chunk[..read]);
    }
    let mut body = Vec::with_capacity(content_length.min(BODY_PREALLOC_CLAMP_BYTES));
    body.extend_from_slice(&buffer[body_offset..body_offset + content_length]);
    Ok(body)
}

pub(crate) async fn stream_content_length_body<R, W>(
    reader: &mut R,
    writer: &mut W,
    buffered_client_bytes: &[u8],
    content_length: usize,
    io_timeout: Duration,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let buffered_len = buffered_client_bytes.len().min(content_length);
    if buffered_len > 0 {
        timeout_io(
            io_timeout,
            writer.write_all(&buffered_client_bytes[..buffered_len]),
        )
        .await?;
    }

    let mut remaining = content_length - buffered_len;
    let mut chunk = [0_u8; BODY_STREAM_CHUNK_BYTES];
    while remaining > 0 {
        let read_len = remaining.min(chunk.len());
        let read = timeout_io(io_timeout, reader.read(&mut chunk[..read_len])).await?;
        if read == 0 {
            return Err(io::Error::new(
                io::ErrorKind::UnexpectedEof,
                "client closed before sending declared request body",
            ));
        }
        timeout_io(io_timeout, writer.write_all(&chunk[..read])).await?;
        remaining -= read;
    }
    Ok(())
}

pub(crate) async fn copy_until_eof<R, W>(
    reader: &mut R,
    writer: &mut W,
    io_timeout: Duration,
) -> io::Result<()>
where
    R: AsyncRead + Unpin,
    W: AsyncWrite + Unpin,
{
    let mut chunk = [0_u8; BODY_STREAM_CHUNK_BYTES];
    loop {
        let read = match timeout_io(io_timeout, reader.read(&mut chunk)).await {
            Ok(read) => read,
            Err(error)
                if error.kind() == io::ErrorKind::UnexpectedEof
                    && error
                        .to_string()
                        .contains("without sending TLS close_notify") =>
            {
                0
            }
            Err(error) => return Err(error),
        };
        if read == 0 {
            return Ok(());
        }
        timeout_io(io_timeout, writer.write_all(&chunk[..read])).await?;
    }
}

pub(crate) async fn timeout_io<T>(
    io_timeout: Duration,
    operation: impl std::future::Future<Output = io::Result<T>>,
) -> io::Result<T> {
    match time::timeout(io_timeout, operation).await {
        Ok(result) => result,
        Err(_) => Err(io::Error::new(io::ErrorKind::TimedOut, "I/O timed out")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn bounded_reader_clamps_declared_length_preallocation() {
        let mut reader = tokio::io::empty();
        let mut buffer = b"POST / HTTP/1.1\r\nContent-Length: 1048576\r\n\r\nshort".to_vec();
        let body_offset = buffer.len() - 5;

        let response = read_exact_body_into_buffer(
            &mut reader,
            &mut buffer,
            body_offset,
            1024 * 1024,
            Duration::from_millis(10),
        )
        .await
        .expect_err("short stream should fail before the declared body is read");

        assert!(response.body().contains("client closed early"));
        assert!(
            buffer.len() < BODY_PREALLOC_CLAMP_BYTES,
            "reader must not grow the shared buffer to the declared length on early EOF"
        );
    }
}
