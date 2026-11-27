//! Detección de hardware Windows: GPU NVIDIA / VRAM (`nvidia-smi`), arquitectura
//! RTX 50xx Blackwell, RAM física y memoria virtual (commit), elevación de
//! administrador y mandos Xbox (XInput) / PlayStation (HID).

use std::os::windows::process::CommandExt;
use std::process::Command;
use std::time::{Duration, Instant};

const CREATE_NO_WINDOW: u32 = 0x0800_0000;

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
            Self::Gb8 => "8 GB",
            Self::Gb12 => "12 GB",
            Self::Gb16 => "16 GB",
            Self::Gb24Plus => "24 GB+",
        }
    }

    pub fn friendly_tier_es(self) -> &'static str {
        match self {
            Self::Gb8 => "Optimizado 8 GB (720p fluido)",
            Self::Gb12 => "Gama Media-Alta (12 GB)",
            Self::Gb16 => "Alto Rendimiento (16 GB)",
            Self::Gb24Plus => "Modo Ultra Automático (24 GB+)",
        }
    }

    pub fn default_windows_bar_mb(self) -> u32 {
        match self {
            Self::Gb8 => 1024,
            Self::Gb12 => 2048,
            Self::Gb16 => 3584,
            Self::Gb24Plus => 8192,
        }
    }

    pub fn default_mip_drop(self) -> u8 {
        match self {
            Self::Gb8 => 2,
            Self::Gb12 | Self::Gb16 => 1,
            Self::Gb24Plus => 0,
        }
    }

    pub fn default_card_buffers_min_kb(self) -> u32 {
        match self {
            Self::Gb8 | Self::Gb12 | Self::Gb16 => 1024,
            Self::Gb24Plus => 256,
        }
    }

    pub fn default_card_buffers_mb(self) -> u32 {
        match self {
            Self::Gb8 => 1280,
            Self::Gb12 => 2560,
            Self::Gb16 => 4096,
            Self::Gb24Plus => 0,
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
    pub is_elevated_admin: bool,
    pub probe_done: bool,
}

impl HardwareInfo {
    /// Lectura instantánea (< 0.5 ms) para abrir la ventana sin bloqueos:
    /// RAM y núcleos por Win32 nativo; GPU (`nvidia-smi`) llega en segundo plano.
    pub fn probe_fast() -> Self {
        let (ram_total_gb, ram_free_gb, commit_free_gb) = probe_memory_gb();
        let logical_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8);
        let is_elevated_admin = is_process_elevated();
        Self {
            gpu_name: "Detectando tarjeta NVIDIA...".to_owned(),
            vram_total_mb: 0,
            vram_used_mb: 0,
            gpu_util_pct: 0,
            vram_tier: VramTier::Gb12,
            is_blackwell: false,
            ram_total_gb,
            ram_free_gb,
            commit_free_gb,
            logical_cpus,
            is_elevated_admin,
            probe_done: false,
        }
    }

    pub fn probe() -> Self {
        let (gpu_name, gpu_util_pct, vram_used_mb, vram_total_mb) = probe_nvidia_smi();
        let vram_tier = VramTier::from_mb(vram_total_mb);
        let is_blackwell = is_rtx_50_series(&gpu_name);
        let (ram_total_gb, ram_free_gb, commit_free_gb) = probe_memory_gb();
        let logical_cpus = std::thread::available_parallelism()
            .map(|n| n.get())
            .unwrap_or(8);
        let is_elevated_admin = is_process_elevated();

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
            is_elevated_admin,
            probe_done: true,
        }
    }

    pub fn auto_wide_threads(&self) -> u32 {
        if self.logical_cpus >= 16 {
            8
        } else if self.logical_cpus >= 12 {
            6
        } else {
            0
        }
    }

    pub fn low_ram_auto(&self) -> bool {
        (self.ram_total_gb > 0.0 && self.ram_total_gb <= 24.5)
            || (self.commit_free_gb > 0.0 && self.commit_free_gb < 8.0)
    }

    pub fn summary_line_es(&self) -> String {
        let vram_gb = (self.vram_total_mb as f64 / 1024.0).round() as u32;
        if self.vram_total_mb > 0 {
            format!(
                "{} ({} GB) · {:.0} GB RAM · {}",
                self.gpu_name,
                vram_gb,
                self.ram_total_gb,
                self.vram_tier.friendly_tier_es()
            )
        } else {
            format!(
                "{} · {:.0} GB RAM · {} hilos",
                self.gpu_name, self.ram_total_gb, self.logical_cpus
            )
        }
    }
}

