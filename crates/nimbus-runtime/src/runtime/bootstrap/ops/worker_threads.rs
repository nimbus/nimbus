use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicU64, Ordering};

use deno_core::DetachedBuffer;
use deno_core::JsBuffer;
use deno_core::futures::channel::mpsc::{
    TryRecvError, UnboundedReceiver, UnboundedSender, unbounded,
};
use deno_core::futures::{StreamExt, future::poll_fn};
use deno_web::{
    JsMessageData, MessagePort, MessagePortError, RecvMessageData, create_entangled_message_port,
};
use serde::Deserialize;
use serde_json::json;
use tempfile::TempDir;

use crate::backends::v8::V8RuntimeConstructionMode;
use crate::backends::v8::embedder::{Extension, JsErrorBox, OpState, op2};
use crate::runtime::bootstrap::state::{
    InstalledRuntimeCapabilityPolicy, InstalledRuntimeContract, InstalledRuntimeOwner,
    InstalledRuntimeWorkerBootstrapState, RuntimeSharedWorkerEnv, RuntimeWorkerBootstrapDescriptor,
    install_missing_deno_extension_state,
};
use crate::runtime::{NimbusRuntime, RuntimeBundle};

static WORKER_ID_COUNTER: AtomicU32 = AtomicU32::new(1);

type WorkersTable = HashMap<u32, WorkerThreadEntry>;

#[derive(Deserialize, Default)]
#[serde(rename_all = "camelCase")]
struct WorkerResourceLimits {
    #[serde(default)]
    stack_size_mb: Option<usize>,
}

#[derive(Deserialize)]
#[serde(rename_all = "camelCase")]
struct CreateWorkerArgs {
    has_source_code: bool,
    #[serde(default)]
    source_code: String,
    specifier: String,
    #[serde(default)]
    close_on_idle: bool,
    #[serde(default)]
    resource_limits: Option<WorkerResourceLimits>,
}

struct WorkerThreadEntry {
    port: Rc<MessagePort>,
    ctrl_receiver: Rc<RefCell<UnboundedReceiver<WorkerControlEvent>>>,
    cpu_thread_handle: Arc<AtomicU64>,
}

enum WorkerControlEvent {
    TerminalError {
        message: String,
        name: String,
        exit_code: i32,
    },
    Close(i32),
}

fn default_worker_bootstrap_descriptor() -> RuntimeWorkerBootstrapDescriptor {
    RuntimeWorkerBootstrapDescriptor {
        running_on_main_thread: true,
        worker_id: 0,
        close_on_idle: false,
        module_specifier: None,
        worker_metadata: None,
    }
}

impl serde::Serialize for WorkerControlEvent {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        match self {
            Self::TerminalError {
                message,
                name,
                exit_code,
            } => serde::Serialize::serialize(
                &(
                    1_i32,
                    json!({
                        "message": message,
                        "name": name,
                        "errorMessage": message,
                        "exitCode": exit_code,
                    }),
                ),
                serializer,
            ),
            Self::Close(exit_code) => serde::Serialize::serialize(&(3_i32, exit_code), serializer),
        }
    }
}

#[op2]
#[serde]
pub(super) fn op_nimbus_worker_bootstrap_state(
    state: &mut OpState,
) -> RuntimeWorkerBootstrapDescriptor {
    let Some(bootstrap_state) = state.try_borrow_mut::<InstalledRuntimeWorkerBootstrapState>()
    else {
        return default_worker_bootstrap_descriptor();
    };
    RuntimeWorkerBootstrapDescriptor {
        running_on_main_thread: bootstrap_state.descriptor.running_on_main_thread,
        worker_id: bootstrap_state.descriptor.worker_id,
        close_on_idle: bootstrap_state.descriptor.close_on_idle,
        module_specifier: bootstrap_state.descriptor.module_specifier.clone(),
        worker_metadata: bootstrap_state.descriptor.worker_metadata.take(),
    }
}

