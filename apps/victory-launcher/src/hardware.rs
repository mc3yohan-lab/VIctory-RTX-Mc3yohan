//! Hardware probing: NVIDIA GPU + VRAM tier (`nvidia-smi`), system RAM /
//! commit (`GlobalMemoryStatusEx`), logical CPU count, and Xbox / PlayStation
//! controller detection (XInput + Raw Input HID).

use std::process::Command;
use std::sync::Mutex;
use std::time::{Duration, Instant};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VramTier {
    Gb8,
    Gb12,
    Gb16,
    Gb24Plus,
}

impl VramTier {
    pub fn from_mb(total_mb: u32) -> Self {
        if total_mb > 0 && total_mb < 10_000 {
            Self::Gb8
        } else if (10_000..14_000).contains(&total_mb) {
            Self::Gb12
        } else if (14_000..20_000).contains(&total_mb) {
            Self::Gb16
        } else {
            Self::Gb24Plus
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Gb8 => "8GB",
            Self::Gb12 => "12GB",
            Self::Gb16 => "16GB",
            Self::Gb24Plus => "24GB+",
        }
    }

    pub fn default_windows_bar_mb(self) -> u32 {
        match self {
            Self::Gb8 => 1536,
            Self::Gb12 => 2048,
            Self::Gb16 => 3584,
            Self::Gb24Plus => 8192,
        }
    }

    pub fn default_mip_drop(self) -> u32 {
        match self {
            Self::Gb8 => 2,
            Self::Gb12 | Self::Gb16 => 1,
            Self::Gb24Plus => 0,
        }
    }

    pub fn default_card_buffers_min_kb(self) -> u32 {
        match self {
            Self::Gb8 => 1024,
            Self::Gb12 | Self::Gb16 | Self::Gb24Plus => 64,
        }
    }

