use std::io::{self, Write};
use std::net::TcpStream;

pub(crate) struct HttpProxyResponse {
    status: &'static str,
    body: String,
}

impl HttpProxyResponse {
    pub(crate) fn bad_request(body: &str) -> Self {
        Self {
            status: "400 Bad Request",
            body: body.to_owned(),
        }
    }

    pub(crate) fn forbidden(body: &str) -> Self {
        Self {
            status: "403 Forbidden",
            body: body.to_owned(),
        }
    }

    pub(crate) fn not_implemented(body: &str) -> Self {
        Self {
            status: "501 Not Implemented",
            body: body.to_owned(),
        }
    }

    pub(crate) fn bad_gateway(body: &str) -> Self {
        Self {
            status: "502 Bad Gateway",
            body: body.to_owned(),
        }
    }

    pub(crate) fn service_unavailable(body: &str) -> Self {
        Self {
            status: "503 Service Unavailable",
            body: body.to_owned(),
        }
    }

    pub(crate) fn request_header_fields_too_large(body: &str) -> Self {
        Self {
            status: "431 Request Header Fields Too Large",
            body: body.to_owned(),
        }
    }
}

pub(crate) fn write_http_response(
    client: &mut TcpStream,
    response: HttpProxyResponse,
) -> io::Result<()> {
    let rendered = format!(
        "HTTP/1.1 {}\r\nContent-Type: text/plain\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{}",
        response.status,
        response.body.len(),
        response.body
    );
    client.write_all(rendered.as_bytes())
}