#[op2]
pub(super) fn op_nimbus_worker_parent_post_message(
    state: &mut OpState,
    #[serde] data: JsMessageData,
) -> Result<(), MessagePortError> {
    let port = worker_parent_port(state)?;
    port.send(state, data)
}

#[op2]
pub(super) fn op_nimbus_worker_parent_post_message_raw(
    state: &mut OpState,
    #[buffer(detach)] data: JsBuffer,
) -> Result<(), MessagePortError> {
    let port = worker_parent_port(state)?;
    let detached = DetachedBuffer::from_v8slice(data.into_parts());
    send_raw_message_to_port(port.as_ref(), detached, "worker parent")
}

#[op2]
pub(super) async fn op_nimbus_worker_parent_recv_message(
    state: Rc<RefCell<OpState>>,
) -> Result<Option<RecvMessageData>, MessagePortError> {
    let port = {
        let state = state.borrow();
        state
            .try_borrow::<InstalledRuntimeWorkerBootstrapState>()
            .and_then(|bootstrap_state| bootstrap_state.parent_port.clone())
    };
    match port {
        Some(port) => port.recv(state).await,
        None => Ok(None),
    }
}

#[op2]
#[serde]
pub(super) fn op_nimbus_worker_parent_recv_message_sync(
    state: &mut OpState,
) -> Result<Option<JsMessageData>, MessagePortError> {
    let port = state
        .try_borrow::<InstalledRuntimeWorkerBootstrapState>()
        .and_then(|bootstrap_state| bootstrap_state.parent_port.clone());
    match port {
        Some(port) => port.try_recv_sync(state),
        None => Ok(None),
    }
}

#[op2(reentrant)]
#[smi]
pub(super) fn op_create_worker(
    state: &mut OpState,
    #[serde] args: CreateWorkerArgs,
    #[serde] maybe_worker_metadata: Option<JsMessageData>,
) -> Result<u32, JsErrorBox> {
    if state
        .borrow::<InstalledRuntimeContract>()
        .limits
        .grants
        .worker
        .iter()
        .all(|grant| grant != "thread")
    {
        return Err(JsErrorBox::generic(
            "runtime worker grant denied for `thread`",
        ));
    }

    let worker_id = WORKER_ID_COUNTER.fetch_add(1, Ordering::SeqCst);
    let runtime_owner = state.borrow::<InstalledRuntimeOwner>().runtime.clone();
    let worker_cwd = state
        .borrow::<InstalledRuntimeCapabilityPolicy>()
        .paths
        .cwd()
        .to_path_buf();
    let shared_env = state.borrow::<RuntimeSharedWorkerEnv>().clone();
    let (tempdir, bundle, module_specifier) = prepare_worker_bundle(&args, &worker_cwd)?;
    let (parent_port, child_port) = create_entangled_message_port();
    let parent_port = Rc::new(parent_port);
    let worker_bootstrap_descriptor = RuntimeWorkerBootstrapDescriptor {
        running_on_main_thread: false,
        worker_id,
        close_on_idle: args.close_on_idle,
        module_specifier: Some(module_specifier),
        worker_metadata: maybe_worker_metadata,
    };
    let (ctrl_sender, ctrl_receiver) = unbounded::<WorkerControlEvent>();
    let ctrl_receiver = Rc::new(RefCell::new(ctrl_receiver));
    let cpu_thread_handle = Arc::new(AtomicU64::new(0));
    spawn_worker_thread(WorkerThreadSpawnRequest {
        worker_id,
        runtime_owner,
        bundle,
        tempdir,
        worker_bootstrap_descriptor,
        shared_env,
        child_port,
        cpu_thread_handle: cpu_thread_handle.clone(),
        ctrl_sender,
        stack_size_mb: args.resource_limits.and_then(|limits| limits.stack_size_mb),
    })?;
    state.borrow_mut::<WorkersTable>().insert(
        worker_id,
        WorkerThreadEntry {
            port: parent_port,
            ctrl_receiver,
            cpu_thread_handle,
        },
    );
    Ok(worker_id)
}

