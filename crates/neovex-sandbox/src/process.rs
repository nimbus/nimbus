#[cfg(unix)]
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    let result = unsafe { libc::kill(pid as i32, 0) };
    result == 0
        || matches!(
            std::io::Error::last_os_error().raw_os_error(),
            Some(libc::EPERM)
        )
}

#[cfg(windows)]
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ACCESS_DENIED, GetLastError, STILL_ACTIVE,
    };
    use windows_sys::Win32::System::Threading::{
        GetExitCodeProcess, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION,
    };

    if pid == 0 {
        return false;
    }

    let handle = unsafe { OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, 0, pid) };
    if handle == 0 {
        return unsafe { GetLastError() } == ERROR_ACCESS_DENIED;
    }

    let mut exit_code = 0;
    let status_ok = unsafe { GetExitCodeProcess(handle, &mut exit_code) != 0 };
    let _ = unsafe { CloseHandle(handle) };
    status_ok && exit_code == STILL_ACTIVE
}

#[cfg(not(any(unix, windows)))]
pub(crate) fn pid_is_alive(pid: u32) -> bool {
    let _ = pid;
    false
}