    pub fn default_card_buffers_mb(self) -> Option<u32> {
        match self {
            Self::Gb12 => Some(2560),
            Self::Gb16 => Some(4096),
            Self::Gb8 | Self::Gb24Plus => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct HardwareInfo {
    pub gpu_name: String,
    pub vram_total_mb: u32,
    pub vram_used_mb: u32,
    pub gpu_util_pct: u32,
    pub vram_tier: VramTier,
    pub is_blackwell: bool,
    pub ram_total_gb: f64,
    pub ram_free_gb: f64,
    pub commit_free_gb: f64,
    pub logical_cpus: usize,
}

impl HardwareInfo {
    pub fn probe() -> Self {
        let (gpu_name, gpu_util_pct, vram_used_mb, vram_total_mb) = probe_nvidia_smi();
        let vram_tier = VramTier::from_mb(vram_total_mb);
        let is_blackwell = is_rtx_50_series(&gpu_name);
        let (ram_total_gb, ram_free_gb, commit_free_gb) = probe_memory_gb();
        let logical_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8);

        Self {
            gpu_name,
            vram_total_mb,
            vram_used_mb,
            gpu_util_pct,
            vram_tier,
            is_blackwell,
            ram_total_gb,
            ram_free_gb,
            commit_free_gb,
            logical_cpus,
        }
    }

    pub fn auto_ram16(&self) -> bool {
        self.ram_total_gb > 0.0 && self.ram_total_gb <= 16.5
    }

    pub fn recommended_shader_jobs(&self) -> usize {
        self.logical_cpus.saturating_sub(2).clamp(2, 24)
    }

    pub fn default_wide_threads(&self) -> Option<u32> {
        if self.logical_cpus >= 16 {
            Some(8)
        } else if self.logical_cpus >= 12 {
            Some(6)
        } else {
            None
        }
    }

    pub fn summary_line(&self) -> String {
        let vram_str = if self.vram_total_mb > 0 {
            format!("{} GB · Tier {}", self.vram_total_mb.div_ceil(1024), self.vram_tier.label())
        } else {
            format!("Tier {}", self.vram_tier.label())
        };
        let ram_str = if self.ram_total_gb > 0.0 {
            format!("{:.0} GB RAM", self.ram_total_gb)
        } else {
            "RAM ?".to_owned()
        };
        format!("{} ({})  ·  {}  ·  {} threads", self.gpu_name, vram_str, ram_str, self.logical_cpus)
    }
}

pub fn is_rtx_50_series(gpu_name: &str) -> bool {
    let upper = gpu_name.to_ascii_uppercase();
    if let Some(pos) = upper.find("RTX") {
        let rest = upper[pos + 3..].trim_start();
        let bytes = rest.as_bytes();
        if bytes.len() >= 4
            && bytes[0] == b'5'
            && bytes[1] == b'0'
            && bytes[2].is_ascii_digit()
            && bytes[3].is_ascii_digit()
        {
            return true;
        }
    }
    false
}

fn probe_nvidia_smi() -> (String, u32, u32, u32) {
    let mut cmd = Command::new("nvidia-smi");
    cmd.args([
        "--query-gpu=name,utilization.gpu,memory.used,memory.total",
        "--format=csv,noheader,nounits",
    ]);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let Ok(out) = cmd.output() else {
        return ("NVIDIA RTX (auto)".to_owned(), 0, 0, 0);
    };
    if !out.status.success() {
        return ("NVIDIA RTX (auto)".to_owned(), 0, 0, 0);
    }
    let text = String::from_utf8_lossy(&out.stdout);
    let Some(first) = text.lines().find(|l| !l.trim().is_empty()) else {
        return ("NVIDIA RTX (auto)".to_owned(), 0, 0, 0);
    };
    let parts: Vec<&str> = first.split(',').map(str::trim).collect();
    if parts.len() < 4 {
        return (first.trim().to_owned(), 0, 0, 0);
    }
    let name = parts[0].to_owned();
    let util = parts[1].parse::<f64>().unwrap_or(0.0) as u32;
    let used = parts[2].parse::<f64>().unwrap_or(0.0) as u32;
    let total = parts[3].parse::<f64>().unwrap_or(0.0) as u32;
    (name, util, used, total)
}

#[cfg(windows)]
fn probe_memory_gb() -> (f64, f64, f64) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status = MEMORYSTATUSEX {
        dwLength: std::mem::size_of::<MEMORYSTATUSEX>() as u32,
        dwMemoryLoad: 0,
        ullTotalPhys: 0,
        ullAvailPhys: 0,
        ullTotalPageFile: 0,
        ullAvailPageFile: 0,
        ullTotalVirtual: 0,
        ullAvailVirtual: 0,
        ullAvailExtendedVirtual: 0,
    };
    // SAFETY: valid MEMORYSTATUSEX with dwLength initialized.
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) };
    if ok == 0 {
        return (0.0, 0.0, 0.0);
    }
    const GIB: f64 = (1u64 << 30) as f64;
    (
        status.ullTotalPhys as f64 / GIB,
        status.ullAvailPhys as f64 / GIB,
        status.ullAvailPageFile as f64 / GIB,
    )
}

#[cfg(not(windows))]
fn probe_memory_gb() -> (f64, f64, f64) {
    (0.0, 0.0, 0.0)
}

/// Connected gamepads (Xbox XInput + PlayStation USB HID).
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Controllers {
    pub xbox: u32,
    pub playstation: Vec<&'static str>,
}

impl Controllers {
    pub fn describe(&self, kbm_enabled: bool) -> (String, &'static str, bool) {
        let mut parts = Vec::new();
        match self.xbox {
            0 => {}
            1 => parts.push("Xbox controller".to_owned()),
            n => parts.push(format!("{n} Xbox controllers")),
        }
        for name in &self.playstation {
            parts.push(format!("{name} (USB)"));
        }
        if kbm_enabled {
            parts.push("GTA Keyboard + Mouse-Look (F2/~)".to_owned());
        }
        if parts.is_empty() {
            (
                "No pad detected: plug in an Xbox / PlayStation pad, or toggle GTA KBM Mouse-Look".to_owned(),
                "None",
                false,
            )
        } else {
            let badge = match (self.xbox > 0, !self.playstation.is_empty(), kbm_enabled) {
                (true, true, _) => "Xbox + PS",
                (true, false, false) => "Xbox",
                (false, true, false) => "PlayStation",
                (false, false, true) => "KBM",
                (true, false, true) => "Xbox + KBM",
                (false, true, true) => "PS + KBM",
                (false, false, false) => "None",
            };
            (parts.join("  ·  "), badge, true)
        }
    }
}

