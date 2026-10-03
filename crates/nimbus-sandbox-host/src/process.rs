#[cfg(unix)]
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    // SAFETY: `kill(pid, 0)` performs permission/existence probing without
    // dereferencing pointers or sending a signal. PID reuse is possible, so this
    // remains a best-effort liveness hint rather than identity proof.
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0
        || matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM)
        )
}

#[cfg(windows)]
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    use std::ptr;

    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return false;
    }

    // SAFETY: `OpenProcess` receives a numeric PID and no inherited handle; it
    // returns either a nullable handle or an error observable through
    // `GetLastError`.
    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle == ptr::null_mut() {
        // SAFETY: `GetLastError` reads the thread-local error set by the failed
        // `OpenProcess` call above.
        return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
    }

    let mut exit_code = 0;
    // SAFETY: `handle` was returned by `OpenProcess`, and `exit_code` points to
    // valid writable stack storage for the duration of the call.
    let status_ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) != 0 };
    // SAFETY: `handle` was opened successfully above and is closed exactly once.
    let _ = unsafe { CloseHandle(handle) };
    status_ok && exit_code == STILL_ACTIVE as u32
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    let _ = pid;
    false
}
