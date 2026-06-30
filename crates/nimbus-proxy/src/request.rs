use nimbus_egress::{EgressProtocol, EgressRequest};
use url::Url;

use crate::decision_log::EgressDecisionLog;
use crate::response::HttpProxyResponse;

pub(crate) struct ParsedProxyRequest {
    pub(crate) egress_request: EgressRequest,
    pub(crate) upstream_host: String,
    pub(crate) upstream_port: u16,
    pub(crate) mode: ProxyRequestMode,
    pub(crate) method: String,
    pub(crate) version: String,
    pub(crate) header_lines: Vec<String>,
    pub(crate) content_length: Option<usize>,
    pub(crate) body_offset: usize,
}

pub(crate) struct PreparedProxyRequest {
    pub(crate) header_lines: Vec<String>,
    pub(crate) inspected_body: Option<Vec<u8>>,
    pub(crate) decision_log: EgressDecisionLog,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) enum ProxyRequestMode {
    ForwardHttp { origin_form: String },
    ConnectTunnel,
}

pub(crate) fn parse_proxy_request(
    buffer: &[u8],
) -> std::result::Result<ParsedProxyRequest, HttpProxyResponse> {
    let Some(header_end) = find_header_end(buffer) else {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy request is missing HTTP headers",
        ));
    };
    let body_offset = header_end + 4;
    let headers = std::str::from_utf8(&buffer[..header_end]).map_err(|_| {
        HttpProxyResponse::bad_request("egress proxy request headers must be UTF-8")
    })?;
    reject_bare_cr_or_lf(headers)?;
    let mut lines = headers.split("\r\n");
    let request_line = lines.next().unwrap_or_default();
    let mut parts = request_line.split_whitespace();
    let method = parts.next().unwrap_or_default();
    let target = parts.next().unwrap_or_default();
    let version = parts.next().unwrap_or_default();
    if method.is_empty() || target.is_empty() || version.is_empty() || parts.next().is_some() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy request line must be METHOD absolute-uri HTTP-version",
        ));
    }
    if method.eq_ignore_ascii_case("CONNECT") {
        let (host, port) = parse_connect_authority(target)?;
        let egress_request =
            EgressRequest::new(EgressProtocol::Https, host.clone(), port).with_http("CONNECT", "");
        return Ok(ParsedProxyRequest {
            egress_request,
            upstream_host: host,
            upstream_port: port,
            mode: ProxyRequestMode::ConnectTunnel,
            method: method.to_owned(),
            version: version.to_owned(),
            header_lines: lines.map(ToOwned::to_owned).collect(),
            content_length: None,
            body_offset,
        });
    }
    let url = Url::parse(target).map_err(|_| {
        HttpProxyResponse::bad_request("egress proxy target must be an absolute URI")
    })?;
    if !url.username().is_empty() || url.password().is_some() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy canonical authority must not include userinfo",
        ));
    }
    let protocol = match url.scheme() {
        "http" => EgressProtocol::Http,
        "https" => {
            return Err(HttpProxyResponse::not_implemented(
                "egress proxy HTTPS requests must use CONNECT",
            ));
        }
        _ => {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy only supports http and https targets",
            ));
        }
    };
    let host = url
        .host_str()
        .ok_or_else(|| HttpProxyResponse::bad_request("egress proxy target needs a host"))?
        .to_owned();
    let host = canonicalize_proxy_host(&host)?;
    let port = url.port_or_known_default().ok_or_else(|| {
        HttpProxyResponse::bad_request("egress proxy target needs an explicit port")
    })?;
    let origin_form = origin_form(&url);
    let egress_request =
        EgressRequest::new(protocol, host.clone(), port).with_http(method, url.path());
    let header_lines: Vec<String> = lines
        .filter(|line| {
            let header = line
                .split_once(':')
                .map(|(name, _)| name.trim())
                .unwrap_or_default();
            !header.eq_ignore_ascii_case("connection")
                && !header.eq_ignore_ascii_case("proxy-connection")
        })
        .map(ToOwned::to_owned)
        .collect();
    if has_header(&header_lines, "transfer-encoding") {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy does not forward Transfer-Encoding requests; use Content-Length",
        ));
    }
    let content_length = content_length(&header_lines)?;

    Ok(ParsedProxyRequest {
        egress_request,
        upstream_host: host,
        upstream_port: port,
        mode: ProxyRequestMode::ForwardHttp { origin_form },
        method: method.to_owned(),
        version: version.to_owned(),
        header_lines,
        content_length,
        body_offset,
    })
}

pub(crate) fn render_upstream_request(
    parsed: &ParsedProxyRequest,
    header_lines: &[String],
) -> String {
    let ProxyRequestMode::ForwardHttp { origin_form } = &parsed.mode else {
        return String::new();
    };
    let mut rendered = format!("{} {} {}\r\n", parsed.method, origin_form, parsed.version);
    // Normalize the Host header to the authorized authority. The caller's Host
    // header is advisory and must never be forwarded verbatim: the policy
    // authorized `upstream_host:upstream_port`, so that is the only authority
    // the upstream may be told it is serving. Any caller-supplied Host lines
    // are dropped and replaced.
    rendered.push_str(&format!(
        "Host: {}:{}\r\n",
        parsed.upstream_host, parsed.upstream_port
    ));
    for line in header_lines {
        if is_host_header(line) {
            continue;
        }
        rendered.push_str(line);
        rendered.push_str("\r\n");
    }
    rendered.push_str("Connection: close\r\n\r\n");
    rendered
}

