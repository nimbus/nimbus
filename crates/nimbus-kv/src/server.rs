use std::collections::BTreeMap;
use std::io;
use std::net::SocketAddr;
use std::sync::Arc;
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use nimbus_core::{TenantId, refuse_non_loopback_bind};
use rand::{RngCore, rngs::OsRng};
use redis_protocol::error::RedisProtocolError;
use redis_protocol::resp2::{
    decode as resp2_decode, encode as resp2_encode,
    types::{OwnedFrame as Resp2Frame, Resp2Frame as Resp2FrameTrait},
};
use redis_protocol::resp3::{
    decode::complete as resp3_decode,
    encode::complete as resp3_encode,
    types::{FrameMap, OwnedFrame as Resp3Frame, Resp3Frame as Resp3FrameTrait, RespVersion},
};
use thiserror::Error;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::{TcpListener, TcpStream};

use crate::{NimbusKvMetrics, NimbusKvStore, TieringConfig};

const MAX_FRAME_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub enum KvError {
    #[error(transparent)]
    Io(#[from] io::Error),
    #[error("RESP protocol error: {0}")]
    Protocol(#[from] RedisProtocolError),
    #[error(transparent)]
    Core(#[from] nimbus_core::Error),
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CredentialBinding {
    pub username: String,
    pub password: String,
    pub tenant: TenantId,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DevCredential {
    pub username: String,
    pub password: String,
    pub tenant: TenantId,
}

#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct CredentialRegistry {
    bindings: BTreeMap<String, CredentialBinding>,
}

impl CredentialRegistry {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    #[must_use]
    pub fn bind(
        mut self,
        username: impl Into<String>,
        password: impl Into<String>,
        tenant: TenantId,
    ) -> Self {
        let username = username.into();
        self.bindings.insert(
            username.clone(),
            CredentialBinding {
                username,
                password: password.into(),
                tenant,
            },
        );
        self
    }

    #[must_use]
    pub fn single_dev(tenant: TenantId, password: impl Into<String>) -> Self {
        let username = tenant.as_str().to_owned();
        Self::new().bind(username, password, tenant)
    }

    #[must_use]
    pub fn generated_dev(tenant: TenantId) -> (Self, DevCredential) {
        let username = tenant.as_str().to_owned();
        Self::generated_dev_for(username, tenant)
    }

    #[must_use]
    pub fn generated_dev_for(
        username: impl Into<String>,
        tenant: TenantId,
    ) -> (Self, DevCredential) {
        let username = username.into();
        let password = generate_dev_password();
        let credential = DevCredential {
            username: username.clone(),
            password: password.clone(),
            tenant: tenant.clone(),
        };
        (Self::new().bind(username, password, tenant), credential)
    }

    #[must_use]
    pub fn len(&self) -> usize {
        self.bindings.len()
    }

    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.bindings.is_empty()
    }

    pub fn authenticate(
        &self,
        username: Option<&str>,
        password: &str,
    ) -> Option<CredentialBinding> {
        match username {
            Some(username) => self
                .bindings
                .get(username)
                .filter(|binding| binding.password == password)
                .cloned(),
            None => {
                let mut matches = self
                    .bindings
                    .values()
                    .filter(|binding| binding.password == password);
                let binding = matches.next()?;
                if matches.next().is_some() {
                    None
                } else {
                    Some(binding.clone())
                }
            }
        }
    }
}

#[derive(Debug, Clone)]
pub struct NimbusKvConfig {
    pub bind_addr: SocketAddr,
    pub credentials: CredentialRegistry,
    pub store: Option<NimbusKvStore>,
    pub metrics: Option<NimbusKvMetrics>,
}

impl NimbusKvConfig {
    #[must_use]
    pub fn new(bind_addr: SocketAddr, credentials: CredentialRegistry) -> Self {
        Self {
            bind_addr,
            credentials,
            store: None,
            metrics: None,
        }
    }

    #[must_use]
    pub fn with_store(mut self, store: NimbusKvStore) -> Self {
        self.store = Some(store);
        self
    }

    #[must_use]
    pub fn with_metrics(mut self, metrics: NimbusKvMetrics) -> Self {
        self.metrics = Some(metrics);
        self
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Protocol {
    Resp2,
    Resp3,
}

#[derive(Debug, Clone)]
struct ConnectionState {
    protocol: Protocol,
    binding: Option<CredentialBinding>,
}

impl Default for ConnectionState {
    fn default() -> Self {
        Self {
            protocol: Protocol::Resp2,
            binding: None,
        }
    }
}

#[derive(Debug)]
struct CommandRequest {
    args: Vec<Vec<u8>>,
}

impl CommandRequest {
    fn name(&self) -> String {
        String::from_utf8_lossy(&self.args[0]).to_ascii_uppercase()
    }
}

#[derive(Debug)]
enum RequestFrame {
    Resp2(Resp2Frame),
    Resp3(Resp3Frame),
}

#[derive(Debug)]
enum Response {
    SimpleString(String),
    Error(String),
    Bulk(Vec<u8>),
    Null,
    Integer(i64),
    Array(Vec<Response>),
    Hello { proto: i64 },
}

#[derive(Debug)]
struct CommandOutcome {
    response: Response,
    protocol: Option<Protocol>,
    close: bool,
}

/// Bind and run the RESP listener until the process is interrupted.
pub async fn run_listener(config: NimbusKvConfig) -> Result<(), KvError> {
    refuse_non_loopback_bind(config.bind_addr)?;
    let listener = TcpListener::bind(config.bind_addr).await?;
    serve(listener, config).await
}

/// Serve an already-bound listener.
pub async fn serve(listener: TcpListener, config: NimbusKvConfig) -> Result<(), KvError> {
    refuse_non_loopback_bind(listener.local_addr()?)?;
    let credentials = Arc::new(config.credentials);
    let store = config
        .store
        .unwrap_or(NimbusKvStore::no_disk(TieringConfig::no_disk())?);
    let metrics = config.metrics.unwrap_or_else(|| store.metrics());

    loop {
        let (stream, peer_addr) = listener.accept().await?;
        let credentials = Arc::clone(&credentials);
        let store = store.clone();
        let metrics = metrics.clone();
        tokio::spawn(async move {
            if let Err(error) = handle_connection(stream, credentials, store, metrics).await {
                tracing::warn!(%peer_addr, %error, "nimbus-kv connection ended with error");
            }
        });
    }
}

async fn handle_connection(
    mut stream: TcpStream,
    credentials: Arc<CredentialRegistry>,
    store: NimbusKvStore,
    metrics: NimbusKvMetrics,
) -> Result<(), KvError> {
    let _client = metrics.client_connected();
    let mut state = ConnectionState::default();
    let mut buffer = Vec::new();
    let mut read_buf = [0_u8; 4096];

    loop {
        while let Some(frame) = decode_next_frame(state.protocol, &mut buffer)? {
            let outcome = match parse_command(frame) {
                Ok(command) => {
                    let name = command.name();
                    let started_at = Instant::now();
                    let outcome =
                        execute_command(command, &mut state, &credentials, &store, &metrics);
                    metrics.record_command(
                        &name,
                        started_at.elapsed(),
                        matches!(outcome.response, Response::Error(_)),
                    );
                    outcome
                }
                Err(response) => CommandOutcome {
                    response,
                    protocol: None,
                    close: false,
                },
            };
            if let Some(protocol) = outcome.protocol {
                state.protocol = protocol;
            }
            let response = encode_response(&outcome.response, state.protocol)?;
            stream.write_all(&response).await?;
            if outcome.close {
                return Ok(());
            }
        }

        let read = stream.read(&mut read_buf).await?;
        if read == 0 {
            return Ok(());
        }
        buffer.extend_from_slice(&read_buf[..read]);
        if buffer.len() > MAX_FRAME_BYTES {
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!("RESP frame exceeds {MAX_FRAME_BYTES} bytes"),
            )
            .into());
        }
    }
}

fn decode_next_frame(
    protocol: Protocol,
    buffer: &mut Vec<u8>,
) -> Result<Option<RequestFrame>, KvError> {
    match protocol {
        Protocol::Resp2 => {
            let Some((frame, consumed)) = resp2_decode::decode(buffer)? else {
                return Ok(None);
            };
            buffer.drain(..consumed);
            Ok(Some(RequestFrame::Resp2(frame)))
        }
        Protocol::Resp3 => {
            let Some((frame, consumed)) = resp3_decode::decode(buffer)? else {
                return Ok(None);
            };
            buffer.drain(..consumed);
            Ok(Some(RequestFrame::Resp3(frame)))
        }
    }
}

fn parse_command(frame: RequestFrame) -> Result<CommandRequest, Response> {
    let args = match frame {
        RequestFrame::Resp2(frame) => parse_resp2_args(frame),
        RequestFrame::Resp3(frame) => parse_resp3_args(frame),
    }?;
    if args.is_empty() {
        return Err(Response::Error("ERR empty command".to_owned()));
    }
    Ok(CommandRequest { args })
}

fn parse_resp2_args(frame: Resp2Frame) -> Result<Vec<Vec<u8>>, Response> {
    match frame {
        Resp2Frame::Array(frames) => frames
            .into_iter()
            .map(resp2_frame_bytes)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(protocol_error),
        _ => Err(protocol_error()),
    }
}

fn resp2_frame_bytes(frame: Resp2Frame) -> Option<Vec<u8>> {
    match frame {
        Resp2Frame::BulkString(data) | Resp2Frame::SimpleString(data) => Some(data),
        Resp2Frame::Integer(value) => Some(value.to_string().into_bytes()),
        _ => None,
    }
}

fn parse_resp3_args(frame: Resp3Frame) -> Result<Vec<Vec<u8>>, Response> {
    match frame {
        Resp3Frame::Array { data, .. } => data
            .into_iter()
            .map(resp3_frame_bytes)
            .collect::<Option<Vec<_>>>()
            .ok_or_else(protocol_error),
        Resp3Frame::Hello { version, auth, .. } => {
            let mut args = vec![
                b"HELLO".to_vec(),
                match version {
                    RespVersion::RESP2 => b"2".to_vec(),
                    RespVersion::RESP3 => b"3".to_vec(),
                },
            ];
            if let Some((username, password)) = auth {
                args.push(b"AUTH".to_vec());
                args.push(username.into_bytes());
                args.push(password.into_bytes());
            }
            Ok(args)
        }
        _ => Err(protocol_error()),
    }
}

fn resp3_frame_bytes(frame: Resp3Frame) -> Option<Vec<u8>> {
    match frame {
        Resp3Frame::BlobString { data, .. }
        | Resp3Frame::SimpleString { data, .. }
        | Resp3Frame::BigNumber { data, .. }
        | Resp3Frame::ChunkedString(data) => Some(data),
        Resp3Frame::Number { data, .. } => Some(data.to_string().into_bytes()),
        _ => None,
    }
}

fn protocol_error() -> Response {
    Response::Error("ERR Protocol error: expected an array command frame".to_owned())
}

fn execute_command(
    command: CommandRequest,
    state: &mut ConnectionState,
    credentials: &CredentialRegistry,
    store: &NimbusKvStore,
    metrics: &NimbusKvMetrics,
) -> CommandOutcome {
    let name = command.name();
    match name.as_str() {
        "AUTH" => auth_command(&command.args, state, credentials),
        "HELLO" => hello_command(&command.args, state, credentials),
        "QUIT" if state.binding.is_some() => CommandOutcome {
            response: Response::SimpleString("OK".to_owned()),
            protocol: None,
            close: true,
        },
        "QUIT" => noauth(),
        _ if state.binding.is_none() => noauth(),
        "PING" => ping_command(&command.args),
        "ECHO" => echo_command(&command.args),
        "COMMAND" => CommandOutcome {
            response: Response::Array(Vec::new()),
            protocol: None,
            close: false,
        },
        "CLIENT" => client_command(&command.args),
        "SELECT" => select_command(&command.args, state),
        "GET" => get_command(&command.args, store),
        "SET" => set_command(&command.args, store),
        "DEL" => del_command(&command.args, store),
        "FLUSHALL" => flushall_command(&command.args, store),
        "FUNCTION" => function_command(&command.args),
        "EXPIRE" => expire_command(&command.args, store),
        "TTL" => ttl_command(&command.args, store),
        "INCR" => incr_command(&command.args, store),
        "NIMBUS.READY" => ready_command(&command.args),
        "NIMBUS.METRICS" => metrics_command(&command.args, metrics),
        _ => CommandOutcome {
            response: Response::Error(format!("ERR unknown command '{name}'")),
            protocol: None,
            close: false,
        },
    }
}

fn get_command(args: &[Vec<u8>], store: &NimbusKvStore) -> CommandOutcome {
    let response = match args {
        [_, key] => match store.get(key, now_ms()) {
            Ok(Some(value)) => Response::Bulk(value),
            Ok(None) => Response::Null,
            Err(error) => storage_error(error),
        },
        _ => Response::Error("ERR wrong number of arguments for 'GET'".to_owned()),
    };
    command_response(response)
}

fn set_command(args: &[Vec<u8>], store: &NimbusKvStore) -> CommandOutcome {
    let response = match args {
        [_, key, value] => match store.set(key.clone(), value.clone(), None) {
            Ok(()) => Response::SimpleString("OK".to_owned()),
            Err(error) => storage_error(error),
        },
        [_, key, value, option, ttl] if option.eq_ignore_ascii_case(b"EX") => {
            match parse_expire_seconds(ttl)
                .and_then(|seconds| expire_at_from_now(seconds, now_ms()))
            {
                Ok(expire_at_ms) => match store.set(key.clone(), value.clone(), Some(expire_at_ms))
                {
                    Ok(()) => Response::SimpleString("OK".to_owned()),
                    Err(error) => storage_error(error),
                },
                Err(response) => response,
            }
        }
        [_, key, value, option, ttl] if option.eq_ignore_ascii_case(b"PX") => {
            match parse_expire_millis(ttl)
                .and_then(|millis| expire_at_ms_from_now(millis, now_ms()))
            {
                Ok(expire_at_ms) => match store.set(key.clone(), value.clone(), Some(expire_at_ms))
                {
                    Ok(()) => Response::SimpleString("OK".to_owned()),
                    Err(error) => storage_error(error),
                },
                Err(response) => response,
            }
        }
        _ => Response::Error("ERR syntax error".to_owned()),
    };
    command_response(response)
}

fn del_command(args: &[Vec<u8>], store: &NimbusKvStore) -> CommandOutcome {
    if args.len() < 2 {
        return command_response(Response::Error(
            "ERR wrong number of arguments for 'DEL'".to_owned(),
        ));
    }
    let mut deleted = 0_i64;
    for key in &args[1..] {
        match store.delete(key) {
            Ok(true) => deleted += 1,
            Ok(false) => {}
            Err(error) => return command_response(storage_error(error)),
        }
    }
    command_response(Response::Integer(deleted))
}

fn flushall_command(args: &[Vec<u8>], store: &NimbusKvStore) -> CommandOutcome {
    let response = match args {
        [_] => match store.flush_all(now_ms()) {
            Ok(_) => Response::SimpleString("OK".to_owned()),
            Err(error) => storage_error(error),
        },
        [_, option]
            if option.eq_ignore_ascii_case(b"SYNC") || option.eq_ignore_ascii_case(b"ASYNC") =>
        {
            match store.flush_all(now_ms()) {
                Ok(_) => Response::SimpleString("OK".to_owned()),
                Err(error) => storage_error(error),
            }
        }
        _ => Response::Error("ERR wrong number of arguments for 'FLUSHALL'".to_owned()),
    };
    command_response(response)
}

fn function_command(args: &[Vec<u8>]) -> CommandOutcome {
    let response = match args {
        [_, subcommand] if subcommand.eq_ignore_ascii_case(b"FLUSH") => {
            Response::SimpleString("OK".to_owned())
        }
        [_, subcommand, option]
            if subcommand.eq_ignore_ascii_case(b"FLUSH")
                && (option.eq_ignore_ascii_case(b"SYNC")
                    || option.eq_ignore_ascii_case(b"ASYNC")) =>
        {
            Response::SimpleString("OK".to_owned())
        }
        _ => Response::Error("ERR unsupported FUNCTION subcommand".to_owned()),
    };
    command_response(response)
}

fn ready_command(args: &[Vec<u8>]) -> CommandOutcome {
    let response = match args {
        [_] => Response::SimpleString("READY".to_owned()),
        _ => Response::Error("ERR wrong number of arguments for 'NIMBUS.READY'".to_owned()),
    };
    command_response(response)
}

fn metrics_command(args: &[Vec<u8>], metrics: &NimbusKvMetrics) -> CommandOutcome {
    let response = match args {
        [_] => Response::Bulk(metrics.render_text().into_bytes()),
        _ => Response::Error("ERR wrong number of arguments for 'NIMBUS.METRICS'".to_owned()),
    };
    command_response(response)
}

fn expire_command(args: &[Vec<u8>], store: &NimbusKvStore) -> CommandOutcome {
    let response = match args {
        [_, key, seconds] => match parse_expire_seconds(seconds)
            .and_then(|seconds| expire_at_from_now(seconds, now_ms()))
        {
            Ok(expire_at_ms) => match store.expire(key, expire_at_ms, now_ms()) {
                Ok(true) => Response::Integer(1),
                Ok(false) => Response::Integer(0),
                Err(error) => storage_error(error),
            },
            Err(response) => response,
        },
        _ => Response::Error("ERR wrong number of arguments for 'EXPIRE'".to_owned()),
    };
    command_response(response)
}

fn ttl_command(args: &[Vec<u8>], store: &NimbusKvStore) -> CommandOutcome {
    let response = match args {
        [_, key] => match store.ttl(key, now_ms()) {
            Ok(ttl) => Response::Integer(ttl),
            Err(error) => storage_error(error),
        },
        _ => Response::Error("ERR wrong number of arguments for 'TTL'".to_owned()),
    };
    command_response(response)
}

fn incr_command(args: &[Vec<u8>], store: &NimbusKvStore) -> CommandOutcome {
    let response = match args {
        [_, key] => match store.incr(key, now_ms()) {
            Ok(value) => Response::Integer(value),
            Err(error) => storage_error(error),
        },
        _ => Response::Error("ERR wrong number of arguments for 'INCR'".to_owned()),
    };
    command_response(response)
}

fn auth_command(
    args: &[Vec<u8>],
    state: &mut ConnectionState,
    credentials: &CredentialRegistry,
) -> CommandOutcome {
    let authenticated = match args {
        [_, password] => credentials.authenticate(None, &String::from_utf8_lossy(password)),
        [_, username, password] => credentials.authenticate(
            Some(&String::from_utf8_lossy(username)),
            &String::from_utf8_lossy(password),
        ),
        _ => {
            return CommandOutcome {
                response: Response::Error("ERR wrong number of arguments for 'AUTH'".to_owned()),
                protocol: None,
                close: false,
            };
        }
    };

    match authenticated {
        Some(binding) => {
            state.binding = Some(binding);
            CommandOutcome {
                response: Response::SimpleString("OK".to_owned()),
                protocol: None,
                close: false,
            }
        }
        None => CommandOutcome {
            response: Response::Error(
                "WRONGPASS invalid username-password pair or user is disabled".to_owned(),
            ),
            protocol: None,
            close: false,
        },
    }
}

fn hello_command(
    args: &[Vec<u8>],
    state: &mut ConnectionState,
    credentials: &CredentialRegistry,
) -> CommandOutcome {
    let Some(version) = args.get(1).and_then(|arg| std::str::from_utf8(arg).ok()) else {
        return CommandOutcome {
            response: Response::Error("ERR HELLO requires RESP version".to_owned()),
            protocol: None,
            close: false,
        };
    };
    let protocol = match version {
        "2" => Protocol::Resp2,
        "3" => Protocol::Resp3,
        _ => {
            return CommandOutcome {
                response: Response::Error("NOPROTO unsupported RESP protocol version".to_owned()),
                protocol: None,
                close: false,
            };
        }
    };

    if args.len() > 2 {
        match parse_hello_auth(args, credentials) {
            Ok(binding) => state.binding = Some(binding),
            Err(response) => {
                return CommandOutcome {
                    response,
                    protocol: None,
                    close: false,
                };
            }
        }
    }

    if state.binding.is_none() {
        return noauth();
    }

    CommandOutcome {
        response: Response::Hello {
            proto: if protocol == Protocol::Resp3 { 3 } else { 2 },
        },
        protocol: Some(protocol),
        close: false,
    }
}

fn parse_hello_auth(
    args: &[Vec<u8>],
    credentials: &CredentialRegistry,
) -> Result<CredentialBinding, Response> {
    if args.len() != 5 || !args[2].eq_ignore_ascii_case(b"AUTH") {
        return Err(Response::Error(
            "ERR HELLO only supports AUTH username password".to_owned(),
        ));
    }
    credentials
        .authenticate(
            Some(&String::from_utf8_lossy(&args[3])),
            &String::from_utf8_lossy(&args[4]),
        )
        .ok_or_else(|| {
            Response::Error(
                "WRONGPASS invalid username-password pair or user is disabled".to_owned(),
            )
        })
}

fn ping_command(args: &[Vec<u8>]) -> CommandOutcome {
    let response = match args {
        [_] => Response::SimpleString("PONG".to_owned()),
        [_, payload] => Response::Bulk(payload.clone()),
        _ => Response::Error("ERR wrong number of arguments for 'PING'".to_owned()),
    };
    CommandOutcome {
        response,
        protocol: None,
        close: false,
    }
}

fn echo_command(args: &[Vec<u8>]) -> CommandOutcome {
    let response = match args {
        [_, payload] => Response::Bulk(payload.clone()),
        _ => Response::Error("ERR wrong number of arguments for 'ECHO'".to_owned()),
    };
    CommandOutcome {
        response,
        protocol: None,
        close: false,
    }
}

fn client_command(args: &[Vec<u8>]) -> CommandOutcome {
    let response = if args.len() >= 2 && args[1].eq_ignore_ascii_case(b"SETINFO") {
        Response::SimpleString("OK".to_owned())
    } else {
        Response::Error("ERR unsupported CLIENT subcommand".to_owned())
    };
    CommandOutcome {
        response,
        protocol: None,
        close: false,
    }
}

fn select_command(args: &[Vec<u8>], state: &ConnectionState) -> CommandOutcome {
    let response = match (args, &state.binding) {
        ([_, db], Some(binding))
            if db == b"0" || db.as_slice() == binding.tenant.as_str().as_bytes() =>
        {
            Response::SimpleString("OK".to_owned())
        }
        ([_, _], Some(_)) => Response::Error(
            "ERR SELECT cannot change tenant from the credential-bound tenant".to_owned(),
        ),
        _ => Response::Error("ERR wrong number of arguments for 'SELECT'".to_owned()),
    };
    CommandOutcome {
        response,
        protocol: None,
        close: false,
    }
}

fn noauth() -> CommandOutcome {
    CommandOutcome {
        response: Response::Error("NOAUTH Authentication required".to_owned()),
        protocol: None,
        close: false,
    }
}

fn command_response(response: Response) -> CommandOutcome {
    CommandOutcome {
        response,
        protocol: None,
        close: false,
    }
}

fn storage_error(error: KvError) -> Response {
    Response::Error(format!("ERR {error}"))
}

fn now_ms() -> i64 {
    let millis = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or(0);
    i64::try_from(millis).unwrap_or(i64::MAX)
}

fn parse_expire_seconds(value: &[u8]) -> Result<i64, Response> {
    let value = std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Response::Error("ERR invalid expire time".to_owned()))?;
    value
        .checked_mul(1_000)
        .ok_or_else(|| Response::Error("ERR invalid expire time".to_owned()))
}

fn parse_expire_millis(value: &[u8]) -> Result<i64, Response> {
    std::str::from_utf8(value)
        .ok()
        .and_then(|value| value.parse::<i64>().ok())
        .filter(|value| *value > 0)
        .ok_or_else(|| Response::Error("ERR invalid expire time".to_owned()))
}

fn expire_at_from_now(ttl_ms: i64, now_ms: i64) -> Result<i64, Response> {
    expire_at_ms_from_now(ttl_ms, now_ms)
}

fn expire_at_ms_from_now(ttl_ms: i64, now_ms: i64) -> Result<i64, Response> {
    now_ms
        .checked_add(ttl_ms)
        .ok_or_else(|| Response::Error("ERR invalid expire time".to_owned()))
}

fn encode_response(response: &Response, protocol: Protocol) -> Result<Vec<u8>, KvError> {
    match protocol {
        Protocol::Resp2 => encode_resp2(response),
        Protocol::Resp3 => encode_resp3(response),
    }
}

fn encode_resp2(response: &Response) -> Result<Vec<u8>, KvError> {
    let frame = response.to_resp2();
    let mut buf = vec![0_u8; frame.encode_len(false)];
    let written = resp2_encode::encode(&mut buf, &frame, false)?;
    buf.truncate(written);
    Ok(buf)
}

fn encode_resp3(response: &Response) -> Result<Vec<u8>, KvError> {
    let frame = response.to_resp3();
    let mut buf = vec![0_u8; frame.encode_len(false)];
    let written = resp3_encode::encode(&mut buf, &frame, false)?;
    buf.truncate(written);
    Ok(buf)
}

impl Response {
    fn to_resp2(&self) -> Resp2Frame {
        match self {
            Self::SimpleString(data) => Resp2Frame::SimpleString(data.as_bytes().to_vec()),
            Self::Error(data) => Resp2Frame::Error(data.clone()),
            Self::Bulk(data) => Resp2Frame::BulkString(data.clone()),
            Self::Null => Resp2Frame::Null,
            Self::Integer(data) => Resp2Frame::Integer(*data),
            Self::Array(items) => Resp2Frame::Array(items.iter().map(Self::to_resp2).collect()),
            Self::Hello { proto } => Resp2Frame::Array(vec![
                Resp2Frame::BulkString(b"server".to_vec()),
                Resp2Frame::BulkString(b"nimbus-kv".to_vec()),
                Resp2Frame::BulkString(b"proto".to_vec()),
                Resp2Frame::Integer(*proto),
            ]),
        }
    }

    fn to_resp3(&self) -> Resp3Frame {
        match self {
            Self::SimpleString(data) => Resp3Frame::SimpleString {
                data: data.as_bytes().to_vec(),
                attributes: None,
            },
            Self::Error(data) => Resp3Frame::SimpleError {
                data: data.clone(),
                attributes: None,
            },
            Self::Bulk(data) => Resp3Frame::BlobString {
                data: data.clone(),
                attributes: None,
            },
            Self::Null => Resp3Frame::Null,
            Self::Integer(data) => Resp3Frame::Number {
                data: *data,
                attributes: None,
            },
            Self::Array(items) => Resp3Frame::Array {
                data: items.iter().map(Self::to_resp3).collect(),
                attributes: None,
            },
            Self::Hello { proto } => {
                let mut data = FrameMap::new();
                data.insert(resp3_blob("server"), resp3_blob("nimbus-kv"));
                data.insert(resp3_blob("version"), resp3_blob(env!("CARGO_PKG_VERSION")));
                data.insert(
                    resp3_blob("proto"),
                    Resp3Frame::Number {
                        data: *proto,
                        attributes: None,
                    },
                );
                data.insert(resp3_blob("mode"), resp3_blob("standalone"));
                data.insert(resp3_blob("role"), resp3_blob("master"));
                Resp3Frame::Map {
                    data,
                    attributes: None,
                }
            }
        }
    }
}

fn resp3_blob(data: &str) -> Resp3Frame {
    Resp3Frame::BlobString {
        data: data.as_bytes().to_vec(),
        attributes: None,
    }
}

fn generate_dev_password() -> String {
    let mut bytes = [0_u8; 24];
    OsRng.fill_bytes(&mut bytes);
    hex::encode(bytes)
}
