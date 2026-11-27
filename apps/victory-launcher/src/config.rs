//! Launcher configuration, presets, `victory.toml` sync, and full `VICTORY_*`
//! environment generation matching `scripts/play-nvidia.ps1`.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hardware::{HardwareInfo, VramTier};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    /// Series X 1920x1080 output (720p internal -> title upscaler 6, NO FSR, fastest clean visuals).
    Fast1080p,
    /// Series X native 1920x1080 internal -> 1080p output (`Scale 1.0`, sharpest 1080p, NO FSR).
    Native1080p,
    /// Series X native 1280x720 internal -> 720p output (`Scale 1.0`, lightest GPU load).
    Ultra720p,
    /// Native PC AMD FidelityFX FSR 3.1.x / 4.x 4K upscaling (`VICTORY_FSR_4K`).
    Fsr4K,
    /// Fallback stability profile (disables port patches, ReBAR windows, and thin lists).
    Safe,
    /// User-customized switches on the Settings tab.
    Custom,
}

impl Preset {
    pub const ALL: [Self; 6] = [
        Self::Fast1080p,
        Self::Native1080p,
        Self::Ultra720p,
        Self::Fsr4K,
        Self::Safe,
        Self::Custom,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Self::Fast1080p => "Fast 1080p",
            Self::Native1080p => "Native 1080p",
            Self::Ultra720p => "720p Ultra",
            Self::Fsr4K => "4K FSR 3.1",
            Self::Safe => "Safe",
            Self::Custom => "Custom",
        }
    }

    pub fn summary(self) -> &'static str {
        match self {
            Self::Fast1080p => "Recommended · 1080p output (720p internal), pure NVIDIA, CS_RaytracingResolve off",
            Self::Native1080p => "Sharpest 1080p · 1920x1080 native internal (scale 1.0), pure NVIDIA",
            Self::Ultra720p => "Max FPS · 1280x720 native internal (scale 1.0), lightest GPU load",
            Self::Fsr4K => "4K PC FidelityFX · requires amd_fidelityfx_upscaler_dx12.dll in LocalAppData",
            Self::Safe => "Fallback · disables port patches, ReBAR windows, and thin lists",
            Self::Custom => "Custom settings configured in Settings tab",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputRes {
    P720,
    P1080,
    P1440,
    P4K,
}

impl OutputRes {
    pub fn label(self) -> &'static str {
        match self {
            Self::P720 => "1280x720 (720p)",
            Self::P1080 => "1920x1080 (1080p)",
            Self::P1440 => "2560x1440 (1440p)",
            Self::P4K => "3840x2160 (4K)",
        }
    }

    pub fn env_value(self) -> Option<&'static str> {
        match self {
            Self::P720 => Some("1280x720"),
            Self::P1080 => Some("1920x1080"),
            Self::P1440 => Some("2560x1440"),
            Self::P4K => None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsrMode {
    Performance,
    Balanced,
    Quality,
}

impl FsrMode {
    pub fn env_value(self) -> &'static str {
        match self {
            Self::Performance => "performance",
            Self::Balanced => "balanced",
            Self::Quality => "quality",
        }
    }

    pub fn label(self) -> &'static str {
        match self {
            Self::Performance => "Performance (1080p -> 4K)",
            Self::Balanced => "Balanced (1270p -> 4K)",
            Self::Quality => "Quality (1440p -> 4K)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TriState {
    Auto,
    On,
    Off,
}

impl TriState {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::On => "On",
            Self::Off => "Off",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum ConsoleModel {
    Auto,
    SeriesX,
    SeriesS,
}

impl ConsoleModel {
    pub fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto (Series X, 8GB -> Series S)",
            Self::SeriesX => "Series X",
            Self::SeriesS => "Series S",
        }
    }
}

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherConfig {
    pub game_dir: String,
    pub preset: Preset,
    pub console: ConsoleModel,
    pub output: OutputRes,
    /// `0.0` = title default (0.667), otherwise clamped safely to `0.50..=1.00`.
    pub render_scale: f32,
    pub fps_cap: u32,
    pub fsr_4k: bool,
    pub fsr_mode: FsrMode,
    pub smooth_motion: bool,
    pub diag_build: bool,
    pub gpu_timing: bool,
    pub panel_hud: bool,
    // Input
    pub kbm_gta: bool,
    pub mouse_sens: f32,
    pub mouse_invert_y: bool,
    // Performance / NVIDIA tuning
    pub skip_rt_resolve: bool,
    pub ray_tracing: bool,
    pub send_bar: bool,
    pub wc_no_back: bool,
    pub wc_bar: bool,
    pub tex_store: bool,
    pub wait_textures: bool,
    pub thin_lists: TriState,
    pub ram16_profile: TriState,
    /// `-1` = auto by detected GPU VRAM tier.
    pub windows_bar_mb: i32,
    /// `-1` = auto by detected GPU VRAM tier.
    pub mip_drop: i32,
    /// `0` = auto by logical CPU count.
    pub wide_threads: u32,
    pub thin_threads: u32,
    // Renderer Ports
    pub port_constants: bool,
    pub port_cbv: bool,
    pub port_guard: bool,
    pub port_journal: bool,
    pub port_rt: bool,
    pub port_descriptors: bool,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            game_dir: String::new(),
            preset: Preset::Fast1080p,
            console: ConsoleModel::Auto,
            output: OutputRes::P1080,
            render_scale: 0.0,
            fps_cap: 60,
            fsr_4k: false,
            fsr_mode: FsrMode::Performance,
            smooth_motion: false,
            diag_build: false,
            gpu_timing: false,
            panel_hud: false,
            kbm_gta: false,
            mouse_sens: 1.0,
            mouse_invert_y: false,
            skip_rt_resolve: true,
            ray_tracing: false,
            send_bar: false,
            wc_no_back: false,
            wc_bar: false,
            tex_store: false,
            wait_textures: false,
            thin_lists: TriState::Auto,
            ram16_profile: TriState::Auto,
            windows_bar_mb: -1,
            mip_drop: -1,
            wide_threads: 0,
            thin_threads: 8,
            port_constants: true,
            port_cbv: true,
            port_guard: true,
            port_journal: true,
            port_rt: true,
            port_descriptors: false,
        }
    }
}

