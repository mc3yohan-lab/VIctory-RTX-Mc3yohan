//! Configuración persistente del launcher (`%LOCALAPPDATA%\victory\launcher.json`),
//! sincronización con `victory.toml`, auto-detección de la carpeta del juego y
//! generación de variables de entorno calibradas para NVIDIA RTX.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hardware::{HardwareInfo, VramTier};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    Auto,
    Fluid720p,
    Balanced1080p,
    Native1080p,
    High1440p,
    Fsr4K,
    Safe,
    Custom,
}

impl Preset {
    pub fn label_es(self) -> &'static str {
        match self {
            Self::Auto => "Recomendado",
            Self::Fluid720p => "Fluido 720p",
            Self::Balanced1080p => "Equilibrado 1080p",
            Self::Native1080p => "Nítido 1080p",
            Self::High1440p => "Alta 1440p",
            Self::Fsr4K => "4K FSR 3.1",
            Self::Safe => "Modo Seguro",
            Self::Custom => "Personalizado",
        }
    }

    pub fn description_es(self, hw: &HardwareInfo) -> String {
        match self {
            Self::Auto => match hw.vram_tier {
                VramTier::Gb8 => {
                    "Ajuste automático para tarjetas de 8 GB: 720p fluido, consumo de memoria controlado y máximo rendimiento.".to_owned()
                }
                VramTier::Gb12 => {
                    "Ajuste automático para 12 GB: 1080p equilibrado con texturas optimizadas para evitar tirones.".to_owned()
                }
                VramTier::Gb16 => {
                    "Ajuste automático para 16 GB: 1080p rápido con sincronización optimizada para tarjetas RTX 40/50.".to_owned()
                }
                VramTier::Gb24Plus => {
                    "Ajuste automático Ultra (24 GB+): salida 1080p súper fluida con texturas al máximo y brillo de escenas mejorado.".to_owned()
                }
            },
            Self::Fluid720p => {
                "Prioriza FPS máximos (salida 720p). Ideal para laptops, tarjetas de 8 GB o ganar la mayor fluidez posible.".to_owned()
            }
            Self::Balanced1080p => {
                "Salida 1080p con reescalado interno rápido. Excelente equilibrio entre nitidez y FPS en tarjetas NVIDIA.".to_owned()
            }
            Self::Native1080p => {
                "Resolución 1080p completamente nativa (escala 100%). Imagen muy nítida sin filtros externos.".to_owned()
            }
            Self::High1440p => {
                "Salida 2560×1440 (2K) pensada para tarjetas de 16–24 GB VRAM.".to_owned()
            }
            Self::Fsr4K => {
                "Reescalado 4K con AMD FidelityFX FSR 3.1 (requiere DLL de FidelityFX en la carpeta de datos).".to_owned()
            }
            Self::Safe => {
                "Desactiva optimizaciones agresivas. Úsalo solo si experimentas cierres inesperados o fallos gráficos.".to_owned()
            }
            Self::Custom => {
                "Usando tus ajustes personalizados de la pestaña Configuración.".to_owned()
            }
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
    pub fn label_es(self) -> &'static str {
        match self {
            Self::Auto => "Automático (según tu tarjeta)",
            Self::SeriesX => "Xbox Series X (más calidad)",
            Self::SeriesS => "Xbox Series S (menos memoria)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum OutputRes {
    Auto,
    P720,
    P1080,
    P1440,
    P4K,
}

impl OutputRes {
    pub fn label_es(self) -> &'static str {
        match self {
            Self::Auto => "Automática",
            Self::P720 => "1280×720 (HD)",
            Self::P1080 => "1920×1080 (Full HD)",
            Self::P1440 => "2560×1440 (2K)",
            Self::P4K => "3840×2160 (4K)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum FsrQuality {
    Off,
    Quality,
    Balanced,
    Performance,
}

impl FsrQuality {
    pub fn label_es(self) -> &'static str {
        match self {
            Self::Off => "Desactivado (NVIDIA puro)",
            Self::Quality => "Calidad (4K FSR)",
            Self::Balanced => "Equilibrado (4K FSR)",
            Self::Performance => "Rendimiento (4K FSR)",
        }
    }

    pub fn env_value(self) -> Option<&'static str> {
        match self {
            Self::Off => None,
            Self::Quality => Some("quality"),
            Self::Balanced => Some("balanced"),
            Self::Performance => Some("performance"),
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
    pub fn label_es(self) -> &'static str {
        match self {
            Self::Auto => "Automático",
            Self::On => "Activado",
            Self::Off => "Desactivado",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum TextureQuality {
    Auto,
    Max,
    High,
    Medium,
}

impl TextureQuality {
    pub fn label_es(self) -> &'static str {
        match self {
            Self::Auto => "Automática (según VRAM)",
            Self::Max => "Máxima (sin recorte, 24 GB)",
            Self::High => "Alta (12–16 GB)",
            Self::Medium => "Media (recomendada 8 GB)",
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
    /// `0.0` = automático seguro; `0.50..=1.0` = escala interna explícita.
    pub render_scale: f32,
    /// `0` = sin límite (60 por defecto).
    pub fps_cap: u32,
    pub texture_quality: TextureQuality,
    pub fsr_4k: FsrQuality,
    pub kbm_gta: bool,
    pub mouse_sens: f32,
    pub mouse_invert_y: bool,
    pub smooth_motion: bool,
    pub wait_textures: bool,
    pub tex_store: bool,
    pub live_textures: bool,
    pub video_planes: bool,
    pub ambient_light: bool,
    pub ambient_most: f32,
    pub ambient_reflect: f32,
    pub skip_rt_resolve: bool,
    pub skip_raymarched_lights: bool,
    pub port_rt: bool,
    pub port_descriptors: bool,
    pub thin_lists: TriState,
    pub ram_low_profile: TriState,
    pub send_bar: bool,
    pub wc_no_back: bool,
    /// `0` = automático según nivel de VRAM.
    pub windows_bar_mb_override: u32,
    pub use_diag_build: bool,
    pub gpu_timing: bool,
    pub record_pipelines: bool,
    /// Variables extra opcionales en formato `CLAVE=VALOR` (una por línea).
    pub extra_env: String,
}

impl Default for LauncherConfig {
    fn default() -> Self {
        Self {
            game_dir: String::new(),
            preset: Preset::Auto,
            console: ConsoleModel::Auto,
            output: OutputRes::Auto,
            render_scale: 0.0,
            fps_cap: 60,
            texture_quality: TextureQuality::Auto,
            fsr_4k: FsrQuality::Off,
            kbm_gta: false,
            mouse_sens: 1.0,
            mouse_invert_y: false,
            smooth_motion: false,
            wait_textures: false,
            tex_store: false,
            live_textures: true,
            video_planes: true,
            ambient_light: true,
            ambient_most: 3.0,
            ambient_reflect: 1.0,
            skip_rt_resolve: true,
            skip_raymarched_lights: true,
            port_rt: true,
            port_descriptors: false,
            thin_lists: TriState::Auto,
            ram_low_profile: TriState::Auto,
            send_bar: false,
            wc_no_back: false,
            windows_bar_mb_override: 0,
            use_diag_build: false,
            gpu_timing: false,
            record_pipelines: false,
            extra_env: String::new(),
        }
    }
}

impl LauncherConfig {
    pub fn apply_preset(&mut self, preset: Preset) {
        self.preset = preset;
        match preset {
            Preset::Auto => {
                self.console = ConsoleModel::Auto;
                self.output = OutputRes::Auto;
                self.render_scale = 0.0;
                self.texture_quality = TextureQuality::Auto;
                self.fsr_4k = FsrQuality::Off;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.port_descriptors = false;
                self.thin_lists = TriState::Auto;
                self.ram_low_profile = TriState::Auto;
                self.live_textures = true;
                self.video_planes = true;
                self.ambient_light = true;
                self.ambient_most = 3.0;
                self.ambient_reflect = 1.0;
            }
            Preset::Fluid720p => {
                self.console = ConsoleModel::SeriesS;
                self.output = OutputRes::P720;
                self.render_scale = 0.667;
                self.fsr_4k = FsrQuality::Off;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.thin_lists = TriState::Auto;
            }
            Preset::Balanced1080p => {
                self.console = ConsoleModel::Auto;
                self.output = OutputRes::P1080;
                self.render_scale = 0.667;
                self.fsr_4k = FsrQuality::Off;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.thin_lists = TriState::Auto;
            }
            Preset::Native1080p => {
                self.console = ConsoleModel::SeriesX;
                self.output = OutputRes::P1080;
                self.render_scale = 1.0;
                self.fsr_4k = FsrQuality::Off;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.thin_lists = TriState::Auto;
            }
            Preset::High1440p => {
                self.console = ConsoleModel::SeriesX;
                self.output = OutputRes::P1440;
                self.render_scale = 0.667;
                self.fsr_4k = FsrQuality::Off;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.thin_lists = TriState::Auto;
            }
            Preset::Fsr4K => {
                self.console = ConsoleModel::SeriesX;
                self.output = OutputRes::P4K;
                self.render_scale = 0.0;
                self.fsr_4k = FsrQuality::Quality;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.thin_lists = TriState::Auto;
            }
            Preset::Safe => {
                self.console = ConsoleModel::SeriesS;
                self.output = OutputRes::P720;
                self.render_scale = 0.667;
                self.fsr_4k = FsrQuality::Off;
                self.thin_lists = TriState::Off;
                self.port_rt = false;
                self.port_descriptors = false;
                self.send_bar = false;
                self.wc_no_back = false;
            }
            Preset::Custom => {}
        }
    }

    pub fn effective_console(&self, hw: &HardwareInfo) -> &'static str {
        if self.fsr_4k != FsrQuality::Off {
            return "series-x";
        }
        match self.console {
            ConsoleModel::SeriesX => "series-x",
            ConsoleModel::SeriesS => "series-s",
            ConsoleModel::Auto => {
                if hw.vram_tier == VramTier::Gb8 || self.preset == Preset::Safe {
                    "series-s"
                } else {
                    "series-x"
                }
            }
        }
    }

    pub fn effective_output(&self, hw: &HardwareInfo) -> OutputRes {
        if self.fsr_4k != FsrQuality::Off {
            return OutputRes::P4K;
        }
        match self.output {
            OutputRes::Auto => {
                if hw.vram_tier == VramTier::Gb8 {
                    OutputRes::P720
                } else {
                    OutputRes::P1080
                }
            }
            explicit => explicit,
        }
    }

    pub fn effective_windows_bar_mb(&self, hw: &HardwareInfo) -> u32 {
        if self.preset == Preset::Safe {
            0
        } else if self.windows_bar_mb_override > 0 {
            self.windows_bar_mb_override
        } else {
            hw.vram_tier.default_windows_bar_mb()
        }
    }

    pub fn effective_mip_drop(&self, hw: &HardwareInfo) -> u8 {
        match self.texture_quality {
            TextureQuality::Auto => hw.vram_tier.default_mip_drop(),
            TextureQuality::Max => 0,
            TextureQuality::High => 1,
            TextureQuality::Medium => 2,
        }
    }

    pub fn effective_thin(&self, hw: &HardwareInfo) -> bool {
        if self.preset == Preset::Safe {
            return false;
        }
        match self.thin_lists {
            TriState::Auto => !hw.is_blackwell,
            TriState::On => true,
            TriState::Off => false,
        }
    }

    pub fn effective_scale(&self, hw: &HardwareInfo) -> Option<f32> {
        let is_series_s = self.effective_console(hw) == "series-s";
        let out = self.effective_output(hw);
        let max_scale = if is_series_s || out == OutputRes::P4K {
            0.667_f32
        } else {
            1.0_f32
        };
        if self.render_scale > 0.05 {
            Some(self.render_scale.clamp(0.45, max_scale))
        } else if is_series_s {
            Some(0.667)
        } else {
            None
        }
    }

    /// Variables de entorno calibradas para `victory run --execute`.
    pub fn build_run_env(&self, hw: &HardwareInfo) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        let mut set = |k: &str, v: &str| {
            env.insert(k.to_owned(), v.to_owned());
        };

        let safe = self.preset == Preset::Safe;
        let console = self.effective_console(hw);
        let is_series_s = console == "series-s";
        let output = self.effective_output(hw);

        // --- Ruta rápida base (sincronizada con scripts/play-nvidia.ps1) ---
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
        set("VICTORY_VIDEO_PLANES", if self.video_planes { "1" } else { "0" });
        set("VICTORY_LIVE_TEXTURES", if self.live_textures { "1" } else { "0" });
        set("VICTORY_RELAY", "2");
        set("VICTORY_CARD_STEPS", if safe { "0" } else { "2" });
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

        // --- Heap de vistas NVIDIA 1M ---
        set("VICTORY_TABLES_EARLY", "1");
        set("VICTORY_DYNAMIC_SLOTS", "393216");

        // --- Pipelines y audio ---
        set("VICTORY_PIPELINE_LOWEST", "0");
        set("VICTORY_PIPELINE_WORKERS", "8");
        set("VICTORY_PIPELINE_WAIT_ONCE", "1");
        set("VICTORY_TARGET_VARIANTS", "1");
        set("VICTORY_NO_WATCHDOG", "1");
        set("VICTORY_XMA_REFILL", "1");
        if self.record_pipelines {
            set("VICTORY_PIPELINE_BOOK", "1");
        }

        // --- Claves del traductor Wave32 (deben coincidir con shaders/warm) ---
        set("VICTORY_SHADER_OPT", "63");
        set("VICTORY_WAVE_WIDE", "last");
        set("VICTORY_NAN_SCRUB", "1");
        set("VICTORY_SMALL_RECHECK", "0");

        let wide = hw.auto_wide_threads();
        if wide > 0 {
            set("VICTORY_WIDE_THREADS", &wide.to_string());
        }

        if safe {
            set("VICTORY_UNDER_WIDE", "0");
            set("VICTORY_LAND_ECHO", "0");
            set("VICTORY_ASYNC_SUBMIT", "0");
            set("VICTORY_THIN", "0");
        } else {
            set("VICTORY_PORT_CONSTANTS", "1");
            if self.port_descriptors {
                set("VICTORY_PORT_DESCRIPTORS", "1");
            }
            set("VICTORY_PORT_CBV", "1");
            set("VICTORY_CONSTANT_MARKS", "1");
            set("VICTORY_PORT_GUARD", "1");
            set("VICTORY_PORT_JOURNAL", "1");
            if self.port_rt {
                set("VICTORY_PORT_RT", "1");
            }
            set("VICTORY_FEED_ONE_CALL", "1");

            set(
                "VICTORY_CARD_BUFFERS_MIN_KB",
                &hw.vram_tier.default_card_buffers_min_kb().to_string(),
            );
            let card_mb = hw.vram_tier.default_card_buffers_mb();
            if card_mb > 0 {
                set("VICTORY_CARD_BUFFERS_MB", &card_mb.to_string());
            }
            let bar_mb = self.effective_windows_bar_mb(hw);
            if bar_mb > 0 {
                set("VICTORY_WINDOWS_BAR_MB", &bar_mb.to_string());
            }
            set("VICTORY_WINDOW_KB", "4096");
            if self.send_bar {
                set("VICTORY_SEND_BAR", "1");
            }
            if self.wc_no_back {
                set("VICTORY_WC_NO_BACK", "1");
                set("VICTORY_WC_NO_BACK_MIN_KB", "16384");
            }

            set("VICTORY_ASYNC_SUBMIT", "1");
            if self.effective_thin(hw) {
                set("VICTORY_THIN", "1");
                set("VICTORY_THIN_THREADS", "8");
            } else {
                set("VICTORY_THIN", "0");
            }
            set("VICTORY_JOIN_PARTS", "8");
        }

        let mip_drop = self.effective_mip_drop(hw);
        if mip_drop > 0 {
            set("VICTORY_TEXTURE_MIP_DROP", &mip_drop.to_string());
        }

        match self.ram_low_profile {
            TriState::On => set("VICTORY_RAM_16GB", "1"),
            TriState::Off => set("VICTORY_RAM_16GB", "0"),
            TriState::Auto => {}
        }

        if self.tex_store {
            let dir = local_app_data().join("victory").join("cache").join("textures");
            let _ = std::fs::create_dir_all(&dir);
            set("VICTORY_TEXTURE_STORE", &dir.to_string_lossy());
        }

        if self.gpu_timing {
            set("VICTORY_GPU_TIMING", "1");
            set("VICTORY_MEMORY_CATEGORIES", "1");
        }

        if self.kbm_gta {
            set("VICTORY_KBM_GTA", "1");
            if (self.mouse_sens - 1.0).abs() > 0.01 {
                set("VICTORY_MOUSE_SENS", &format!("{:.2}", self.mouse_sens));
            }
            if self.mouse_invert_y {
                set("VICTORY_MOUSE_INVERT_Y", "1");
            }
        }

        if self.fps_cap > 0 {
            let interval_ms = (1000.0_f64 / self.fps_cap as f64).clamp(1.0, 1000.0);
            set("VICTORY_FRAME_INTERVAL_MS", &format!("{interval_ms:.3}"));
        }

        // --- Iluminación ambiental / reflejos (calibrada para escenas con teléfono encendido) ---
        if self.ambient_light {
            set("VICTORY_AMBIENT", "1.0");
            set("VICTORY_AMBIENT_AIR", "1.5");
            set("VICTORY_AMBIENT_MOST", &format!("{:.2}", self.ambient_most.clamp(1.5, 4.5)));
            set("VICTORY_AMBIENT_SHADE", "0.7,0.5");
            set("VICTORY_AMBIENT_REFL", "2.0");
            set("VICTORY_AMBIENT_REFLECT", &format!("{:.2}", self.ambient_reflect.clamp(0.0, 3.0)));
            set("VICTORY_AMBIENT_REFLECT_LEAST", "0.35");
        }

        // --- RT off ---
        set("VICTORY_BVH_NO_HIT", "1");
        if self.skip_rt_resolve {
            set("VICTORY_SKIP_PREFIXES", "CS_RaytracingResolve");
        } else {
            set("VICTORY_SKIP_PREFIXES", "none");
        }
        set(
            "VICTORY_RAYMARCHED_LOCAL_LIGHTS",
            if self.skip_raymarched_lights { "0" } else { "1" },
        );
        set("VICTORY_SKIP_PIPELINES", "none");

        // --- Salida y reescalado ---
        if let Some(fsr) = self.fsr_4k.env_value() {
            set("VICTORY_FSR_4K", fsr);
            let ffx_dir = local_app_data().join("victory").join("fidelityfx");
            if ffx_dir.join("amd_fidelityfx_upscaler_dx12.dll").is_file() {
                set("VICTORY_FIDELITYFX_DIR", &ffx_dir.to_string_lossy());
            }
        } else if is_series_s {
            set("VICTORY_PLANES", "1");
            set("VICTORY_UPSCALER", "6");
            match output {
                OutputRes::P720 => set("VICTORY_OUTPUT", "1280x720"),
                OutputRes::P1080 => set("VICTORY_OUTPUT", "1920x1080"),
                OutputRes::P1440 => set("VICTORY_OUTPUT", "2560x1440"),
                OutputRes::P4K | OutputRes::Auto => {}
            }
        } else {
            set("VICTORY_UPSCALER", "6");
            match output {
                OutputRes::P720 => set("VICTORY_OUTPUT", "1280x720"),
                OutputRes::P1080 | OutputRes::Auto => set("VICTORY_OUTPUT", "1920x1080"),
                OutputRes::P1440 => set("VICTORY_OUTPUT", "2560x1440"),
                OutputRes::P4K => {}
            }
        }

        if let Some(scale) = self.effective_scale(hw) {
            set("VICTORY_RENDER_SCALE", &format!("{scale:.3}"));
        }

        if !self.game_dir.is_empty() {
            set("VICTORY_GAME_DIR", &self.game_dir);
        }

        // Variables personalizadas al final (el usuario experto siempre manda).
        for line in self.extra_env.lines() {
            let trimmed = line.trim();
            if trimmed.is_empty() || trimmed.starts_with('#') {
                continue;
            }
            if let Some((k, v)) = trimmed.split_once('=') {
                let k = k.trim();
                let v = v.trim();
                if !k.is_empty() {
                    env.insert(k.to_owned(), v.to_owned());
                }
            }
        }

        env
    }

    pub fn build_prep_env(&self) -> BTreeMap<String, String> {
        let mut env = BTreeMap::new();
        env.insert("VICTORY_SHADER_OPT".to_owned(), "63".to_owned());
        env.insert("VICTORY_WAVE_WIDE".to_owned(), "last".to_owned());
        env.insert("VICTORY_NAN_SCRUB".to_owned(), "1".to_owned());
        env.insert("VICTORY_PORT_CBV".to_owned(), "1".to_owned());
        env.insert("VICTORY_CONSTANT_MARKS".to_owned(), "1".to_owned());
        env.insert("VICTORY_BVH_NO_HIT".to_owned(), "1".to_owned());
        if !self.game_dir.is_empty() {
            env.insert("VICTORY_GAME_DIR".to_owned(), self.game_dir.clone());
        }
        env
    }
}

pub fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn launcher_config_path() -> PathBuf {
    local_app_data().join("victory").join("launcher.json")
}

pub fn load_config(repo_root: Option<&Path>) -> LauncherConfig {
    let path = launcher_config_path();
    let mut cfg = std::fs::read_to_string(&path)
        .ok()
        .and_then(|raw| serde_json::from_str::<LauncherConfig>(&raw).ok())
        .unwrap_or_default();

    // Si la carpeta guardada está vacía o ya no existe, buscar automáticamente.
    let valid_saved = !cfg.game_dir.is_empty()
        && GameFilesStatus::inspect(Path::new(&cfg.game_dir)).ready();
    if !valid_saved && let Some(found) = auto_find_game_dir(repo_root) {
        cfg.game_dir = found.to_string_lossy().into_owned();
        let _ = save_config(&cfg, repo_root);
    }
    cfg
}

pub fn save_config(cfg: &LauncherConfig, repo_root: Option<&Path>) -> Result<(), String> {
    let path = launcher_config_path();
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| format!("No se pudo crear {}: {e}", parent.display()))?;
    }
    let json = serde_json::to_string_pretty(cfg).map_err(|e| e.to_string())?;
    std::fs::write(&path, json).map_err(|e| format!("No se pudo guardar {}: {e}", path.display()))?;

    if !cfg.game_dir.is_empty() {
        let toml_target = repo_root
            .map(|r| r.join("victory.toml"))
            .or_else(|| {
                std::env::current_exe()
                    .ok()
                    .and_then(|e| e.parent().map(|p| p.join("victory.toml")))
            });
        if let Some(toml_path) = toml_target {
            let ffx_dir = local_app_data().join("victory").join("fidelityfx");
            let mut body = format!(
                "[paths]\ngame_dir = '{}'\n",
                cfg.game_dir.replace('\'', "''")
            );
            if ffx_dir.is_dir() {
                body.push_str(&format!(
                    "fidelityfx_dir = '{}'\n",
                    ffx_dir.to_string_lossy().replace('\'', "''")
                ));
            }
            let _ = std::fs::write(&toml_path, body);
        }
    }
    Ok(())
}

/// Busca automáticamente la carpeta del juego en:
/// 1) `victory.toml` (raíz del repo o junto al exe)
/// 2) `%LOCALAPPDATA%\ScarlettPort\launcher.json` (si probó otro launcher)
/// 3) Carpetas comunes en `Downloads`, `Desktop`, `Games`, `XboxGames` y raíces de disco `C:\..F:\`.
pub fn auto_find_game_dir(repo_root: Option<&Path>) -> Option<PathBuf> {
    // 1. victory.toml en repo o junto al ejecutable.
    let mut toml_candidates = Vec::new();
    if let Some(root) = repo_root {
        toml_candidates.push(root.join("victory.toml"));
    }
    if let Ok(exe) = std::env::current_exe()
        && let Some(dir) = exe.parent()
    {
        toml_candidates.push(dir.join("victory.toml"));
    }
    for toml_path in toml_candidates {
        if let Some(dir) = read_game_dir_from_toml(&toml_path)
            && GameFilesStatus::inspect(&dir).ready()
        {
            return Some(dir);
        }
    }

    // 2. ScarlettPort launcher.json previo.
    let sp_json = local_app_data().join("ScarlettPort").join("launcher.json");
    if let Ok(raw) = std::fs::read_to_string(&sp_json)
        && let Ok(val) = serde_json::from_str::<serde_json::Value>(&raw)
        && let Some(p) = val.get("game_path").and_then(|v| v.as_str())
    {
        let dir = PathBuf::from(p);
        if GameFilesStatus::inspect(&dir).ready() {
            return Some(dir);
        }
    }

    // 3. Escaneo ligero (profundidad máx 2) en ubicaciones habituales del usuario.
    let mut roots: Vec<PathBuf> = Vec::new();
    if let Some(profile) = std::env::var_os("USERPROFILE").map(PathBuf::from) {
        roots.push(profile.join("Downloads"));
        roots.push(profile.join("Desktop"));
        roots.push(profile.join("Games"));
    }
    for drive in ["C:\\", "D:\\", "E:\\", "F:\\"] {
        let p = PathBuf::from(drive);
        if p.is_dir() {
            roots.push(p.join("Games"));
            roots.push(p.join("XboxGames"));
            roots.push(p);
        }
    }

    for root in roots {
        if let Some(found) = scan_dir_for_game(&root, 2) {
            return Some(found);
        }
    }
    None
}

fn scan_dir_for_game(dir: &Path, depth: u8) -> Option<PathBuf> {
    if !dir.is_dir() {
        return None;
    }
    if GameFilesStatus::inspect(dir).ready() {
        return Some(dir.to_path_buf());
    }
    if depth == 0 {
        return None;
    }
    let Ok(entries) = std::fs::read_dir(dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_dir() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if matches!(
            name.as_str(),
            "windows"
                | "program files"
                | "program files (x86)"
                | "programdata"
                | "appdata"
                | "$recycle.bin"
                | "system volume information"
                | "node_modules"
                | ".git"
                | "target"
                | "users"
        ) || name.starts_with('.')
        {
            continue;
        }
        if let Some(found) = scan_dir_for_game(&path, depth - 1) {
            return Some(found);
        }
    }
    None
}

fn read_game_dir_from_toml(path: &Path) -> Option<PathBuf> {
    let text = std::fs::read_to_string(path).ok()?;
    let val: toml::Value = text.parse().ok()?;
    val.get("paths")?
        .get("game_dir")?
        .as_str()
        .map(PathBuf::from)
}

#[derive(Clone, Debug, Default)]
pub struct GameFilesStatus {
    pub dir_exists: bool,
    pub exe_path: Option<PathBuf>,
    pub exe_mb: u64,
    pub shaders_rpf_mb: Option<u64>,
    pub manifest_kb: Option<u64>,
}

impl GameFilesStatus {
    pub fn inspect(dir: &Path) -> Self {
        if !dir.is_dir() {
            return Self::default();
        }
        let mut status = Self {
            dir_exists: true,
            ..Default::default()
        };
        let mut largest_exe: Option<(u64, PathBuf)> = None;
        if let Ok(entries) = std::fs::read_dir(dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if !path.is_file() {
                    continue;
                }
                let Ok(meta) = entry.metadata() else {
                    continue;
                };
                let size = meta.len();
                let name = path
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or_default()
                    .to_ascii_lowercase();
                if name.ends_with(".exe")
                    && !name.starts_with("victory")
                    && !name.starts_with("scarlett")
                    && size >= 20 * 1024 * 1024
                    && largest_exe.as_ref().is_none_or(|(b, _)| size > *b)
                {
                    largest_exe = Some((size, path));
                } else if name == "shaders.rpf" {
                    status.shaders_rpf_mb = Some(size / (1024 * 1024));
                } else if name == "microsoftgame.config" {
                    status.manifest_kb = Some((size / 1024).max(1));
                }
            }
        }
        if let Some((sz, p)) = largest_exe {
            status.exe_mb = sz / (1024 * 1024);
            status.exe_path = Some(p);
        }
        status
    }

    pub fn ready(&self) -> bool {
        self.dir_exists && self.exe_path.is_some() && (self.manifest_kb.is_some() || self.shaders_rpf_mb.is_some())
    }

    pub fn summary_es(&self) -> String {
        if !self.dir_exists {
            return "Aún no seleccionada — pulsa Buscar automáticamente o Elegir carpeta".to_owned();
        }
        if !self.ready() {
            return "Esta carpeta no contiene los archivos principales del juego".to_owned();
        }
        let mut parts = Vec::new();
        if let Some(p) = &self.exe_path
            && let Some(n) = p.file_name().and_then(|s| s.to_str())
        {
            parts.push(format!("{n} ({} MB)", self.exe_mb));
        }
        if let Some(mb) = self.shaders_rpf_mb {
            parts.push(format!("shaders.rpf ({mb} MB)"));
        }
        parts.join(" · ")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn mock_hw(name: &str, vram_mb: u32, ram_gb: f64) -> HardwareInfo {
        HardwareInfo {
            gpu_name: name.to_owned(),
            vram_total_mb: vram_mb,
            vram_used_mb: 512,
            gpu_util_pct: 2,
            vram_tier: VramTier::from_mb(vram_mb),
            is_blackwell: crate::hardware::is_rtx_50_series(name),
            ram_total_gb: ram_gb,
            ram_free_gb: ram_gb * 0.6,
            commit_free_gb: 60.0,
            logical_cpus: 24,
            is_elevated_admin: false,
        }
    }

    #[test]
    fn auto_preset_on_4090_enables_all_tuned_switches() {
        let hw = mock_hw("NVIDIA GeForce RTX 4090", 24564, 64.0);
        let cfg = LauncherConfig::default();
        let env = cfg.build_run_env(&hw);
        assert_eq!(env.get("VICTORY_WINDOWS_BAR_MB").map(String::as_str), Some("8192"));
        assert_eq!(env.get("VICTORY_PORT_RT").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_LIVE_TEXTURES").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_VIDEO_PLANES").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_AMBIENT_MOST").map(String::as_str), Some("3.00"));
        assert_eq!(env.get("VICTORY_AMBIENT_REFLECT").map(String::as_str), Some("1.00"));
        assert_eq!(env.get("VICTORY_RAYMARCHED_LOCAL_LIGHTS").map(String::as_str), Some("0"));
        assert_eq!(env.get("VICTORY_THIN").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_OUTPUT").map(String::as_str), Some("1920x1080"));
    }

    #[test]
    fn blackwell_8gb_uses_720p_and_disables_thin_lists() {
        let hw = mock_hw("NVIDIA GeForce RTX 5050 Laptop GPU", 8192, 24.0);
        let cfg = LauncherConfig::default();
        let env = cfg.build_run_env(&hw);
        assert_eq!(cfg.effective_console(&hw), "series-s");
        assert_eq!(env.get("VICTORY_THIN").map(String::as_str), Some("0"));
        assert_eq!(env.get("VICTORY_OUTPUT").map(String::as_str), Some("1280x720"));
        assert_eq!(env.get("VICTORY_CARD_BUFFERS_MB").map(String::as_str), Some("1280"));
        assert_eq!(env.get("VICTORY_CARD_BUFFERS_MIN_KB").map(String::as_str), Some("1024"));
        assert_eq!(env.get("VICTORY_WINDOWS_BAR_MB").map(String::as_str), Some("1024"));
    }

    #[test]
    fn scale_clamps_safely_on_series_s_and_4k() {
        let hw = mock_hw("NVIDIA GeForce RTX 4090", 24564, 64.0);
        let mut cfg = LauncherConfig::default();
        cfg.output = OutputRes::P4K;
        cfg.render_scale = 1.0;
        assert_eq!(cfg.effective_scale(&hw), Some(0.667));
        cfg.output = OutputRes::P1080;
        assert_eq!(cfg.effective_scale(&hw), Some(1.0));
    }
}
