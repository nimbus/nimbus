use super::*;

type LibsqlTransportError = Box<dyn std::error::Error + Send + Sync + 'static>;

/// Owns the remote database factory and the tenant's bounded transport lanes.
///
/// Idempotent reads and control operations use the generation-owned read lane,
/// so a safe retry can replace a failed Hrana/Hyper client. Non-replayable
/// tenant transactions use a distinct retained write lane. libSQL opens a new
/// Hrana stream for each transaction while retaining the lane's Hyper client,
/// which bounds connection pools without ever replaying a write.
#[derive(Clone)]
pub(super) struct LibsqlRemoteSession {
    database: Arc<Database>,
    connections: Arc<RwLock<RemoteConnectionSet<Connection>>>,
}

#[derive(Clone)]
pub(super) struct LibsqlRemoteConnection {
    pub(super) generation: u64,
    pub(super) connection: Connection,
}

struct RemoteConnectionSet<T> {
    read_generation: u64,
    read: Option<T>,
    write: Option<T>,
}

impl<T> RemoteConnectionSet<T> {
    fn new(read: T, write: T) -> Self {
        Self {
            read_generation: 0,
            read: Some(read),
            write: Some(write),
        }
    }

    fn replace_read_if_generation(
        &mut self,
        failed_generation: u64,
        replacement: T,
    ) -> Result<bool> {
        if self.read_generation != failed_generation {
            return Ok(false);
        }
        if self.read.is_none() || self.write.is_none() {
            return Err(retired_session_error());
        }
        let next_generation = self.read_generation.checked_add(1).ok_or_else(|| {
            Error::Internal("libsql remote session generation exhausted".to_string())
        })?;
        self.read_generation = next_generation;
        self.read = Some(replacement);
        Ok(true)
    }

    fn take_for_retirement(&mut self) -> Result<Option<(T, T)>> {
        if self.read.is_none() && self.write.is_none() {
            return Ok(None);
        }
        if self.read.is_none() || self.write.is_none() {
            return Err(Error::Internal(
                "libsql remote session transport lanes retired inconsistently".to_string(),
            ));
        }
        let next_generation = self.read_generation.checked_add(1).ok_or_else(|| {
            Error::Internal("libsql remote session generation exhausted".to_string())
        })?;
        self.read_generation = next_generation;
        Ok(Some((
            self.read
                .take()
                .expect("read lane checked present before retirement"),
            self.write
                .take()
                .expect("write lane checked present before retirement"),
        )))
    }
}

impl<T: Clone> RemoteConnectionSet<T> {
    fn versioned_read(&self) -> Result<(u64, T)> {
        self.read
            .as_ref()
            .map(|connection| (self.read_generation, connection.clone()))
            .ok_or_else(retired_session_error)
    }

    fn retained_write(&self) -> Result<T> {
        self.write.clone().ok_or_else(retired_session_error)
    }
}

impl LibsqlRemoteSession {
    pub(super) fn new(database: Database) -> Result<Self> {
        let read = database.connect().map_err(map_libsql_error)?;
        let write = database.connect().map_err(map_libsql_error)?;
        Ok(Self {
            database: Arc::new(database),
            connections: Arc::new(RwLock::new(RemoteConnectionSet::new(read, write))),
        })
    }

    pub(super) fn retryable_connection(&self) -> Result<Connection> {
        Ok(self.versioned_retryable_connection()?.connection)
    }

    pub(super) fn versioned_retryable_connection(&self) -> Result<LibsqlRemoteConnection> {
        let session = self
            .connections
            .read()
            .map_err(|_| Error::Internal("libsql remote session lock is poisoned".to_string()))?;
        let (generation, connection) = session.versioned_read()?;
        Ok(LibsqlRemoteConnection {
            generation,
            connection,
        })
    }

    /// Returns the retained connection for one non-replayable operation.
    ///
    /// A remote libSQL transaction opens an independent Hrana stream on this
    /// connection's shared Hyper client. A failed write is never retried; its
    /// outcome is classified through the independent read/control lane. The
    /// retained client lets Hyper discard a failed socket and establish a new
    /// one for a later transaction without creating one pool per mutation.
    pub(super) fn write_connection(&self) -> Result<Connection> {
        self.connections
            .read()
            .map_err(|_| Error::Internal("libsql remote session lock is poisoned".to_string()))?
            .retained_write()
    }