impl LauncherConfig {
    /// Apply a named preset to display/perf fields while preserving `game_dir`,
    /// KBM preferences, and personal overrides.
    pub fn apply_preset(&mut self, preset: Preset) {
        self.preset = preset;
        match preset {
            Preset::Fast1080p => {
                self.console = ConsoleModel::Auto;
                self.output = OutputRes::P1080;
                self.render_scale = 0.0;
                self.fsr_4k = false;
                self.skip_rt_resolve = true;
                self.ray_tracing = false;
                self.thin_lists = TriState::Auto;
                self.port_constants = true;
                self.port_cbv = true;
                self.port_guard = true;
                self.port_journal = true;
                self.port_rt = true;
            }
            Preset::Native1080p => {
                self.console = ConsoleModel::SeriesX;
                self.output = OutputRes::P1080;
                self.render_scale = 1.0;
                self.fsr_4k = false;
                self.skip_rt_resolve = true;
                self.ray_tracing = false;
                self.thin_lists = TriState::Auto;
                self.port_constants = true;
                self.port_cbv = true;
                self.port_guard = true;
                self.port_journal = true;
                self.port_rt = true;
            }
            Preset::Ultra720p => {
                self.console = ConsoleModel::SeriesX;
                self.output = OutputRes::P720;
                self.render_scale = 1.0;
                self.fsr_4k = false;
                self.skip_rt_resolve = true;
                self.ray_tracing = false;
                self.thin_lists = TriState::Auto;
                self.port_constants = true;
                self.port_cbv = true;
                self.port_guard = true;
                self.port_journal = true;
                self.port_rt = true;
            }
            Preset::Fsr4K => {
                self.console = ConsoleModel::SeriesX;
                self.output = OutputRes::P4K;
                self.render_scale = 0.0;
                self.fsr_4k = true;
                self.skip_rt_resolve = true;
                self.ray_tracing = false;
                self.thin_lists = TriState::Auto;
                self.port_constants = true;
                self.port_cbv = true;
                self.port_guard = true;
                self.port_journal = true;
                self.port_rt = true;
            }
            Preset::Safe => {
                self.fsr_4k = false;
                self.ray_tracing = false;
                self.thin_lists = TriState::Off;
                self.port_constants = false;
                self.port_cbv = false;
                self.port_guard = false;
                self.port_journal = false;
                self.port_rt = false;
                self.port_descriptors = false;
            }
            Preset::Custom => {}
        }
    }

