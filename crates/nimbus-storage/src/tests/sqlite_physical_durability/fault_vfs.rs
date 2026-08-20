//! A test-only SQLite VFS shim that fails bounded physical operations.
//!
//! The shim wraps whatever VFS SQLite already chose and passes every call
//! straight through until a test arms a fault. Arming is scoped to one
//! database path, so a fault can never reach another test's file, and the
//! guard disarms on drop even if the test panics.
//!
//! Production code has no knowledge of this: nothing here is reachable outside
//! `cfg(test)`, and the shim installs itself only when a test first arms a
//! fault. The `SqliteTenantStore` under test opens exactly as it does in
//! production.

use std::ffi::{CStr, CString, c_char, c_int, c_void};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Mutex, Once, OnceLock};

use rusqlite::ffi;

/// The physical failure a test wants SQLite to meet.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum PhysicalFault {
    /// The device reports no free space on the next database write.
    DiskFull,
    /// `fsync` fails, so SQLite cannot know the bytes reached the platter.
    SyncFailure,
    /// The write-ahead log cannot take a byte.
    WalWriteFailure,
}

/// Which file of a SQLite database an operation reached.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum FileKind {
    Main,
    Wal,
    Other,
}

struct Arm {
    /// Faults apply only to files whose path contains this marker. The marker
    /// is the unique basename stem of the database under test, so a fault can
    /// never reach another test's file and the match survives whatever
    /// directory form the platform's VFS resolves.
    marker: String,
    fault: PhysicalFault,
    /// Successful matching operations to let through before failing.
    grace: u32,
    fired: bool,
}

/// A relaxed fast path so an unarmed shim never takes the mutex.
static ANY_ARMED: AtomicBool = AtomicBool::new(false);

fn armed() -> &'static Mutex<Option<Arm>> {
    static ARMED: OnceLock<Mutex<Option<Arm>>> = OnceLock::new();
    ARMED.get_or_init(|| Mutex::new(None))
}

/// Disarms the shim when the test's scope ends, panic or not.
pub(crate) struct PhysicalFaultGuard {
    _private: (),
}

impl Drop for PhysicalFaultGuard {
    fn drop(&mut self) {
        ANY_ARMED.store(false, Ordering::SeqCst);
        *armed()
            .lock()
            .expect("fault arm lock should not be poisoned") = None;
    }
}

/// Arms `fault` for files under `marker`, after `grace` matching operations.
///
/// The grace count exists because a database that has just been created is
/// mid-write for its own header and schema. A fault at the very first write
/// would prove that SQLite refuses to open a broken file, which is not the
/// contract under test; the tests here want a fault against an already
/// acknowledged, already durable database.
pub(crate) fn arm(marker: &str, fault: PhysicalFault, grace: u32) -> PhysicalFaultGuard {
    install();
    *armed()
        .lock()
        .expect("fault arm lock should not be poisoned") = Some(Arm {
        marker: marker.to_string(),
        fault,
        grace,
        fired: false,
    });
    ANY_ARMED.store(true, Ordering::SeqCst);
    PhysicalFaultGuard { _private: () }
}

/// Whether the armed fault has actually fired yet.
pub(crate) fn fault_fired() -> bool {
    armed()
        .lock()
        .expect("fault arm lock should not be poisoned")
        .as_ref()
        .is_some_and(|arm| arm.fired)
}