#[op2(fast)]
pub(super) fn op_host_terminate_worker(state: &mut OpState, #[smi] id: u32) {
    if let Some(entry) = state.borrow_mut::<WorkersTable>().remove(&id) {
        entry.port.disentangle();
    }
}

#[op2]
#[serde]
pub(super) async fn op_host_recv_ctrl(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
) -> WorkerControlEvent {
    let receiver = {
        let state_ref = state.borrow();
        match state_ref.borrow::<WorkersTable>().get(&id) {
            Some(entry) => entry.ctrl_receiver.clone(),
            None => return WorkerControlEvent::Close(0),
        }
    };
    let maybe_event = poll_fn(|cx| receiver.borrow_mut().poll_next_unpin(cx)).await;
    maybe_event.unwrap_or(WorkerControlEvent::Close(0))
}

#[op2]
#[serde]
pub(super) fn op_host_recv_ctrl_sync(
    state: &mut OpState,
    #[smi] id: u32,
) -> Option<WorkerControlEvent> {
    let receiver = {
        let workers = state.borrow::<WorkersTable>();
        match workers.get(&id) {
            Some(entry) => entry.ctrl_receiver.clone(),
            None => return Some(WorkerControlEvent::Close(0)),
        }
    };
    match receiver.borrow_mut().try_recv() {
        Ok(event) => Some(event),
        Err(TryRecvError::Empty) => None,
        Err(TryRecvError::Closed) => Some(WorkerControlEvent::Close(0)),
    }
}

#[op2]
pub(super) async fn op_host_recv_message(
    state: Rc<RefCell<OpState>>,
    #[smi] id: u32,
) -> Result<Option<RecvMessageData>, MessagePortError> {
    let port = {
        let state_ref = state.borrow();
        match state_ref.borrow::<WorkersTable>().get(&id) {
            Some(entry) => entry.port.clone(),
            None => return Ok(None),
        }
    };
    let data = port.recv(state.clone()).await?;
    Ok(data)
}

#[op2]
#[serde]
pub(super) fn op_host_recv_message_sync(
    state: &mut OpState,
    #[smi] id: u32,
) -> Result<Option<JsMessageData>, MessagePortError> {
    let port = {
        let workers = state.borrow::<WorkersTable>();
        match workers.get(&id) {
            Some(entry) => entry.port.clone(),
            None => return Ok(None),
        }
    };
    let data = port.try_recv_sync(state)?;
    Ok(data)
}

#[op2]
pub(super) fn op_host_post_message(
    state: &mut OpState,
    #[smi] id: u32,
    #[serde] data: JsMessageData,
) -> Result<(), MessagePortError> {
    let port = {
        let workers = state.borrow::<WorkersTable>();
        match workers.get(&id) {
            Some(entry) => entry.port.clone(),
            None => return Ok(()),
        }
    };
    port.send(state, data)
}

#[op2]
pub(super) fn op_host_post_message_raw(
    state: &mut OpState,
    #[smi] id: u32,
    #[buffer(detach)] data: JsBuffer,
) -> Result<(), MessagePortError> {
    let port = {
        let workers = state.borrow::<WorkersTable>();
        match workers.get(&id) {
            Some(entry) => entry.port.clone(),
            None => return Ok(()),
        }
    };
    let detached = DetachedBuffer::from_v8slice(data.into_parts());
    send_raw_message_to_port(port.as_ref(), detached, "host worker")
}

#[op2(fast)]
pub(super) fn op_host_get_worker_cpu_usage(
    state: &mut OpState,
    #[smi] id: u32,
    #[buffer] out: &mut [f64],
) {
    write_worker_cpu_usage(state.borrow::<WorkersTable>(), id, out);
}

