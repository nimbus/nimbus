mod extension;
pub mod firebase_admin;

pub use self::extension::{
    dispatch_runtime_extension_call, dispatch_runtime_extension_call_async,
    dispatch_runtime_extension_call_cancellable,
};
