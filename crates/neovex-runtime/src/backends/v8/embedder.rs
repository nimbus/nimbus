//! V8 embedder namespace for the current `deno_core` integration.
//!
//! This is intentionally a namespacing seam, not a fake abstraction layer.
//! We still use `deno_core` directly, but routing those imports through the
//! V8 backend boundary keeps generic runtime modules from naming the embedder
//! crate directly.

pub(crate) use deno_core::error::JsError;
pub(crate) use deno_core::{
    CancelFuture, CancelHandle, Extension, JsRuntime, JsRuntimeForSnapshot, ModuleLoadOptions,
    ModuleLoadReferrer, ModuleLoadResponse, ModuleLoader, ModuleSource, ModuleSourceCode,
    ModuleSpecifier, ModuleType, OpState, PollEventLoopOptions, RequestedModuleType,
    ResolutionKind, RuntimeOptions, SourceCodeCacheInfo, extension, op2, resolve_import, scope,
    serde_v8, v8,
};
pub(crate) use deno_error::JsErrorBox;