/// Decides one operation: `None` to pass through, `Some(code)` to fail.
fn decide(path: &str, kind: FileKind, op: Operation) -> Option<c_int> {
    if !ANY_ARMED.load(Ordering::SeqCst) {
        return None;
    }
    let mut guard = armed()
        .lock()
        .expect("fault arm lock should not be poisoned");
    let arm = guard.as_mut()?;
    if !path.contains(&arm.marker) {
        return None;
    }
    let matches = matches!(
        (arm.fault, op, kind),
        (
            PhysicalFault::DiskFull,
            Operation::Write,
            FileKind::Main | FileKind::Wal
        ) | (PhysicalFault::SyncFailure, Operation::Sync, _)
            | (
                PhysicalFault::WalWriteFailure,
                Operation::Write,
                FileKind::Wal
            )
    );
    if !matches {
        return None;
    }
    if arm.grace > 0 {
        arm.grace -= 1;
        return None;
    }
    arm.fired = true;
    Some(match arm.fault {
        PhysicalFault::DiskFull => ffi::SQLITE_FULL,
        PhysicalFault::SyncFailure => ffi::SQLITE_IOERR_FSYNC,
        PhysicalFault::WalWriteFailure => ffi::SQLITE_IOERR_WRITE,
    })
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum Operation {
    Write,
    Sync,
}

// --------------------------------------------------------------- the shim

const SHIM_NAME: &CStr = c"nimbus-physical-fault-shim";

/// The file object SQLite allocates: our header, then the real VFS's file.
#[repr(C)]
struct ShimFile {
    base: ffi::sqlite3_file,
    path: *mut c_char,
    kind: FileKind,
}

struct ShimVfs {
    vfs: ffi::sqlite3_vfs,
}

// SQLite owns the pointers for the process lifetime and serializes its own
// access to them; the shim adds no interior state beyond the arm mutex.
unsafe impl Send for ShimVfs {}
unsafe impl Sync for ShimVfs {}

fn shim() -> &'static ShimVfs {
    static SHIM: OnceLock<ShimVfs> = OnceLock::new();
    SHIM.get_or_init(|| {
        // SAFETY: `sqlite3_vfs_find(NULL)` returns the current default VFS,
        // which SQLite keeps alive for the process lifetime.
        let base = unsafe { ffi::sqlite3_vfs_find(std::ptr::null()) };
        assert!(!base.is_null(), "SQLite must have a default VFS");
        // SAFETY: `base` is a live VFS SQLite owns.
        let base_ref = unsafe { &*base };

        let mut vfs = ffi::sqlite3_vfs {
            iVersion: 1,
            szOsFile: (std::mem::size_of::<ShimFile>() as c_int) + base_ref.szOsFile,
            mxPathname: base_ref.mxPathname,
            pNext: std::ptr::null_mut(),
            zName: SHIM_NAME.as_ptr(),
            pAppData: std::ptr::null_mut(),
            xOpen: Some(shim_open),
            xDelete: base_ref.xDelete,
            xAccess: base_ref.xAccess,
            xFullPathname: base_ref.xFullPathname,
            xDlOpen: base_ref.xDlOpen,
            xDlError: base_ref.xDlError,
            xDlSym: base_ref.xDlSym,
            xDlClose: base_ref.xDlClose,
            xRandomness: base_ref.xRandomness,
            xSleep: base_ref.xSleep,
            xCurrentTime: base_ref.xCurrentTime,
            xGetLastError: base_ref.xGetLastError,
            xCurrentTimeInt64: None,
            xSetSystemCall: None,
            xGetSystemCall: None,
            xNextSystemCall: None,
        };
        // The VFS this shim delegates to travels in `pAppData`, so `xOpen`
        // reaches it without a second static.
        vfs.pAppData = base.cast::<c_void>();
        ShimVfs { vfs }
    })
}

/// Registers the shim as the default VFS. Idempotent.
///
/// A connection binds its VFS when it opens, so every test that may arm a
/// fault must install the shim *before* it opens the store under test.
pub(crate) fn install() {
    static INSTALL: Once = Once::new();
    INSTALL.call_once(|| {
        let shim = shim();
        let vfs_ptr = std::ptr::from_ref(&shim.vfs).cast_mut();
        // SAFETY: the shim outlives the process and delegates every call to
        // the VFS it captured before registering, so an unarmed shim behaves
        // exactly like the VFS it replaced.
        let rc = unsafe { ffi::sqlite3_vfs_register(vfs_ptr, 1) };
        assert_eq!(rc, ffi::SQLITE_OK, "the fault shim VFS must register");
    });
}

