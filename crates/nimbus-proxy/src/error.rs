use std::error::Error as StdError;
use std::fmt;
use std::io;
use std::net::SocketAddr;

pub type Result<T> = std::result::Result<T, EgressProxyError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressProxyError {
    BindFailed {
        address: SocketAddr,
        kind: io::ErrorKind,
    },
    OperationFailed {
        message: String,
    },
}

impl fmt::Display for EgressProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::BindFailed { address, kind } => {
                write!(
                    formatter,
                    "failed to bind egress proxy on {address}: {kind}"
                )
            }
            Self::OperationFailed { message } => formatter.write_str(message),
        }
    }
}

impl StdError for EgressProxyError {}