fn parse_connect_authority(target: &str) -> std::result::Result<(String, u16), HttpProxyResponse> {
    if target.contains("://") || target.contains('/') || target.contains('@') {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy CONNECT target must be host:port",
        ));
    }
    let (host, port) = if let Some(rest) = target.strip_prefix('[') {
        let Some((host, suffix)) = rest.split_once(']') else {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT IPv6 target needs closing bracket",
            ));
        };
        let Some(port) = suffix.strip_prefix(':') else {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT target needs a port",
            ));
        };
        (host.to_owned(), port)
    } else {
        let Some((host, port)) = target.rsplit_once(':') else {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT target needs a port",
            ));
        };
        if host.contains(':') {
            return Err(HttpProxyResponse::bad_request(
                "egress proxy CONNECT IPv6 target must use brackets",
            ));
        }
        (host.to_owned(), port)
    };
    if host.is_empty() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy CONNECT target needs a host",
        ));
    }
    let port = port.parse::<u16>().map_err(|_| {
        HttpProxyResponse::bad_request("egress proxy CONNECT port must be a number")
    })?;
    if port == 0 {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy CONNECT port must not be 0",
        ));
    }
    Ok((canonicalize_proxy_host(&host)?, port))
}

fn origin_form(url: &Url) -> String {
    match url.query() {
        Some(query) => format!("{}?{query}", url.path()),
        None => url.path().to_owned(),
    }
}

fn content_length(
    header_lines: &[String],
) -> std::result::Result<Option<usize>, HttpProxyResponse> {
    let mut parsed = None;
    for line in header_lines {
        let Some((name, value)) = line.split_once(':') else {
            continue;
        };
        if !name.trim().eq_ignore_ascii_case("content-length") {
            continue;
        }
        let value = value.trim().parse::<usize>().map_err(|_| {
            HttpProxyResponse::bad_request("Content-Length must be a non-negative integer")
        })?;
        if parsed.is_some_and(|current| current != value) {
            return Err(HttpProxyResponse::bad_request(
                "conflicting Content-Length headers are not allowed",
            ));
        }
        parsed = Some(value);
    }
    Ok(parsed)
}

fn is_host_header(line: &str) -> bool {
    line.split_once(':')
        .map(|(name, _)| name.trim().eq_ignore_ascii_case("host"))
        .unwrap_or(false)
}

fn has_header(header_lines: &[String], expected: &str) -> bool {
    header_lines.iter().any(|line| {
        line.split_once(':')
            .map(|(name, _)| name.trim().eq_ignore_ascii_case(expected))
            .unwrap_or(false)
    })
}

pub(crate) fn find_header_end(buffer: &[u8]) -> Option<usize> {
    buffer.windows(4).position(|window| window == b"\r\n\r\n")
}

/// Reject a header block containing a bare CR or bare LF. Within `buffer[..header_end]`
/// every header line is `\r\n`-terminated, so a `\r` not followed by `\n` (or a `\n`
/// not preceded by `\r`) is an embedded line that survives `split("\r\n")` inside one
/// header value and smuggles a forbidden header (e.g. `Authorization`) past the
/// per-line credential guard to any LF-lenient upstream. (audit H2.)
fn reject_bare_cr_or_lf(headers: &str) -> std::result::Result<(), HttpProxyResponse> {
    let bytes = headers.as_bytes();
    for (index, &byte) in bytes.iter().enumerate() {
        match byte {
            b'\r' if bytes.get(index + 1) != Some(&b'\n') => {
                return Err(HttpProxyResponse::bad_request(
                    "egress proxy request header block contains a bare CR",
                ));
            }
            b'\n' if index == 0 || bytes[index - 1] != b'\r' => {
                return Err(HttpProxyResponse::bad_request(
                    "egress proxy request header block contains a bare LF",
                ));
            }
            _ => {}
        }
    }
    Ok(())
}

fn canonicalize_proxy_host(host: &str) -> std::result::Result<String, HttpProxyResponse> {
    let trimmed = host.trim();
    if trimmed.is_empty() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy target needs a host",
        ));
    }
    if trimmed.contains('%')
        || trimmed.contains('@')
        || trimmed.contains('/')
        || trimmed.contains('\\')
        || trimmed.chars().any(char::is_whitespace)
    {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy canonical authority rejected ambiguous host",
        ));
    }
    let canonical = trimmed.trim_end_matches('.').to_ascii_lowercase();
    // M7: CONNECT authorities skip the WHATWG IPv4 normalization that `Url::parse`
    // gives ForwardHttp targets, so hex/octal/dword IPv4 obfuscation
    // (`0x7f000001`, `2130706433`, `0177.0.0.1`) would otherwise reach the resolver
    // unrecognized. A host that is purely IPv4-numeric-shaped must already be
    // canonical dotted-decimal; reject every other numeric form. (audit M7.)
    let ipv4_shaped = canonical
        .chars()
        .all(|c| c.is_ascii_hexdigit() || c == '.' || c == 'x' || c == 'X')
        && canonical.chars().any(|c| c.is_ascii_digit());
    if ipv4_shaped && canonical.parse::<std::net::Ipv4Addr>().is_err() {
        return Err(HttpProxyResponse::bad_request(
            "egress proxy rejected a non-canonical numeric authority",
        ));
    }
    Ok(canonical)
}