const SHIM_METHODS: ffi::sqlite3_io_methods = ffi::sqlite3_io_methods {
    iVersion: 2,
    xClose: Some(shim_close),
    xRead: Some(shim_read),
    xWrite: Some(shim_write),
    xTruncate: Some(shim_truncate),
    xSync: Some(shim_sync),
    xFileSize: Some(shim_file_size),
    xLock: Some(shim_lock),
    xUnlock: Some(shim_unlock),
    xCheckReservedLock: Some(shim_check_reserved_lock),
    xFileControl: Some(shim_file_control),
    xSectorSize: Some(shim_sector_size),
    xDeviceCharacteristics: Some(shim_device_characteristics),
    xShmMap: Some(shim_shm_map),
    xShmLock: Some(shim_shm_lock),
    xShmBarrier: Some(shim_shm_barrier),
    xShmUnmap: Some(shim_shm_unmap),
    xFetch: None,
    xUnfetch: None,
};

/// The real file object, which lives immediately after our header.
///
/// # Safety
/// `file` must be a `sqlite3_file` this shim's `xOpen` produced.
unsafe fn real_file(file: *mut ffi::sqlite3_file) -> *mut ffi::sqlite3_file {
    unsafe { file.cast::<u8>().add(std::mem::size_of::<ShimFile>()) }.cast()
}

/// # Safety
/// `file` must be a `sqlite3_file` this shim's `xOpen` produced.
unsafe fn shim_header<'a>(file: *mut ffi::sqlite3_file) -> &'a mut ShimFile {
    unsafe { &mut *file.cast::<ShimFile>() }
}

/// # Safety
/// `file` must be a live shim file whose real methods are installed.
unsafe fn real_methods<'a>(file: *mut ffi::sqlite3_file) -> &'a ffi::sqlite3_io_methods {
    unsafe { &*(*real_file(file)).pMethods }
}

unsafe extern "C" fn shim_open(
    vfs: *mut ffi::sqlite3_vfs,
    name: ffi::sqlite3_filename,
    file: *mut ffi::sqlite3_file,
    flags: c_int,
    out_flags: *mut c_int,
) -> c_int {
    unsafe {
        // SQLite treats a non-null `pMethods` as an open file, so leave the
        // header inert until the real open succeeds.
        let header = shim_header(file);
        header.base.pMethods = std::ptr::null();
        header.path = std::ptr::null_mut();

        let base = (*vfs).pAppData.cast::<ffi::sqlite3_vfs>();
        let real = real_file(file);
        let rc = ((*base).xOpen.expect("the base VFS must implement xOpen"))(
            base, name, real, flags, out_flags,
        );
        if rc != ffi::SQLITE_OK {
            return rc;
        }

        let path = if name.is_null() {
            String::new()
        } else {
            CStr::from_ptr(name).to_string_lossy().into_owned()
        };
        let kind = if path.ends_with("-wal") {
            FileKind::Wal
        } else if path.ends_with("-journal") || path.ends_with("-shm") {
            FileKind::Other
        } else {
            FileKind::Main
        };

        header.base.pMethods = &SHIM_METHODS;
        header.path = CString::new(path)
            .expect("a SQLite path carries no interior NUL")
            .into_raw();
        header.kind = kind;
        ffi::SQLITE_OK
    }
}

unsafe extern "C" fn shim_close(file: *mut ffi::sqlite3_file) -> c_int {
    unsafe {
        let rc = (real_methods(file).xClose.expect("xClose"))(real_file(file));
        let header = shim_header(file);
        if !header.path.is_null() {
            drop(CString::from_raw(header.path));
            header.path = std::ptr::null_mut();
        }
        rc
    }
}

unsafe extern "C" fn shim_read(
    file: *mut ffi::sqlite3_file,
    buffer: *mut c_void,
    amount: c_int,
    offset: ffi::sqlite3_int64,
) -> c_int {
    unsafe { (real_methods(file).xRead.expect("xRead"))(real_file(file), buffer, amount, offset) }
}

unsafe extern "C" fn shim_write(
    file: *mut ffi::sqlite3_file,
    buffer: *const c_void,
    amount: c_int,
    offset: ffi::sqlite3_int64,
) -> c_int {
    unsafe {
        let header = shim_header(file);
        if let Some(code) = decide(&header_path(header), header.kind, Operation::Write) {
            return code;
        }
        (real_methods(file).xWrite.expect("xWrite"))(real_file(file), buffer, amount, offset)
    }
}

