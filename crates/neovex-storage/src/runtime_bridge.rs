use neovex_core::{Error, Result};
use tokio::runtime::{Handle as TokioRuntimeHandle, RuntimeFlavor};

pub(crate) fn bridge_tokio_runtime<T, F>(
    handle: &TokioRuntimeHandle,
    thread_panic_message: &'static str,
    task: F,
) -> Result<T>
where
    T: Send,
    F: FnOnce() -> Result<T> + Send,
{
    if TokioRuntimeHandle::try_current().is_ok() {
        return match handle.runtime_flavor() {
            RuntimeFlavor::MultiThread => tokio::task::block_in_place(task),
            RuntimeFlavor::CurrentThread | _ => std::thread::scope(|scope| {
                scope
                    .spawn(task)
                    .join()
                    .map_err(|_| Error::Internal(thread_panic_message.to_string()))
            })?,
        };
    }

    task()
}

pub(crate) fn bridge_tokio_runtime_local<T, F>(
    handle: &TokioRuntimeHandle,
    current_thread_runtime_message: &'static str,
    task: F,
) -> Result<T>
where
    F: FnOnce() -> Result<T>,
{
    if TokioRuntimeHandle::try_current().is_ok() {
        return match handle.runtime_flavor() {
            RuntimeFlavor::MultiThread => tokio::task::block_in_place(task),
            RuntimeFlavor::CurrentThread | _ => {
                Err(Error::Internal(current_thread_runtime_message.to_string()))
            }
        };
    }

    task()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bridge_tokio_runtime_stays_on_current_thread_for_multi_thread_runtime() {
        let runtime_thread = std::thread::current().id();
        let handle = TokioRuntimeHandle::current();

        let bridged_thread =
            bridge_tokio_runtime(&handle, "bridge helper should not panic", || {
                Ok(std::thread::current().id())
            })
            .expect("bridge helper should return the current thread id");

        assert_eq!(
            bridged_thread, runtime_thread,
            "multi-thread runtime bridging should use block_in_place instead of spawning a new thread"
        );
    }

    #[test]
    fn bridge_tokio_runtime_spawns_a_fallback_thread_for_current_thread_runtimes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime should build");

        let (runtime_thread, bridged_thread) = runtime.block_on(async {
            let runtime_thread = std::thread::current().id();
            let handle = TokioRuntimeHandle::current();
            let bridged_thread =
                bridge_tokio_runtime(&handle, "bridge helper should not panic", || {
                    Ok(std::thread::current().id())
                })
                .expect("bridge helper should return a fallback thread id");
            (runtime_thread, bridged_thread)
        });

        assert_ne!(
            bridged_thread, runtime_thread,
            "current-thread runtimes should keep using the dedicated bridge-thread fallback"
        );
    }

    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn bridge_tokio_runtime_local_stays_on_current_thread_for_multi_thread_runtime() {
        let runtime_thread = std::thread::current().id();
        let handle = TokioRuntimeHandle::current();

        let bridged_thread =
            bridge_tokio_runtime_local(&handle, "local bridge helper should not panic", || {
                Ok(std::thread::current().id())
            })
            .expect("local bridge helper should return the current thread id");

        assert_eq!(
            bridged_thread, runtime_thread,
            "multi-thread local bridging should use block_in_place instead of spawning a new thread"
        );
    }

    #[test]
    fn bridge_tokio_runtime_local_rejects_current_thread_runtimes() {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .expect("current-thread runtime should build");

        let error = runtime
            .block_on(async {
                let handle = TokioRuntimeHandle::current();
                bridge_tokio_runtime_local(
                    &handle,
                    "local bridge requires a multi-thread runtime",
                    || Ok::<_, Error>(()),
                )
            })
            .expect_err("local bridge should reject current-thread runtimes");

        assert!(
            matches!(
                error,
                Error::Internal(ref message)
                    if message == "local bridge requires a multi-thread runtime"
            ),
            "expected a clear current-thread runtime error, got {error:?}"
        );
    }
}
