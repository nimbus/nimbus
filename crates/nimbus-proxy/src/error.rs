use std::error::Error as StdError;
use std::fmt;

pub type Result<T> = std::result::Result<T, EgressProxyError>;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum EgressProxyError {
    OperationFailed { message: String },
}

impl fmt::Display for EgressProxyError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::OperationFailed { message } => formatter.write_str(message),
        }
    }
}

impl StdError for EgressProxyError {}