    pub fn config_path() -> PathBuf {
        local_app_data().join("victory").join("launcher.json")
    }

    pub fn load(repo_root: Option<&Path>) -> Self {
        let mut cfg = std::fs::read_to_string(Self::config_path())
            .ok()
            .and_then(|s| serde_json::from_str::<Self>(&s).ok())
            .unwrap_or_default();

        // If `game_dir` is empty, try auto-importing it from `VICTORY_GAME_DIR` or `victory.toml`.
        if cfg.game_dir.is_empty() {
            if let Ok(env_dir) = std::env::var("VICTORY_GAME_DIR")
                && !env_dir.trim().is_empty()
            {
                cfg.game_dir = env_dir;
            } else if let Some(root) = repo_root
                && let Some(found) = read_game_dir_from_toml(&root.join("victory.toml"))
            {
                cfg.game_dir = found;
            }
        }
        cfg
    }

    pub fn save(&self, repo_root: Option<&Path>) {
        let path = Self::config_path();
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_string_pretty(self) {
            let _ = std::fs::write(&path, json);
        }
        // Keep `victory.toml` in sync if the launcher lives inside a repo checkout.
        if let Some(root) = repo_root
            && !self.game_dir.is_empty()
        {
            let _ = sync_victory_toml(&root.join("victory.toml"), &self.game_dir);
        }
    }