fn write_worker_cpu_usage(workers: &WorkersTable, id: u32, out: &mut [f64]) {
    if out.len() < 2 {
        return;
    }
    if let Some(entry) = workers.get(&id) {
        let handle = entry.cpu_thread_handle.load(Ordering::Acquire);
        if handle != 0 {
            let (user, system) = get_thread_cpu_usage_by_handle(handle);
            out[0] = user;
            out[1] = system;
            return;
        }
    }
    out[0] = 0.0;
    out[1] = 0.0;
}

#[op2(fast)]
pub(super) fn op_current_thread_cpu_usage(#[buffer] out: &mut [f64]) {
    write_current_thread_cpu_usage(out);
}

fn write_current_thread_cpu_usage(out: &mut [f64]) {
    if out.len() < 2 {
        return;
    }
    let handle = capture_current_thread_handle();
    let (user, system) = get_thread_cpu_usage_by_handle(handle);
    out[0] = user;
    out[1] = system;
}

fn worker_parent_port(state: &OpState) -> Result<Rc<MessagePort>, MessagePortError> {
    state
        .try_borrow::<InstalledRuntimeWorkerBootstrapState>()
        .and_then(|bootstrap_state| bootstrap_state.parent_port.clone())
        .ok_or_else(|| {
            MessagePortError::Generic(JsErrorBox::generic("worker parent port is unavailable"))
        })
}

fn send_raw_message_to_port(
    port: &MessagePort,
    detached: DetachedBuffer,
    channel_label: &'static str,
) -> Result<(), MessagePortError> {
    let Some(tx) = &*port.tx.borrow() else {
        return Ok(());
    };
    map_raw_send_result(tx.send((detached, vec![])), channel_label)
}

fn map_raw_send_result<T>(
    result: Result<(), tokio::sync::mpsc::error::SendError<T>>,
    channel_label: &'static str,
) -> Result<(), MessagePortError> {
    result.map_err(|_| closed_raw_receiver_error(channel_label))
}

fn closed_raw_receiver_error(channel_label: &'static str) -> MessagePortError {
    MessagePortError::Generic(JsErrorBox::generic(format!(
        "{channel_label} raw message receiver is closed"
    )))
}

pub(crate) fn worker_threads_state_extension(
    bootstrap_state: InstalledRuntimeWorkerBootstrapState,
) -> Extension {
    Extension {
        name: "nimbus_runtime_worker_threads_state_ext",
        op_state_fn: Some(Box::new(move |state| {
            install_missing_deno_extension_state(state);
            state.put(WorkersTable::default());
            state.put(bootstrap_state.shared_env.clone());
            state.put(bootstrap_state);
        })),
        ..Default::default()
    }
}