    /// Replaces the failed generation exactly once.
    ///
    /// Concurrent operations may observe the same transport failure. Only the
    /// first reconnect may install a replacement; a stale reconnect must not
    /// overwrite or reset the newer connection while another caller retries on
    /// it.
    pub(super) fn reconnect_after_failure(&self, failed_generation: u64) -> Result<bool> {
        let replacement = self.database.connect().map_err(map_libsql_error)?;
        self.connections
            .write()
            .map_err(|_| Error::Internal("libsql remote session lock is poisoned".to_string()))?
            .replace_read_if_generation(failed_generation, replacement)
    }

    /// Completes and releases both retained transport lanes after their owner drains.
    ///
    /// Both connections are taken atomically before any await, making stale
    /// runtime handles fail closed and preventing a read reconnect from
    /// reviving a retired generation. An autocommit statement carries each
    /// close in the awaited pipeline request. If that request fails, dropping
    /// the taken connection still invokes libSQL's best-effort close path.
    pub(super) async fn retire_after_drain(&self) -> Result<()> {
        let connections = self
            .connections
            .write()
            .map_err(|_| Error::Internal("libsql remote session lock is poisoned".to_string()))?
            .take_for_retirement()?;
        let Some((read, write)) = connections else {
            return Ok(());
        };
        let read_result = retire_connection(read).await;
        let write_result = retire_connection(write).await;
        read_result.and(write_result)
    }
}

async fn retire_connection(connection: Connection) -> Result<()> {
    if !connection.is_autocommit() {
        connection
            .execute("ROLLBACK", ())
            .await
            .map_err(map_libsql_error)?;
    }
    connection
        .execute("SELECT 1", ())
        .await
        .map_err(map_libsql_error)?;
    Ok(())
}

fn retired_session_error() -> Error {
    Error::storage(
        StorageErrorKind::Unavailable,
        "libsql remote session has been retired",
    )
}

#[doc(hidden)]
#[derive(Clone)]
pub struct LibsqlTransportConnector {
    http: HttpConnector,
    tls: TokioTlsConnector,
}

#[doc(hidden)]
pub enum LibsqlTransportStream {
    Http(TcpStream),
    Https(TlsStream<TcpStream>),
}

impl LibsqlTransportConnector {
    fn new() -> Result<Self> {
        let mut http = HttpConnector::new();
        http.enforce_http(false);
        http.set_nodelay(true);
        let tls = NativeTlsConnector::builder()
            .build()
            .map(TokioTlsConnector::from)
            .map_err(|error| {
                Error::storage(
                    StorageErrorKind::Other,
                    format!("failed to build libsql TLS connector: {error}"),
                )
            })?;
        Ok(Self { http, tls })
    }
}

impl Service<hyper::http::Uri> for LibsqlTransportConnector {
    type Response = LibsqlTransportStream;
    type Error = LibsqlTransportError;
    type Future =
        Pin<Box<dyn Future<Output = std::result::Result<Self::Response, Self::Error>> + Send>>;

    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<std::result::Result<(), Self::Error>> {
        self.http.poll_ready(cx).map_err(Into::into)
    }

    fn call(&mut self, uri: hyper::http::Uri) -> Self::Future {
        // `Service::poll_ready` and `Service::call` must address the same
        // service instance. Dispatching through a fresh clone here bypasses
        // resolver backpressure and becomes unreliable during provider
        // reconnect/reopen bursts.
        let connect = self.http.call(uri.clone());
        let tls = self.tls.clone();
        Box::pin(async move {
            let scheme = uri.scheme_str().unwrap_or("https");
            let stream = connect.await?;
            if scheme.eq_ignore_ascii_case("http") {
                return Ok(LibsqlTransportStream::Http(stream));
            }
            if !scheme.eq_ignore_ascii_case("https") {
                return Err(io::Error::new(
                    io::ErrorKind::InvalidInput,
                    format!("unsupported libsql URI scheme '{scheme}'"),
                )
                .into());
            }
            let host = uri.host().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "libsql URI is missing a host")
            })?;
            let tls_stream = tls.connect(host, stream).await?;
            Ok(LibsqlTransportStream::Https(tls_stream))
        })
    }
}

impl HyperConnection for LibsqlTransportStream {
    fn connected(&self) -> Connected {
        match self {
            Self::Http(stream) => stream.connected(),
            Self::Https(stream) => stream.get_ref().get_ref().get_ref().connected(),
        }
    }
}