    pub fn effective_console(&self, hw: &HardwareInfo) -> &'static str {
        match self.console {
            ConsoleModel::SeriesX => "series-x",
            ConsoleModel::SeriesS => "series-s",
            ConsoleModel::Auto => {
                if hw.vram_tier == VramTier::Gb8 && !self.fsr_4k && self.output == OutputRes::P1080
                {
                    "series-s"
                } else {
                    "series-x"
                }
            }
        }
    }

    pub fn effective_windows_bar_mb(&self, hw: &HardwareInfo) -> u32 {
        if self.windows_bar_mb >= 0 {
            self.windows_bar_mb as u32
        } else {
            hw.vram_tier.default_windows_bar_mb()
        }
    }

    pub fn effective_mip_drop(&self, hw: &HardwareInfo) -> u32 {
        if self.mip_drop >= 0 {
            self.mip_drop as u32
        } else {
            hw.vram_tier.default_mip_drop()
        }
    }

    pub fn effective_thin(&self, hw: &HardwareInfo) -> bool {
        if self.preset == Preset::Safe {
            return false;
        }
        match self.thin_lists {
            TriState::On => true,
            TriState::Off => false,
            TriState::Auto => !hw.is_blackwell,
        }
    }

    /// Build the exact `VICTORY_*` environment dictionary for `victory.exe run`.
    pub fn build_run_env(&self, hw: &HardwareInfo) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        let mut set = |k: &str, v: &str| {
            env.insert(k.to_owned(), v.to_owned());
        };

        let safe = self.preset == Preset::Safe;
        let console = self.effective_console(hw);
        let series_s = console == "series-s";

        // --- Core fast path (exact parity with scripts/play-nvidia.ps1) ---
        set("VICTORY_TEXTURE_NO_WAIT", if self.wait_textures { "0" } else { "1" });
        set("VICTORY_EXACT_NO_WAIT", if self.wait_textures { "0" } else { "1" });
        set("VICTORY_TEXTURE_STAGE_MS", "25");
        set("VICTORY_TEXTURE_CATCHUP", "1");
        set("VICTORY_EARLY_TEXTURES_MS", "12");
        set("VICTORY_BAKE_EXACT", "1");
        set("VICTORY_BAKE_SOURCES", "1");
        set("VICTORY_FAIR_DUE", "1");
        set("VICTORY_SHOW_NEW_LATER", "1");
        if !safe {
            set("VICTORY_UPLOAD_AHEAD", "1");
        }
        set("VICTORY_SHOW_AFTER", "1");
        set("VICTORY_VIDEO_PLANES", "1");
        set("VICTORY_RELAY", "2");
        set("VICTORY_CARD_STEPS", "2");
        set("VICTORY_WIDE_BACK", "1");
        set("VICTORY_CARD_REHASH_RUNS", "1");
        set("VICTORY_JOIN_COPIES", "1");
        set("VICTORY_JOIN_BARRIERS", "1");
        set("VICTORY_INDIRECT_WINDOW", "1");
        set("VICTORY_RELAY_COPY", "1");
        set("VICTORY_INDIRECT_BATCH", "2");
        set("VICTORY_INDIRECT_REST", "0");
        set("VICTORY_LAND_SKIP", "1");
        set("VICTORY_LAND_THREADS", "6");
        set("VICTORY_MAP_THREADS", "1");
        set("VICTORY_TITLE_PRIORITIES", "0");
        set("VICTORY_CARD_AWAKE", "idle");
        set("VICTORY_D3D12_DRED", "light");
        set("VICTORY_D3D12_BACKGROUND", "0");
        set("VICTORY_MEMORY_MAP", "0");
        set("VICTORY_GUARD_HOT", "50");
        set("VICTORY_WRITE_JOBS", "1");
        set("VICTORY_WATCH_TEXTURES_JOINED", "1");
        set("VICTORY_READBACK_POOL", "1");
        set("VICTORY_BATCHED_SWEEP", "1");
        set("VICTORY_PIPELINE_SAVE_ON_EXIT_ONLY", "1");

        // --- NVIDIA 1M view heap & pipeline workers ---
        set("VICTORY_TABLES_EARLY", "1");
        set("VICTORY_DYNAMIC_SLOTS", "393216");
        set("VICTORY_PIPELINE_LOWEST", "0");
        set("VICTORY_PIPELINE_WORKERS", "8");
        set("VICTORY_PIPELINE_WAIT_ONCE", "1");
        set("VICTORY_TARGET_VARIANTS", "1");
        set("VICTORY_NO_WATCHDOG", "1");
        set("VICTORY_XMA_REFILL", "1");

        // --- Shader translator keys (MUST match `shaders` & `warm`) ---
        set("VICTORY_SHADER_OPT", "63");
        set("VICTORY_WAVE_WIDE", "last");
        set("VICTORY_NAN_SCRUB", "1");
        set("VICTORY_SMALL_RECHECK", "0");

        let wide = if self.wide_threads > 0 {
            Some(self.wide_threads)
        } else {
            hw.default_wide_threads()
        };
        if let Some(n) = wide {
            set("VICTORY_WIDE_THREADS", &n.to_string());
        }

        if safe {
            set("VICTORY_UNDER_WIDE", "0");
            set("VICTORY_LAND_ECHO", "0");
            set("VICTORY_CARD_STEPS", "0");
            set("VICTORY_ASYNC_SUBMIT", "0");
            set("VICTORY_THIN", "0");
        } else {
            if self.port_constants {
                set("VICTORY_PORT_CONSTANTS", "1");
            }
            if self.port_descriptors {
                set("VICTORY_PORT_DESCRIPTORS", "1");
            }
            if self.port_cbv {
                set("VICTORY_PORT_CBV", "1");
                set("VICTORY_CONSTANT_MARKS", "1");
            }
            if self.port_guard {
                set("VICTORY_PORT_GUARD", "1");
            }
            if self.port_journal {
                set("VICTORY_PORT_JOURNAL", "1");
            }
            if self.port_rt {
                set("VICTORY_PORT_RT", "1");
            }
            set("VICTORY_FEED_ONE_CALL", "1");
            set("VICTORY_ASYNC_SUBMIT", "1");
            if self.effective_thin(hw) {
                set("VICTORY_THIN", "1");
                set("VICTORY_THIN_THREADS", &self.thin_threads.clamp(2, 16).to_string());
            } else {
                set("VICTORY_THIN", "0");
            }
            set("VICTORY_JOIN_PARTS", "8");

            let bar_mb = self.effective_windows_bar_mb(hw);
            if bar_mb > 0 {
                set("VICTORY_WINDOWS_BAR_MB", &bar_mb.to_string());
            }
            set("VICTORY_WINDOW_KB", "4096");
            set(
                "VICTORY_CARD_BUFFERS_MIN_KB",
                &hw.vram_tier.default_card_buffers_min_kb().to_string(),
            );
            if let Some(cb_mb) = hw.vram_tier.default_card_buffers_mb() {
                set("VICTORY_CARD_BUFFERS_MB", &cb_mb.to_string());
            }
            let mip = self.effective_mip_drop(hw);
            if mip > 0 {
                set("VICTORY_TEXTURE_MIP_DROP", &mip.to_string());
            }
            if self.send_bar {
                set("VICTORY_SEND_BAR", "1");
            }
            if self.wc_no_back {
                set("VICTORY_WC_NO_BACK", "1");
                set("VICTORY_WC_NO_BACK_MIN_KB", "16384");
            }
            if self.wc_bar {
                set("VICTORY_WC_BAR", "1");
            }
        }

        // RAM16 profile override
        match self.ram16_profile {
            TriState::On => set("VICTORY_RAM_16GB", "1"),
            TriState::Off => set("VICTORY_RAM_16GB", "0"),
            TriState::Auto => {}
        }

        if self.tex_store {
            let dir = local_app_data().join("victory").join("cache").join("textures");
            let _ = std::fs::create_dir_all(&dir);
            set("VICTORY_TEXTURE_STORE", &dir.to_string_lossy());
        }

        // Ambient & RT switches
        if self.ray_tracing {
            set("VICTORY_BVH_BUILDER", "1");
            set("VICTORY_SKIP_PIPELINES", "none");
            set("VICTORY_SKIP_PREFIXES", "none");
        } else {
            set("VICTORY_BVH_NO_HIT", "1");
            set("VICTORY_AMBIENT", "1.0");
            set("VICTORY_AMBIENT_AIR", "1.5");
            set("VICTORY_AMBIENT_MOST", "2.0");
            set("VICTORY_AMBIENT_SHADE", "0.7,0.5");
            set("VICTORY_AMBIENT_REFL", "2.0");
            set("VICTORY_AMBIENT_REFLECT", "0.5");
            set("VICTORY_AMBIENT_REFLECT_LEAST", "0.25");
            set("VICTORY_SKIP_PIPELINES", "none");
            if self.skip_rt_resolve {
                set("VICTORY_SKIP_PREFIXES", "CS_RaytracingResolve");
            } else {
                set("VICTORY_SKIP_PREFIXES", "none");
            }
        }

        // Display, Upscaler & Scale
        if self.fsr_4k {
            set("VICTORY_FSR_4K", self.fsr_mode.env_value());
            let ffx_dir = local_app_data().join("victory").join("fidelityfx");
            if ffx_dir.join("amd_fidelityfx_upscaler_dx12.dll").is_file() {
                set("VICTORY_FIDELITYFX_DIR", &ffx_dir.to_string_lossy());
            }
        } else if series_s {
            set("VICTORY_PLANES", "1");
            set("VICTORY_UPSCALER", "6");
        } else {
            set("VICTORY_UPSCALER", "6");
            if let Some(res) = self.output.env_value() {
                set("VICTORY_OUTPUT", res);
            }
        }

        let requested_scale = if series_s && self.render_scale <= 0.0 {
            0.667
        } else {
            self.render_scale
        };
        if requested_scale > 0.0 && !self.fsr_4k {
            let max_scale = if series_s || self.output == OutputRes::P4K {
                0.667f32
            } else {
                1.0f32
            };
            let clamped = requested_scale.clamp(0.5, max_scale);
            set("VICTORY_RENDER_SCALE", &format!("{clamped:.3}"));
        }

        if self.fps_cap > 0 {
            let interval_ms = (1000.0 / f64::from(self.fps_cap)).clamp(1.0, 1000.0);
            set("VICTORY_FRAME_INTERVAL_MS", &format!("{interval_ms:.3}"));
        }

        if self.kbm_gta {
            set("VICTORY_KBM_GTA", "1");
            if (self.mouse_sens - 1.0).abs() > 0.01 {
                set("VICTORY_MOUSE_SENS", &format!("{:.2}", self.mouse_sens.clamp(0.2, 4.0)));
            }
            if self.mouse_invert_y {
                set("VICTORY_MOUSE_INVERT_Y", "1");
            }
        }

        if self.gpu_timing {
            set("VICTORY_GPU_TIMING", "1");
            set("VICTORY_MEMORY_CATEGORIES", "1");
        }
        if self.panel_hud {
            set("VICTORY_PANEL", "1");
        }

        env
    }

    /// Build translator env keys for `victory.exe shaders` / `warm` (exact cache match).
    pub fn build_prep_env(&self) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        env.insert("VICTORY_SHADER_OPT".into(), "63".into());
        env.insert("VICTORY_WAVE_WIDE".into(), "last".into());
        env.insert("VICTORY_NAN_SCRUB".into(), "1".into());
        if self.port_cbv && self.preset != Preset::Safe {
            env.insert("VICTORY_PORT_CBV".into(), "1".into());
            env.insert("VICTORY_CONSTANT_MARKS".into(), "1".into());
        }
        if self.ray_tracing {
            env.insert("VICTORY_BVH_BUILDER".into(), "1".into());
        } else {
            env.insert("VICTORY_BVH_NO_HIT".into(), "1".into());
        }
        env
    }
}