fn prepare_worker_bundle(
    args: &CreateWorkerArgs,
    worker_cwd: &Path,
) -> Result<(TempDir, RuntimeBundle, String), JsErrorBox> {
    if args.has_source_code {
        let tempdir = tempfile::Builder::new()
            .prefix("nimbus-worker-")
            .tempdir_in(worker_cwd)
            .map_err(io_error)?;
        let bundle_path = tempdir.path().join("bundle.mjs");
        let eval_path = tempdir.path().join("worker-eval.cjs");
        std::fs::write(&eval_path, &args.source_code).map_err(io_error)?;
        let bundle_source = format!(
            r#"
import {{ createRequire }} from "node:module";

const require = createRequire(import.meta.url);
globalThis.require = require;
globalThis.__nimbusStartWorkerMessagePump?.();
if (
  globalThis.__nimbusWorkerThreadEnv &&
  globalThis.process &&
  typeof globalThis.process === "object"
) {{
  try {{
    Object.defineProperty(globalThis.process, "env", {{
      value: globalThis.__nimbusWorkerThreadEnv,
      configurable: true,
      enumerable: true,
      writable: true,
    }});
  }} catch (_error) {{
    try {{
      globalThis.process.env = globalThis.__nimbusWorkerThreadEnv;
    }} catch (_secondaryError) {{
      // Leave the worker process untouched; focused compat tests will surface
      // any remaining contract drift.
    }}
  }}
}}
require({});

export {{}};
"#,
            serde_json::to_string(&eval_path.to_string_lossy().into_owned())
                .expect("worker eval path should serialize"),
        );
        std::fs::write(&bundle_path, bundle_source).map_err(io_error)?;
        let bundle = RuntimeBundle::with_module_root(&bundle_path, worker_cwd);
        return Ok((tempdir, bundle, args.specifier.clone()));
    }

    let script_path = resolve_worker_script_path(&args.specifier, worker_cwd)?;
    let module_root = infer_worker_module_root(&script_path);
    let tempdir = tempfile::Builder::new()
        .prefix("nimbus-worker-")
        .tempdir_in(&module_root)
        .map_err(io_error)?;
    let bundle_path = tempdir.path().join("bundle.mjs");
    let bundle_source = r#"
import { createRequire } from "node:module";

const require = createRequire(import.meta.url);
globalThis.require = require;
globalThis.__nimbusStartWorkerMessagePump?.();
if (
  globalThis.__nimbusWorkerThreadEnv &&
  globalThis.process &&
  typeof globalThis.process === "object"
) {
  try {
    Object.defineProperty(globalThis.process, "env", {
      value: globalThis.__nimbusWorkerThreadEnv,
      configurable: true,
      enumerable: true,
      writable: true,
    });
  } catch (_error) {
    try {
      globalThis.process.env = globalThis.__nimbusWorkerThreadEnv;
    } catch (_secondaryError) {
      // Leave the worker process untouched; focused compat tests will surface
      // any remaining contract drift.
    }
  }
}
require("node:module").runMain();

export {};
"#;
    std::fs::write(&bundle_path, bundle_source).map_err(io_error)?;
    let bundle = RuntimeBundle::with_side_entrypoint_and_module_root(&bundle_path, module_root);
    Ok((tempdir, bundle, args.specifier.clone()))
}

fn resolve_worker_script_path(specifier: &str, worker_cwd: &Path) -> Result<PathBuf, JsErrorBox> {
    if specifier.starts_with("file:") {
        let url = url::Url::parse(specifier).map_err(|error| {
            JsErrorBox::generic(format!("invalid worker file URL `{specifier}`: {error}"))
        })?;
        let path = url.to_file_path().map_err(|_| {
            JsErrorBox::generic(format!("worker file URL is not a valid path: {specifier}"))
        });
        return path.map(|path| rebase_worker_script_path(path, worker_cwd));
    }
    let path = PathBuf::from(specifier);
    if path.is_relative() {
        return Ok(worker_cwd.join(path));
    }
    Ok(rebase_worker_script_path(path, worker_cwd))
}

fn rebase_worker_script_path(path: PathBuf, worker_cwd: &Path) -> PathBuf {
    let Ok(host_cwd) = std::env::current_dir() else {
        return path;
    };
    let Ok(relative_path) = path.strip_prefix(&host_cwd) else {
        return path;
    };
    let rebased_path = worker_cwd.join(relative_path);
    if rebased_path.exists() {
        return rebased_path;
    }
    path
}

fn infer_worker_module_root(script_path: &Path) -> PathBuf {
    let canonical = script_path
        .canonicalize()
        .unwrap_or_else(|_| script_path.to_path_buf());
    let parent = canonical
        .parent()
        .map(Path::to_path_buf)
        .unwrap_or_else(|| canonical.clone());
    for ancestor in canonical.ancestors() {
        if ancestor.join("package.json").is_file() {
            return ancestor.to_path_buf();
        }
    }
    for ancestor in canonical.ancestors() {
        if ancestor.file_name().is_some_and(|name| name == "test") {
            return ancestor.to_path_buf();
        }
    }
    parent
}

