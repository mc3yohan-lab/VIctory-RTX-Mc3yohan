//! On-demand present-to-present PIX capture, armed before D3D12 device creation.

use std::ffi::OsStr;
use std::os::windows::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::sync::atomic::{AtomicBool, Ordering};

use anyhow::{Context, Result, ensure};
use windows_sys::Win32::System::LibraryLoader::{
    GetModuleHandleExW, GetProcAddress, LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR,
    LOAD_LIBRARY_SEARCH_SYSTEM32, LoadLibraryExW,
};

// Signature of CaptureNextFrame in Microsoft's WinPixEventRuntime pix3_win.h.
type NextFrame = unsafe extern "system" fn(*const u16, u32) -> i32;

struct Request {
    marker: PathBuf,
    output: PathBuf,
    queued: AtomicBool,
    capture: NextFrame,
}

static REQUEST: OnceLock<Option<Request>> = OnceLock::new();

fn wide(path: &OsStr) -> Vec<u16> {
    path.encode_wide().chain(Some(0)).collect()
}

fn installed_capturer() -> Result<PathBuf> {
    let root = PathBuf::from(std::env::var_os("ProgramFiles").context("ProgramFiles not set")?);
    ensure!(root.is_absolute(), "ProgramFiles must be absolute");
    let dir = root.join("Microsoft PIX");
    let mut versions = Vec::new();
    for entry in dir
        .read_dir()
        .context("reading the Microsoft PIX installation")?
    {
        let entry = entry?;
        let name = entry.file_name();
        let Some(version) = name.to_str().and_then(|s| {
            s.split('.')
                .map(str::parse::<u32>)
                .collect::<Result<Vec<_>, _>>()
                .ok()
        }) else {
            continue;
        };
        let dll = entry.path().join("WinPixGpuCapturer.dll");
        if dll.is_file() {
            versions.push((version, dll));
        }
    }
    versions
        .into_iter()
        .max()
        .map(|(_, dll)| dll)
        .context("Microsoft PIX GPU capturer not found")
}

fn arm(dir: &Path) -> Result<Request> {
    ensure!(dir.is_absolute(), "PIX capture directory must be absolute");
    victory_core::ensure_outside_repo(dir)?;
    std::fs::create_dir_all(dir)?;
    let output = dir.join(format!("frame-{}.wpix", std::process::id()));
    victory_core::ensure_outside_repo(&output)?;
    ensure!(!output.exists(), "PIX capture output already exists");
    let mut module = std::ptr::null_mut();
    // SAFETY: valid UTF-16 name and output handle. Keep one module reference
    // for the process lifetime so the capture function remains callable.
    if unsafe {
        GetModuleHandleExW(
            0,
            windows_sys::core::w!("WinPixGpuCapturer.dll"),
            &mut module,
        )
    } == 0
    {
        let path = wide(installed_capturer()?.as_os_str());
        // SAFETY: absolute installed DLL path. Dependencies resolve beside
        // PIX or in System32, independently of the game's working directory.
        module = unsafe {
            LoadLibraryExW(
                path.as_ptr(),
                std::ptr::null_mut(),
                LOAD_LIBRARY_SEARCH_DLL_LOAD_DIR | LOAD_LIBRARY_SEARCH_SYSTEM32,
            )
        };
        ensure!(
            !module.is_null(),
            "loading Microsoft PIX: {}",
            std::io::Error::last_os_error()
        );
    }
    // SAFETY: Microsoft publishes the ABI for this export in pix3_win.h.
    let capture = unsafe {
        let proc = GetProcAddress(module, c"CaptureNextFrame".as_ptr().cast())
            .context("missing PIX CaptureNextFrame")?;
        std::mem::transmute::<unsafe extern "system" fn() -> isize, NextFrame>(proc)
    };
    Ok(Request {
        marker: dir.join("pix-next"),
        output,
        queued: AtomicBool::new(false),
        capture,
    })
}

pub(crate) fn initialize() {
    REQUEST.get_or_init(|| {
        let dir = std::env::var_os("VICTORY_PIX_CAPTURE_DIR")?;
        match arm(Path::new(&dir)) {
            Ok(request) => {
                tracing::info!(marker = %request.marker.display(), "PIX GPU capture armed");
                Some(request)
            }
            Err(error) => {
                tracing::warn!(%error, "could not arm PIX GPU capture");
                None
            }
        }
    });
}

pub(super) fn on_present(frame: u64) {
    if let Some(Some(request)) = REQUEST.get() {
        request.on_present(frame);
    }
}

impl Request {
    fn on_present(&self, frame: u64) {
        if self.queued.load(Ordering::Relaxed)
            || !self.marker.is_file()
            || self.queued.swap(true, Ordering::Relaxed)
        {
            return;
        }
        let filename = wide(self.output.as_os_str());
        // SAFETY: the loaded capturer remains resident and reads this UTF-16
        // filename during the synchronous API call. PIX writes the frame later.
        let hr = unsafe { (self.capture)(filename.as_ptr(), 1) };
        if hr >= 0 {
            tracing::info!(frame, path = %self.output.display(), "PIX next-frame request accepted");
        } else {
            tracing::warn!(
                frame,
                hresult = format_args!("{hr:#010x}"),
                "PIX next-frame request failed"
            );
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicU32;

    #[test]
    fn marker_requests_one_frame_once() {
        static FRAMES: AtomicU32 = AtomicU32::new(0);
        unsafe extern "system" fn capture(_: *const u16, count: u32) -> i32 {
            FRAMES.fetch_add(count, Ordering::Relaxed);
            0
        }
        let dir = tempfile::tempdir().unwrap();
        let req = Request {
            marker: dir.path().join("pix-next"),
            output: dir.path().join("test.wpix"),
            queued: AtomicBool::new(false),
            capture,
        };
        req.on_present(1);
        assert_eq!(FRAMES.load(Ordering::Relaxed), 0);
        std::fs::write(&req.marker, []).unwrap();
        req.on_present(2);
        req.on_present(3);
        assert_eq!(FRAMES.load(Ordering::Relaxed), 1);
    }
}
