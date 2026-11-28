//! Keep the external runtime's preload and F1 restart in its launcher job.

use std::collections::{HashMap, HashSet};
use std::io;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::process::Child;
use std::time::{Duration, Instant};
use windows_sys::Win32::Foundation::{HANDLE, WAIT_OBJECT_0};
use windows_sys::Win32::System::JobObjects::{
    IsProcessInJob, JOBOBJECT_BASIC_ACCOUNTING_INFORMATION, JobObjectBasicAccountingInformation,
    QueryInformationJobObject,
};
use windows_sys::Win32::System::Threading::{
    OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_SYNCHRONIZE, PROCESS_TERMINATE,
    TerminateProcess, WaitForSingleObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowExW, GetWindowThreadProcessId, PostMessageW, WM_CLOSE,
};

fn active(job: HANDLE) -> io::Result<u32> {
    let mut info = JOBOBJECT_BASIC_ACCOUNTING_INFORMATION::default();
    // SAFETY: the owned job remains live and the output matches this information class.
    let ok = unsafe {
        QueryInformationJobObject(
            job,
            JobObjectBasicAccountingInformation,
            std::ptr::from_mut(&mut info).cast(),
            size_of_val(&info) as u32,
            std::ptr::null_mut(),
        )
    };
    if ok == 0 {
        Err(io::Error::last_os_error())
    } else {
        Ok(info.ActiveProcesses)
    }
}

fn member(job: HANDLE, pid: u32) -> Option<OwnedHandle> {
    // SAFETY: OpenProcess returns a new handle on success. Holding it through
    // termination prevents a stale PID from targeting a replacement process.
    let raw = unsafe {
        OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_TERMINATE | PROCESS_SYNCHRONIZE,
            0,
            pid,
        )
    };
    if raw.is_null() {
        return None;
    }
    // SAFETY: ownership of the non-null process handle is transferred once.
    let process = unsafe { OwnedHandle::from_raw_handle(raw) };
    let mut inside = 0;
    // SAFETY: both handles and the BOOL output are valid.
    (unsafe { IsProcessInJob(raw, job, &mut inside) } != 0 && inside != 0).then_some(process)
}

struct GameWindow {
    process: OwnedHandle,
    missing_since: Option<Instant>,
}

pub fn wait(
    child: &mut Child,
    group: &OwnedHandle,
    stopping: impl Fn() -> bool,
    grace: Duration,
) -> Result<i32, String> {
    wait_for_class(child, group, stopping, grace, "VIctory")
}