struct WorkerThreadSpawnRequest {
    worker_id: u32,
    runtime_owner: NimbusRuntime,
    bundle: RuntimeBundle,
    tempdir: TempDir,
    worker_bootstrap_descriptor: RuntimeWorkerBootstrapDescriptor,
    shared_env: RuntimeSharedWorkerEnv,
    child_port: MessagePort,
    cpu_thread_handle: Arc<AtomicU64>,
    ctrl_sender: UnboundedSender<WorkerControlEvent>,
    stack_size_mb: Option<usize>,
}

fn spawn_worker_thread(request: WorkerThreadSpawnRequest) -> Result<(), JsErrorBox> {
    let WorkerThreadSpawnRequest {
        worker_id,
        runtime_owner,
        bundle,
        tempdir,
        worker_bootstrap_descriptor,
        shared_env,
        child_port,
        cpu_thread_handle,
        ctrl_sender,
        stack_size_mb,
    } = request;
    let mut builder = std::thread::Builder::new().name(format!("worker-{worker_id}"));
    if let Some(stack_size_mb) = stack_size_mb.filter(|value| *value > 0) {
        builder = builder.stack_size(stack_size_mb * 1024 * 1024);
    }
    builder
        .spawn(move || {
            let child_port = Rc::new(child_port);
            let child_port_for_shutdown = child_port.clone();
            cpu_thread_handle.store(capture_current_thread_handle(), Ordering::Release);
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                let runtime = tokio::runtime::Builder::new_current_thread()
                    .enable_all()
                    .build()
                    .map_err(|error| format!("failed to build worker runtime: {error}"))?;
                runtime.block_on(async move {
                    let bootstrap_state = InstalledRuntimeWorkerBootstrapState {
                        descriptor: worker_bootstrap_descriptor,
                        parent_port: Some(child_port),
                        shared_env,
                    };
                    let mut js_runtime = runtime_owner
                        .create_unsnapshotted_runtime_with_worker_bootstrap(
                            &bundle,
                            bootstrap_state,
                        )
                        .map_err(|error| error.to_string())?;
                    runtime_owner
                        .load_bundle_with_trace(
                            &mut js_runtime,
                            &bundle,
                            V8RuntimeConstructionMode::StartupSnapshot,
                            None,
                            None,
                        )
                        .await
                        .map_err(|error| error.to_string())
                })
            }));
            let control_event = match result {
                Ok(Ok(())) => WorkerControlEvent::Close(0),
                Ok(Err(message)) => WorkerControlEvent::TerminalError {
                    message,
                    name: "Error".to_string(),
                    exit_code: 1,
                },
                Err(_) => WorkerControlEvent::TerminalError {
                    message: "worker thread panicked".to_string(),
                    name: "Error".to_string(),
                    exit_code: 1,
                },
            };
            if matches!(control_event, WorkerControlEvent::Close(_)) {
                child_port_for_shutdown.disentangle();
            }
            let _ = ctrl_sender.unbounded_send(control_event);
            drop(tempdir);
        })
        .map_err(io_error)?;
    Ok(())
}

fn io_error(error: std::io::Error) -> JsErrorBox {
    JsErrorBox::generic(error.to_string())
}

#[cfg(target_os = "macos")]
fn capture_current_thread_handle() -> u64 {
    // SAFETY: `mach_thread_self` takes no arguments and returns the send right for
    // the current thread according to the macOS Mach ABI declaration below.
    unsafe { mach_thread_self() as u64 }
}