impl AsyncRead for LibsqlTransportStream {
    fn poll_read(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &mut ReadBuf<'_>,
    ) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_read(cx, buf),
            Self::Https(stream) => Pin::new(stream).poll_read(cx, buf),
        }
    }
}

impl AsyncWrite for LibsqlTransportStream {
    fn poll_write(
        self: Pin<&mut Self>,
        cx: &mut Context<'_>,
        buf: &[u8],
    ) -> Poll<io::Result<usize>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_write(cx, buf),
            Self::Https(stream) => Pin::new(stream).poll_write(cx, buf),
        }
    }

    fn poll_flush(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_flush(cx),
            Self::Https(stream) => Pin::new(stream).poll_flush(cx),
        }
    }

    fn poll_shutdown(self: Pin<&mut Self>, cx: &mut Context<'_>) -> Poll<io::Result<()>> {
        match self.get_mut() {
            Self::Http(stream) => Pin::new(stream).poll_shutdown(cx),
            Self::Https(stream) => Pin::new(stream).poll_shutdown(cx),
        }
    }
}

#[doc(hidden)]
pub fn libsql_transport_connector() -> Result<LibsqlTransportConnector> {
    LibsqlTransportConnector::new()
}

#[cfg(test)]
mod tests {
    use super::RemoteConnectionSet;

    #[test]
    fn stale_read_reconnect_cannot_replace_connection_used_by_retry() {
        let mut session = RemoteConnectionSet::new("failed read", "retained write");
        let first_failure_generation = session.read_generation;
        let concurrent_failure_generation = session.read_generation;

        assert!(
            session
                .replace_read_if_generation(first_failure_generation, "replacement read")
                .expect("first reconnect should advance the generation")
        );
        let retry_generation = session.read_generation;
        assert_eq!(session.read, Some("replacement read"));
        assert_eq!(session.write, Some("retained write"));

        assert!(
            !session
                .replace_read_if_generation(concurrent_failure_generation, "stale replacement read")
                .expect("stale reconnect should be rejected")
        );
        assert_eq!(session.read_generation, retry_generation);
        assert_eq!(session.read, Some("replacement read"));
        assert_eq!(session.write, Some("retained write"));
    }

    #[test]
    fn current_read_generation_failure_installs_one_new_connection() {
        let mut session = RemoteConnectionSet::new("initial read", "retained write");

        assert!(
            session
                .replace_read_if_generation(0, "first replacement read")
                .expect("current generation should be replaceable")
        );
        assert_eq!(session.read_generation, 1);
        assert_eq!(session.read, Some("first replacement read"));
        assert_eq!(session.write, Some("retained write"));

        assert!(
            session
                .replace_read_if_generation(1, "second replacement read")
                .expect("a later current generation should remain replaceable")
        );
        assert_eq!(session.read_generation, 2);
        assert_eq!(session.read, Some("second replacement read"));
        assert_eq!(session.write, Some("retained write"));
    }

    #[test]
    fn repeated_write_acquisition_reuses_lane_across_read_reconnect() {
        let mut session = RemoteConnectionSet::new("initial read", "retained write");

        assert_eq!(
            session
                .retained_write()
                .expect("active session should expose its write lane"),
            "retained write"
        );
        assert!(
            session
                .replace_read_if_generation(0, "replacement read")
                .expect("read reconnect should succeed")
        );
        assert_eq!(
            session
                .retained_write()
                .expect("read reconnect must not replace the write lane"),
            "retained write"
        );
    }

    #[test]
    fn retirement_takes_both_lanes_and_fences_the_session() {
        let mut session = RemoteConnectionSet::new("retained read", "retained write");
        let failed_generation = session.read_generation;

        assert_eq!(
            session
                .take_for_retirement()
                .expect("retirement should advance the generation"),
            Some(("retained read", "retained write"))
        );
        assert_eq!(session.read, None);
        assert_eq!(session.write, None);
        assert_eq!(session.read_generation, failed_generation + 1);
        assert!(
            !session
                .replace_read_if_generation(failed_generation, "stale replacement read")
                .expect("a stale reconnect should remain a no-op")
        );
        assert_eq!(
            session
                .take_for_retirement()
                .expect("retirement should be idempotent"),
            None
        );
    }
}