unsafe extern "C" fn shim_truncate(
    file: *mut ffi::sqlite3_file,
    size: ffi::sqlite3_int64,
) -> c_int {
    unsafe { (real_methods(file).xTruncate.expect("xTruncate"))(real_file(file), size) }
}

unsafe extern "C" fn shim_sync(file: *mut ffi::sqlite3_file, flags: c_int) -> c_int {
    unsafe {
        let header = shim_header(file);
        if let Some(code) = decide(&header_path(header), header.kind, Operation::Sync) {
            return code;
        }
        (real_methods(file).xSync.expect("xSync"))(real_file(file), flags)
    }
}

unsafe extern "C" fn shim_file_size(
    file: *mut ffi::sqlite3_file,
    size: *mut ffi::sqlite3_int64,
) -> c_int {
    unsafe { (real_methods(file).xFileSize.expect("xFileSize"))(real_file(file), size) }
}

unsafe extern "C" fn shim_lock(file: *mut ffi::sqlite3_file, level: c_int) -> c_int {
    unsafe { (real_methods(file).xLock.expect("xLock"))(real_file(file), level) }
}

unsafe extern "C" fn shim_unlock(file: *mut ffi::sqlite3_file, level: c_int) -> c_int {
    unsafe { (real_methods(file).xUnlock.expect("xUnlock"))(real_file(file), level) }
}

unsafe extern "C" fn shim_check_reserved_lock(
    file: *mut ffi::sqlite3_file,
    result: *mut c_int,
) -> c_int {
    unsafe {
        (real_methods(file)
            .xCheckReservedLock
            .expect("xCheckReservedLock"))(real_file(file), result)
    }
}

unsafe extern "C" fn shim_file_control(
    file: *mut ffi::sqlite3_file,
    op: c_int,
    arg: *mut c_void,
) -> c_int {
    unsafe { (real_methods(file).xFileControl.expect("xFileControl"))(real_file(file), op, arg) }
}

unsafe extern "C" fn shim_sector_size(file: *mut ffi::sqlite3_file) -> c_int {
    unsafe { (real_methods(file).xSectorSize.expect("xSectorSize"))(real_file(file)) }
}

unsafe extern "C" fn shim_device_characteristics(file: *mut ffi::sqlite3_file) -> c_int {
    unsafe {
        (real_methods(file)
            .xDeviceCharacteristics
            .expect("xDeviceCharacteristics"))(real_file(file))
    }
}

unsafe extern "C" fn shim_shm_map(
    file: *mut ffi::sqlite3_file,
    page: c_int,
    page_size: c_int,
    extend: c_int,
    out: *mut *mut c_void,
) -> c_int {
    unsafe {
        (real_methods(file).xShmMap.expect("xShmMap"))(
            real_file(file),
            page,
            page_size,
            extend,
            out,
        )
    }
}

unsafe extern "C" fn shim_shm_lock(
    file: *mut ffi::sqlite3_file,
    offset: c_int,
    count: c_int,
    flags: c_int,
) -> c_int {
    unsafe {
        (real_methods(file).xShmLock.expect("xShmLock"))(real_file(file), offset, count, flags)
    }
}

unsafe extern "C" fn shim_shm_barrier(file: *mut ffi::sqlite3_file) {
    unsafe { (real_methods(file).xShmBarrier.expect("xShmBarrier"))(real_file(file)) }
}

unsafe extern "C" fn shim_shm_unmap(file: *mut ffi::sqlite3_file, delete: c_int) -> c_int {
    unsafe { (real_methods(file).xShmUnmap.expect("xShmUnmap"))(real_file(file), delete) }
}

fn header_path(header: &ShimFile) -> String {
    if header.path.is_null() {
        return String::new();
    }
    // SAFETY: `path` was produced by `CString::into_raw` in `shim_open` and is
    // only freed in `shim_close`.
    unsafe { CStr::from_ptr(header.path) }
        .to_string_lossy()
        .into_owned()
}