#[cfg(target_os = "macos")]
fn get_thread_cpu_usage_by_handle(handle: u64) -> (f64, f64) {
    let thread_port = handle as u32;
    // SAFETY: `ThreadBasicInfo` is a repr(C) aggregate of integer fields; all-zero
    // bytes are a valid initial value before `thread_info` fills the structure.
    let mut info: ThreadBasicInfo = unsafe { std::mem::zeroed() };
    let mut count: u32 = THREAD_BASIC_INFO_COUNT;
    // SAFETY: `thread_port` comes from `mach_thread_self` or a stored Mach thread
    // port; `info` is live and sized for `THREAD_BASIC_INFO_COUNT` integer slots,
    // and `count` points to writable storage for the kernel's returned length.
    let kr = unsafe {
        thread_info(
            thread_port,
            THREAD_BASIC_INFO,
            (&raw mut info) as *mut i32,
            &mut count,
        )
    };
    if kr != 0 {
        return (0.0, 0.0);
    }
    let user = info.user_time_seconds as f64 * 1e6 + info.user_time_microseconds as f64;
    let system = info.system_time_seconds as f64 * 1e6 + info.system_time_microseconds as f64;
    (user, system)
}

#[cfg(target_os = "macos")]
const THREAD_BASIC_INFO: u32 = 3;
#[cfg(target_os = "macos")]
const THREAD_BASIC_INFO_COUNT: u32 = 10;

#[cfg(target_os = "macos")]
#[repr(C)]
struct ThreadBasicInfo {
    user_time_seconds: i32,
    user_time_microseconds: i32,
    system_time_seconds: i32,
    system_time_microseconds: i32,
    cpu_usage: i32,
    policy: i32,
    run_state: i32,
    flags: i32,
    suspend_count: i32,
    sleep_time: i32,
}

#[cfg(target_os = "macos")]
// SAFETY: These declarations match the macOS Mach ABI used for querying
// per-thread basic accounting information.
unsafe extern "C" {
    fn mach_thread_self() -> u32;
    fn thread_info(
        target_act: u32,
        flavor: u32,
        thread_info_out: *mut i32,
        thread_info_out_cnt: *mut u32,
    ) -> i32;
}

#[cfg(target_os = "linux")]
fn capture_current_thread_handle() -> u64 {
    // SAFETY: `SYS_gettid` takes no additional arguments on Linux and returns the
    // caller's kernel thread id without writing through any pointers.
    unsafe { libc::syscall(libc::SYS_gettid) as u64 }
}

#[cfg(target_os = "linux")]
fn get_thread_cpu_usage_by_handle(handle: u64) -> (f64, f64) {
    let tid = handle as i32;
    let path = format!("/proc/self/task/{tid}/stat");
    if let Ok(contents) = std::fs::read_to_string(&path)
        && let Some(position) = contents.rfind(')')
    {
        let rest = &contents[position + 2..];
        let fields: Vec<&str> = rest.split_whitespace().collect();
        if fields.len() > 12 {
            // SAFETY: `_SC_CLK_TCK` is a pure libc query that takes no pointers
            // and returns the process clock-tick rate for converting `/proc` CPU
            // counters.
            let ticks_per_second = unsafe { libc::sysconf(libc::_SC_CLK_TCK) } as f64;
            let utime = fields[11].parse::<f64>().unwrap_or(0.0);
            let stime = fields[12].parse::<f64>().unwrap_or(0.0);
            return (
                utime / ticks_per_second * 1e6,
                stime / ticks_per_second * 1e6,
            );
        }
    }
    (0.0, 0.0)
}

#[cfg(windows)]
fn capture_current_thread_handle() -> u64 {
    use windows_sys::Win32::System::Threading::GetCurrentThreadId;

    // SAFETY: `GetCurrentThreadId` takes no arguments and returns the caller's
    // thread id without requiring any borrowed memory or handle ownership.
    unsafe { GetCurrentThreadId() as u64 }
}