pub fn sony_name(pid: u32) -> Option<&'static str> {
    match pid {
        0x05C4 | 0x09CC => Some("DualShock 4"),
        0x0BA0 => Some("DualShock 4 (wireless)"),
        0x0CE6 => Some("DualSense"),
        0x0DF2 => Some("DualSense Edge"),
        _ => None,
    }
}

/// Poll connected controllers at most once every 2 seconds.
pub fn detected_controllers() -> Controllers {
    static CACHE: Mutex<Option<(Instant, Controllers)>> = Mutex::new(None);
    let mut guard = CACHE.lock().unwrap_or_else(|e| e.into_inner());
    if let Some((at, cached)) = guard.as_ref()
        && at.elapsed() < Duration::from_secs(2)
    {
        return cached.clone();
    }
    let fresh = poll_controllers_now();
    *guard = Some((Instant::now(), fresh.clone()));
    fresh
}

#[cfg(windows)]
fn poll_controllers_now() -> Controllers {
    use windows_sys::Win32::UI::Input::{
        GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RID_DEVICE_INFO,
        RIDI_DEVICEINFO, RIM_TYPEHID,
    };
    use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};

    let xbox = (0..4u32)
        .filter(|&slot| {
            // SAFETY: zeroed XINPUT_STATE written by XInputGetState.
            let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
            unsafe { XInputGetState(slot, &mut state) == 0 }
        })
        .count() as u32;

    let mut playstation = Vec::new();
    let mut count: u32 = 0;
    let entry_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    // SAFETY: query count of raw input devices.
    if unsafe { GetRawInputDeviceList(std::ptr::null_mut(), &mut count, entry_size) } != u32::MAX
        && count > 0
    {
        // SAFETY: zeroed Vec of RAWINPUTDEVICELIST structs.
        let mut list: Vec<RAWINPUTDEVICELIST> =
            vec![unsafe { std::mem::zeroed() }; count as usize];
        // SAFETY: buffer holds `count` elements of `entry_size`.
        let got = unsafe { GetRawInputDeviceList(list.as_mut_ptr(), &mut count, entry_size) };
        if got != u32::MAX {
            for dev in &list[..got as usize] {
                if dev.dwType != RIM_TYPEHID {
                    continue;
                }
                // SAFETY: zeroed RID_DEVICE_INFO with cbSize initialized.
                let mut info: RID_DEVICE_INFO = unsafe { std::mem::zeroed() };
                info.cbSize = std::mem::size_of::<RID_DEVICE_INFO>() as u32;
                let mut size = info.cbSize;
                // SAFETY: valid device handle and sized info buffer.
                let res = unsafe {
                    GetRawInputDeviceInfoW(
                        dev.hDevice,
                        RIDI_DEVICEINFO,
                        (&raw mut info).cast(),
                        &mut size,
                    )
                };
                if res != u32::MAX {
                    // SAFETY: dwType checked as RIM_TYPEHID; Anonymous.hid is active.
                    let hid = unsafe { info.Anonymous.hid };
                    if hid.dwVendorId == 0x054C
                        && let Some(name) = sony_name(hid.dwProductId)
                        && !playstation.contains(&name)
                    {
                        playstation.push(name);
                    }
                }
            }
        }
    }

    Controllers { xbox, playstation }
}

#[cfg(not(windows))]
fn poll_controllers_now() -> Controllers {
    Controllers::default()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vram_tier_classification_matches_launcher_script() {
        assert_eq!(VramTier::from_mb(8192), VramTier::Gb8);
        assert_eq!(VramTier::from_mb(12288), VramTier::Gb12);
        assert_eq!(VramTier::from_mb(16384), VramTier::Gb16);
        assert_eq!(VramTier::from_mb(24576), VramTier::Gb24Plus);
        assert_eq!(VramTier::from_mb(0), VramTier::Gb24Plus);
        assert_eq!(VramTier::Gb24Plus.default_windows_bar_mb(), 8192);
        assert_eq!(VramTier::Gb8.default_mip_drop(), 2);
    }

    #[test]
    fn blackwell_detection_matches_rtx_50_series() {
        assert!(is_rtx_50_series("NVIDIA GeForce RTX 5070 Ti"));
        assert!(is_rtx_50_series("NVIDIA GeForce RTX 5050 Laptop GPU"));
        assert!(!is_rtx_50_series("NVIDIA GeForce RTX 4090"));
        assert!(!is_rtx_50_series("NVIDIA GeForce RTX 3060"));
    }
}