pub fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn cache_dir() -> PathBuf {
    std::env::var_os("VICTORY_CACHE_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| local_app_data().join("victory").join("cache"))
}

pub fn data_dir() -> PathBuf {
    std::env::var_os("VICTORY_DATA_DIR")
        .map(PathBuf::from)
        .unwrap_or_else(|| local_app_data().join("victory").join("data"))
}

/// Inspect a candidate game folder for the executable, `shaders.rpf`, and `MicrosoftGame.config`.
#[derive(Clone, Debug, Default)]
pub struct GameFilesStatus {
    pub exe_path: Option<PathBuf>,
    pub exe_mb: u64,
    pub shaders_mb: u64,
    pub config_kb: u64,
}

impl GameFilesStatus {
    pub fn inspect(dir: &Path) -> Self {
        if !dir.is_dir() {
            return Self::default();
        }
        let preferred = dir.join("game_scarlett_master.exe");
        let exe_path = if preferred.is_file() {
            Some(preferred)
        } else {
            std::fs::read_dir(dir)
                .ok()
                .into_iter()
                .flatten()
                .flatten()
                .map(|e| e.path())
                .find(|p| {
                    p.is_file()
                        && p.extension()
                            .is_some_and(|ext| ext.eq_ignore_ascii_case("exe"))
                })
        };
        let exe_mb = exe_path
            .as_ref()
            .and_then(|p| p.metadata().ok())
            .map_or(0, |m| m.len() / (1024 * 1024));
        let shaders_mb = dir
            .join("shaders.rpf")
            .metadata()
            .ok()
            .map_or(0, |m| m.len() / (1024 * 1024));
        let config_kb = dir
            .join("MicrosoftGame.config")
            .metadata()
            .ok()
            .map_or(0, |m| m.len() / 1024);
        Self {
            exe_path,
            exe_mb,
            shaders_mb,
            config_kb,
        }
    }

    pub fn ready(&self) -> bool {
        self.exe_path.is_some()
    }

    pub fn detail_line(&self, dir: &str) -> String {
        if dir.is_empty() {
            return "Select your legally owned Xbox Series X|S dump folder to begin".to_owned();
        }
        let Some(exe) = &self.exe_path else {
            return format!("No title .exe found inside {dir}");
        };
        let exe_name = exe
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| "exe".to_owned());
        let mut parts = vec![format!("{exe_name} ({} MB)", self.exe_mb)];
        if self.shaders_mb > 0 {
            parts.push(format!("shaders.rpf {} MB", self.shaders_mb));
        }
        if self.config_kb > 0 {
            parts.push(format!("MicrosoftGame.config {} KB", self.config_kb));
        }
        parts.join("  ·  ")
    }
}