#[cfg(windows)]
fn get_thread_cpu_usage_by_handle(handle: u64) -> (f64, f64) {
    use windows_sys::Win32::Foundation::{CloseHandle, FALSE, FILETIME};
    use windows_sys::Win32::System::Threading::{
        GetThreadTimes, OpenThread, THREAD_QUERY_INFORMATION,
    };

    let thread_id = handle as u32;
    // SAFETY: `OpenThread` is called with a thread id captured from Windows and
    // requests query-only access. A null handle is handled immediately below, and
    // a non-null handle is closed exactly once after `GetThreadTimes`.
    let thread_handle = unsafe { OpenThread(THREAD_QUERY_INFORMATION, FALSE, thread_id) };
    if thread_handle.is_null() {
        return (0.0, 0.0);
    }

    let mut creation_time = std::mem::MaybeUninit::<FILETIME>::uninit();
    let mut exit_time = std::mem::MaybeUninit::<FILETIME>::uninit();
    let mut kernel_time = std::mem::MaybeUninit::<FILETIME>::uninit();
    let mut user_time = std::mem::MaybeUninit::<FILETIME>::uninit();
    // SAFETY: `thread_handle` is a non-null handle returned by `OpenThread`, and
    // each output pointer references live `MaybeUninit<FILETIME>` storage that
    // `GetThreadTimes` initializes on success.
    let ret = unsafe {
        GetThreadTimes(
            thread_handle,
            creation_time.as_mut_ptr(),
            exit_time.as_mut_ptr(),
            kernel_time.as_mut_ptr(),
            user_time.as_mut_ptr(),
        )
    };
    // SAFETY: `thread_handle` is owned by this function and is closed exactly once
    // after the only API call that needs it.
    unsafe { CloseHandle(thread_handle) };
    if ret == FALSE {
        return (0.0, 0.0);
    }
    // SAFETY: a non-FALSE return from `GetThreadTimes` means all supplied
    // `FILETIME` out-parameters, including user and kernel time, were initialized.
    let user_time = unsafe { user_time.assume_init() };
    // SAFETY: see the successful `GetThreadTimes` check above.
    let kernel_time = unsafe { kernel_time.assume_init() };
    (
        (((user_time.dwHighDateTime as u64) << 32) | user_time.dwLowDateTime as u64) as f64 / 10.0,
        (((kernel_time.dwHighDateTime as u64) << 32) | kernel_time.dwLowDateTime as u64) as f64
            / 10.0,
    )
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn capture_current_thread_handle() -> u64 {
    0
}

#[cfg(not(any(target_os = "macos", target_os = "linux", windows)))]
fn get_thread_cpu_usage_by_handle(_handle: u64) -> (f64, f64) {
    (0.0, 0.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_thread_cpu_usage_ignores_short_output_buffers() {
        let mut one_slot = [42.0_f64];
        write_current_thread_cpu_usage(&mut one_slot);
        assert_eq!(one_slot, [42.0]);

        let mut two_slots = [-1.0_f64, -1.0_f64];
        write_current_thread_cpu_usage(&mut two_slots);
        assert!(two_slots[0].is_finite());
        assert!(two_slots[1].is_finite());
        assert!(two_slots[0] >= 0.0);
        assert!(two_slots[1] >= 0.0);
    }

    #[test]
    fn host_worker_cpu_usage_ignores_short_output_buffers() {
        let workers = WorkersTable::default();

        let mut one_slot = [42.0_f64];
        write_worker_cpu_usage(&workers, u32::MAX, &mut one_slot);
        assert_eq!(one_slot, [42.0]);

        let mut two_slots = [-1.0_f64, -1.0_f64];
        write_worker_cpu_usage(&workers, u32::MAX, &mut two_slots);
        assert_eq!(two_slots, [0.0, 0.0]);
    }

    #[test]
    fn raw_message_send_reports_closed_receiver() {
        let (sender, receiver) = tokio::sync::mpsc::unbounded_channel::<()>();
        drop(receiver);

        let error = map_raw_send_result(sender.send(()), "host worker")
            .expect_err("closed raw message receiver should be reported");
        assert!(
            error
                .to_string()
                .contains("host worker raw message receiver is closed"),
            "unexpected raw-send error: {error}"
        );
    }
}
