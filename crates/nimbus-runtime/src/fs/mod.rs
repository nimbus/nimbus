use std::fmt;

use deno_fs::sync::MaybeArc;

/// Runtime-local filesystem backend seam for in-process V8 filesystem access.
///
/// This trait intentionally lives in `nimbus-runtime` because that crate speaks
/// Deno's `FileSystemRc` ABI. Implementations live outside `nimbus-runtime` so
/// the runtime keeps its zero-workspace-dependency invariant.
pub trait NimbusFsBackend: deno_fs::FileSystem {}

impl<T> NimbusFsBackend for T where T: deno_fs::FileSystem + ?Sized {}

#[derive(Clone)]
pub struct RuntimeFileSystem {
    inner: deno_fs::FileSystemRc,
}

impl RuntimeFileSystem {
    pub fn new(inner: deno_fs::FileSystemRc) -> Self {
        Self { inner }
    }

    pub fn clone_inner(&self) -> deno_fs::FileSystemRc {
        self.inner.clone()
    }
}

impl Default for RuntimeFileSystem {
    fn default() -> Self {
        Self::new(MaybeArc::new(deno_fs::RealFs))
    }
}

impl fmt::Debug for RuntimeFileSystem {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("RuntimeFileSystem").finish_non_exhaustive()
    }
}