fn read_game_dir_from_toml(path: &Path) -> Option<String> {
    let text = std::fs::read_to_string(path).ok()?;
    let val: toml::Value = toml::from_str(&text).ok()?;
    val.get("game_dir")
        .and_then(|v| v.as_str())
        .map(str::to_owned)
}

fn sync_victory_toml(path: &Path, game_dir: &str) -> std::io::Result<()> {
    let mut table = if let Ok(text) = std::fs::read_to_string(path) {
        toml::from_str::<toml::Table>(&text).unwrap_or_default()
    } else {
        toml::Table::new()
    };
    table.insert("game_dir".into(), toml::Value::String(game_dir.to_owned()));
    let rendered = toml::to_string_pretty(&table).unwrap_or_default();
    std::fs::write(path, rendered)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rtx_4090() -> HardwareInfo {
        HardwareInfo {
            gpu_name: "NVIDIA GeForce RTX 4090".into(),
            vram_total_mb: 24576,
            vram_used_mb: 800,
            gpu_util_pct: 2,
            vram_tier: VramTier::Gb24Plus,
            is_blackwell: false,
            ram_total_gb: 64.0,
            ram_free_gb: 48.0,
            commit_free_gb: 60.0,
            logical_cpus: 24,
        }
    }

    #[test]
    fn fast_1080p_on_4090_enables_all_tuned_nvidia_switches() {
        let cfg = LauncherConfig::default();
        let env = cfg.build_run_env(&rtx_4090());
        assert_eq!(env.get("VICTORY_OUTPUT").map(String::as_str), Some("1920x1080"));
        assert_eq!(env.get("VICTORY_UPSCALER").map(String::as_str), Some("6"));
        assert_eq!(env.get("VICTORY_WINDOWS_BAR_MB").map(String::as_str), Some("8192"));
        assert_eq!(env.get("VICTORY_SKIP_PREFIXES").map(String::as_str), Some("CS_RaytracingResolve"));
        assert_eq!(env.get("VICTORY_AMBIENT_REFLECT").map(String::as_str), Some("0.5"));
        assert_eq!(env.get("VICTORY_GUARD_HOT").map(String::as_str), Some("50"));
        assert_eq!(env.get("VICTORY_WRITE_JOBS").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_WATCH_TEXTURES_JOINED").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_READBACK_POOL").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_BATCHED_SWEEP").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_PIPELINE_SAVE_ON_EXIT_ONLY").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_PORT_CBV").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_PORT_RT").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_WIDE_THREADS").map(String::as_str), Some("8"));
        assert!(!env.contains_key("VICTORY_RAM_16GB"));
    }

    #[test]
    fn blackwell_auto_disables_thin_lists() {
        let mut hw = rtx_4090();
        hw.gpu_name = "NVIDIA GeForce RTX 5070 Ti".into();
        hw.is_blackwell = true;
        let cfg = LauncherConfig::default();
        let env = cfg.build_run_env(&hw);
        assert_eq!(env.get("VICTORY_THIN").map(String::as_str), Some("0"));
        assert_eq!(env.get("VICTORY_ASYNC_SUBMIT").map(String::as_str), Some("1"));
    }

    #[test]
    fn scale_clamps_safely_on_4k_and_series_s() {
        let mut cfg = LauncherConfig::default();
        cfg.output = OutputRes::P4K;
        cfg.render_scale = 1.0;
        let env = cfg.build_run_env(&rtx_4090());
        assert_eq!(env.get("VICTORY_RENDER_SCALE").map(String::as_str), Some("0.667"));
    }
}
