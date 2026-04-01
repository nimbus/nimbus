use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, Ordering};

use serde::{Deserialize, Serialize};
use serde_json::Value;
use tokio::sync::Notify;

use crate::error::{NeovexRuntimeError, Result};

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct HostCallRequest {
    pub operation: String,
    #[serde(default)]
    pub payload: Value,
}

pub type HostBridgeFuture = Pin<Box<dyn Future<Output = Result<Value>> + Send + 'static>>;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum HostCallCancellationCause {
    Explicit,
    Disconnect,
}

#[derive(Debug, Clone, Default)]
pub struct HostCallCancellation {
    inner: Arc<HostCallCancellationState>,
}

#[derive(Debug, Default)]
struct HostCallCancellationState {
    canceled: AtomicBool,
    cause: AtomicU8,
    notify: Notify,
}

impl HostCallCancellation {
    pub fn cancel(&self) {
        self.cancel_with_cause(HostCallCancellationCause::Explicit);
    }

    pub fn cancel_due_to_disconnect(&self) {
        self.cancel_with_cause(HostCallCancellationCause::Disconnect);
    }

    pub fn cause(&self) -> Option<HostCallCancellationCause> {
        HostCallCancellationCause::from_u8(self.inner.cause.load(Ordering::SeqCst))
    }

    fn cancel_with_cause(&self, cause: HostCallCancellationCause) {
        let _ =
            self.inner
                .cause
                .compare_exchange(0, cause.as_u8(), Ordering::SeqCst, Ordering::SeqCst);
        self.inner.canceled.store(true, Ordering::SeqCst);
        self.inner.notify.notify_waiters();
    }

    pub fn is_cancelled(&self) -> bool {
        self.inner.canceled.load(Ordering::SeqCst)
    }

    pub async fn cancelled(&self) {
        if self.is_cancelled() {
            return;
        }
        self.inner.notify.notified().await;
    }
}

impl HostCallCancellationCause {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::Disconnect => "disconnect",
        }
    }

    fn as_u8(self) -> u8 {
        match self {
            Self::Explicit => 1,
            Self::Disconnect => 2,
        }
    }

    fn from_u8(value: u8) -> Option<Self> {
        match value {
            1 => Some(Self::Explicit),
            2 => Some(Self::Disconnect),
            _ => None,
        }
    }
}

pub trait HostBridge: Send + Sync + 'static {
    fn call(&self, request: HostCallRequest) -> Result<Value>;

    fn call_cancellable(
        &self,
        request: HostCallRequest,
        cancellation: &HostCallCancellation,
    ) -> Result<Value> {
        if cancellation.is_cancelled() {
            return Err(NeovexRuntimeError::Cancelled);
        }
        self.call(request)
    }

    fn call_async(
        &self,
        request: HostCallRequest,
        cancellation: HostCallCancellation,
    ) -> HostBridgeFuture {
        let result = self.call_cancellable(request, &cancellation);
        Box::pin(async move { result })
    }
}