pub fn is_rtx_50_series(gpu_name: &str) -> bool {
    let up = gpu_name.to_ascii_uppercase();
    if let Some(idx) = up.find("RTX") {
        let tail = up[idx + 3..].trim_start();
        let digits: String = tail.chars().take_while(|c| c.is_ascii_digit()).collect();
        return digits.len() == 4 && digits.starts_with("50");
    }
    false
}

fn probe_nvidia_smi() -> (String, u32, u32, u32) {
    let out = Command::new("nvidia-smi")
        .args([
            "--query-gpu=name,utilization.gpu,memory.used,memory.total",
            "--format=csv,noheader,nounits",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    if let Ok(out) = out
        && out.status.success()
    {
        let text = String::from_utf8_lossy(&out.stdout);
        if let Some(first) = text.lines().find(|l| !l.trim().is_empty()) {
            let parts: Vec<&str> = first.split(',').map(str::trim).collect();
            if parts.len() >= 4 {
                let name = parts[0].to_owned();
                let util = parts[1].parse::<f64>().unwrap_or(0.0).round() as u32;
                let used = parts[2].parse::<f64>().unwrap_or(0.0).round() as u32;
                let total = parts[3].parse::<f64>().unwrap_or(0.0).round() as u32;
                return (name, util, used, total);
            }
        }
    }
    ("GPU DirectX 12".to_owned(), 0, 0, 0)
}

fn probe_memory_gb() -> (f64, f64, f64) {
    use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
    let mut status: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
    status.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
    let ok = unsafe { GlobalMemoryStatusEx(&mut status) } != 0;
    if !ok {
        return (0.0, 0.0, 0.0);
    }
    let gib = (1024.0_f64).powi(3);
    (
        status.ullTotalPhys as f64 / gib,
        status.ullAvailPhys as f64 / gib,
        status.ullAvailPageFile as f64 / gib,
    )
}

/// Comprueba al instante (< 0.05 ms, sin subprocesos) mediante el token Win32
/// si el launcher fue abierto como Administrador (abrir como Administrador
/// rompe la ruta normal de DXCache de NVIDIA).
fn is_process_elevated() -> bool {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::Security::{
        GetTokenInformation, TOKEN_ELEVATION, TOKEN_QUERY, TokenElevation,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    let mut token: HANDLE = std::ptr::null_mut();
    // SAFETY: consultar el token de lectura del proceso actual y cerrarlo al terminar.
    let ok = unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &mut token) } != 0;
    if !ok || token.is_null() {
        return false;
    }
    let mut elevation = TOKEN_ELEVATION { TokenIsElevated: 0 };
    let mut returned: u32 = 0;
    let queried = unsafe {
        GetTokenInformation(
            token,
            TokenElevation,
            (&raw mut elevation).cast(),
            std::mem::size_of::<TOKEN_ELEVATION>() as u32,
            &mut returned,
        )
    } != 0;
    unsafe {
        CloseHandle(token);
    }
    queried && elevation.TokenIsElevated != 0
}

#[derive(Clone, Debug, Default)]
pub struct ControllerStatus {
    pub xbox_count: u8,
    pub playstation_ds4: bool,
    pub playstation_dualsense: bool,
}

impl ControllerStatus {
    pub fn any(&self) -> bool {
        self.xbox_count > 0 || self.playstation_ds4 || self.playstation_dualsense
    }

    pub fn summary_es(&self) -> String {
        let mut parts = Vec::new();
        if self.xbox_count == 1 {
            parts.push("Mando Xbox conectado".to_owned());
        } else if self.xbox_count > 1 {
            parts.push(format!("{} mandos Xbox conectados", self.xbox_count));
        }
        if self.playstation_dualsense {
            parts.push("PlayStation DualSense conectado".to_owned());
        }
        if self.playstation_ds4 {
            parts.push("PlayStation DualShock 4 conectado".to_owned());
        }
        if parts.is_empty() {
            "Sin mando detectado (conecta Xbox/PS por USB o usa Teclado y Ratón)".to_owned()
        } else {
            parts.join(" · ")
        }
    }

    pub fn badge_es(&self) -> String {
        match (
            self.xbox_count > 0,
            self.playstation_dualsense,
            self.playstation_ds4,
        ) {
            (true, false, false) => "Xbox".to_owned(),
            (false, true, false) => "DualSense".to_owned(),
            (false, false, true) => "DualShock 4".to_owned(),
            (false, false, false) => "Sin mando".to_owned(),
            _ => "Conectado".to_owned(),
        }
    }
}

#[derive(Default)]
pub struct ControllerPoller {
    last_check: Option<Instant>,
    cached: ControllerStatus,
}

impl ControllerPoller {
    pub fn poll(&mut self) -> &ControllerStatus {
        let due = self
            .last_check
            .is_none_or(|t| t.elapsed() >= Duration::from_secs(2));
        if due {
            self.cached = detect_controllers();
            self.last_check = Some(Instant::now());
        }
        &self.cached
    }
}

fn detect_controllers() -> ControllerStatus {
    let mut status = ControllerStatus {
        xbox_count: count_xinput_controllers(),
        playstation_ds4: false,
        playstation_dualsense: false,
    };
    detect_sony_hid(&mut status);
    status
}

fn count_xinput_controllers() -> u8 {
    use windows_sys::Win32::UI::Input::XboxController::{XINPUT_STATE, XInputGetState};
    let mut count = 0u8;
    for slot in 0..4u32 {
        let mut state: XINPUT_STATE = unsafe { std::mem::zeroed() };
        let res = unsafe { XInputGetState(slot, &mut state) };
        if res == 0 {
            count += 1;
        }
    }
    count
}

fn detect_sony_hid(status: &mut ControllerStatus) {
    use windows_sys::Win32::UI::Input::{
        GetRawInputDeviceInfoW, GetRawInputDeviceList, RAWINPUTDEVICELIST, RID_DEVICE_INFO,
        RID_DEVICE_INFO_HID, RIDI_DEVICEINFO, RIM_TYPEHID,
    };

    let mut count = 0u32;
    let entry_size = std::mem::size_of::<RAWINPUTDEVICELIST>() as u32;
    if unsafe { GetRawInputDeviceList(std::ptr::null_mut(), &mut count, entry_size) } == u32::MAX
        || count == 0
    {
        return;
    }
    let mut list = vec![
        RAWINPUTDEVICELIST {
            hDevice: std::ptr::null_mut(),
            dwType: 0,
        };
        count as usize
    ];
    let got = unsafe { GetRawInputDeviceList(list.as_mut_ptr(), &mut count, entry_size) };
    if got == u32::MAX {
        return;
    }
    for dev in list.into_iter().take(got as usize) {
        if dev.dwType != RIM_TYPEHID {
            continue;
        }
        let mut info: RID_DEVICE_INFO = unsafe { std::mem::zeroed() };
        info.cbSize = std::mem::size_of::<RID_DEVICE_INFO>() as u32;
        let mut info_size = info.cbSize;
        let rc = unsafe {
            GetRawInputDeviceInfoW(
                dev.hDevice,
                RIDI_DEVICEINFO,
                std::ptr::from_mut(&mut info).cast(),
                &mut info_size,
            )
        };
        if rc == u32::MAX {
            continue;
        }
        let hid: RID_DEVICE_INFO_HID = unsafe { info.Anonymous.hid };
        if hid.dwVendorId == 0x054C {
            match hid.dwProductId {
                0x0CE6 | 0x0DF2 => status.playstation_dualsense = true,
                0x05C4 | 0x09CC | 0x0BA0 => status.playstation_ds4 = true,
                _ => {}
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn blackwell_detection_matches_rtx_50_series() {
        assert!(is_rtx_50_series("NVIDIA GeForce RTX 5070 Ti"));
        assert!(is_rtx_50_series("NVIDIA GeForce RTX 5050 Laptop GPU"));
        assert!(!is_rtx_50_series("NVIDIA GeForce RTX 4090"));
        assert!(!is_rtx_50_series("NVIDIA GeForce RTX 3060 Ti"));
    }

    #[test]
    fn vram_tier_classification_matches_launcher_script() {
        assert_eq!(VramTier::from_mb(8192), VramTier::Gb8);
        assert_eq!(VramTier::from_mb(12288), VramTier::Gb12);
        assert_eq!(VramTier::from_mb(16384), VramTier::Gb16);
        assert_eq!(VramTier::from_mb(24564), VramTier::Gb24Plus);
        assert_eq!(VramTier::Gb8.default_windows_bar_mb(), 1024);
        assert_eq!(VramTier::Gb16.default_card_buffers_min_kb(), 1024);
        assert_eq!(VramTier::Gb24Plus.default_windows_bar_mb(), 8192);
    }
}
