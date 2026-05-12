use super::*;

type LibsqlTransportError = Box<dyn std::error::Error + Send + Sync + 'static>;

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
        let mut http = self.http.clone();
        let tls = self.tls.clone();
        Box::pin(async move {
            let scheme = uri.scheme_str().unwrap_or("https");
            let stream = http.call(uri.clone()).await?;
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