fn wait_for_class(
    child: &mut Child,
    group: &OwnedHandle,
    stopping: impl Fn() -> bool,
    grace: Duration,
    window_class: &str,
) -> Result<i32, String> {
    let job = group.as_raw_handle();
    let mut seen: HashMap<u32, GameWindow> = HashMap::new();
    let class: Vec<u16> = window_class.encode_utf16().chain([0]).collect();
    let mut stop_since = None;
    let mut forced = false;
    while active(job).map_err(|e| e.to_string())? > 0 {
        let mut present = HashSet::new();
        let mut hwnd = std::ptr::null_mut();
        let cancel = stopping();
        loop {
            // SAFETY: null parent enumerates top-level windows; the class is terminated.
            hwnd = unsafe {
                FindWindowExW(std::ptr::null_mut(), hwnd, class.as_ptr(), std::ptr::null())
            };
            if hwnd.is_null() {
                break;
            }
            let mut pid = 0;
            // SAFETY: the window comes from FindWindowExW, pid is writable.
            unsafe { GetWindowThreadProcessId(hwnd, &mut pid) };
            if let std::collections::hash_map::Entry::Vacant(entry) = seen.entry(pid) {
                let Some(process) = member(job, pid) else {
                    continue;
                };
                entry.insert(GameWindow {
                    process,
                    missing_since: None,
                });
            }
            present.insert(pid);
            if cancel && stop_since.is_none() {
                // SAFETY: only the tracked game window receives the normal close request.
                unsafe { PostMessageW(hwnd, WM_CLOSE, 0, 0) };
            }
        }
        if cancel
            && (present.is_empty() && seen.is_empty()
                || stop_since.get_or_insert_with(Instant::now).elapsed() >= grace)
        {
            // The caller drops its kill-on-close job, including any preload workers.
            return Err("Sesión de VIctory-NVIDIA detenida.".into());
        }
        // A minimized game retains its HWND. A game that destroyed its window
        // may still be saving. Reap only that process after the grace period,
        // allowing a waiting F1 restart helper to start the replacement.
        for (&pid, game) in &mut seen {
            if present.contains(&pid) {
                game.missing_since = None;
            } else if game
                .missing_since
                .get_or_insert_with(Instant::now)
                .elapsed()
                >= grace
            {
                let proc = game.process.as_raw_handle();
                // SAFETY: this is the retained handle of a verified job member.
                unsafe {
                    if WaitForSingleObject(proc, 0) != WAIT_OBJECT_0
                        && TerminateProcess(proc, 1) != 0
                    {
                        forced = true;
                    }
                }
            }
        }
        // SAFETY: handles remain valid during the wait. F1 may open another process.
        seen.retain(|_, game| unsafe { WaitForSingleObject(game.process.as_raw_handle(), 0) } != WAIT_OBJECT_0);
        std::thread::sleep(Duration::from_millis(300));
    }
    let status = child.wait().map_err(|e| e.to_string())?;
    if forced {
        return Err("La ventana cerró, pero VIctory-NVIDIA quedó activo. El launcher terminó el proceso tras esperar el guardado.".into());
    }
    if stop_since.is_some() {
        return Err("Partida detenida.".into());
    }
    Ok(status.code().unwrap_or(-1))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::windows::process::CommandExt;
    use std::path::Path;
    use std::process::{Command, Stdio};
    use windows_sys::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DestroyWindow, WS_OVERLAPPED,
    };

    #[test]
    fn an_owned_process_that_survives_its_window_is_reaped() {
        const FIXTURE: &str = "VICTORY_TEST_WINDOW_CLOSE_FIXTURE";
        if let Some(folder) = std::env::var_os(FIXTURE) {
            let go = Path::new(&folder).join("go");
            for _ in 0..100 {
                if go.is_file() {
                    break;
                }
                std::thread::sleep(Duration::from_millis(30));
            }
            let class: Vec<u16> = "STATIC\0".encode_utf16().collect();
            // SAFETY: create an unshown top-level window of the built-in STATIC
            // class; all pointer arguments are null or valid for this call.
            let hwnd = unsafe {
                CreateWindowExW(
                    0,
                    class.as_ptr(),
                    std::ptr::null(),
                    WS_OVERLAPPED,
                    0,
                    0,
                    16,
                    16,
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null_mut(),
                    std::ptr::null(),
                )
            };
            assert!(!hwnd.is_null());
            std::thread::sleep(Duration::from_millis(1500));
            // SAFETY: this thread created the window and owns its destruction.
            assert_ne!(unsafe { DestroyWindow(hwnd) }, 0);
            std::thread::sleep(Duration::from_secs(8));
            return;
        }
        let folder = tempfile::tempdir().unwrap();
        let mut child = Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "nvidia_session::tests::an_owned_process_that_survives_its_window_is_reaped",
            ])
            .env(FIXTURE, folder.path())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .creation_flags(0x0800_0000)
            .spawn()
            .unwrap();
        let job = crate::runner::child_job(&child).unwrap();
        std::fs::write(folder.path().join("go"), []).unwrap();
        let error = wait_for_class(
            &mut child,
            &job,
            || false,
            Duration::from_millis(500),
            "STATIC",
        )
        .unwrap_err();
        assert!(error.contains("ventana cerró"), "{error}");
        assert_eq!(active(job.as_raw_handle()).unwrap(), 0);
    }
}
