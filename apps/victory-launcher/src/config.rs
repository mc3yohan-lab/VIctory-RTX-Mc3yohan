//! Configuración persistente del launcher (`%LOCALAPPDATA%\victory\launcher.json`),
//! sincronización con `victory.toml`, auto-detección de la carpeta del juego y
//! generación de variables de entorno calibradas para NVIDIA RTX.

use std::collections::BTreeMap;
use std::io::{self, Write};
use std::ops::{Deref, DerefMut};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

use serde::{Deserialize, Serialize};
use toml_edit::{DocumentMut, Item, Table, value};

use crate::hardware::{HardwareInfo, VramTier};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub enum Preset {
    Auto,
    Fluid720p,
    Balanced1080p,
    QualityRtx,
    Native1080p,
    High1440p,
    Fsr4K,
    Safe,
    Custom,
}

impl Preset {
    pub fn quality_for(vram: VramTier) -> Self {
        match vram {
            VramTier::Gb24Plus => Self::Native1080p,
            _ => Self::QualityRtx,
        }
    }

    pub fn label_es(self) -> &'static str {
        match self {
            Self::Auto => "Recomendado",
            Self::Fluid720p => "Fluido 720p",
            Self::Balanced1080p => "Equilibrado 1080p",
            Self::QualityRtx => "Calidad RTX",
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
                    "Salida de 1080p y texturas con todos sus niveles de detalle para tarjetas con al menos 24 GB de VRAM.".to_owned()
                }
            },
            Self::Fluid720p => {
                "Prioriza FPS máximos (salida 720p). Ideal para laptops, tarjetas de 8 GB o ganar la mayor fluidez posible.".to_owned()
            }
            Self::Balanced1080p => {
                "Salida 1080p con reescalado interno rápido. Excelente equilibrio entre nitidez y FPS en tarjetas NVIDIA.".to_owned()
            }
            Self::QualityRtx => {
                "Series X con salida de 1080p, escala interna del 67 % y espera de texturas.".to_owned()
            }
            Self::Native1080p => {
                "Series X a 1920 × 1080 con escala del 100 % y espera de texturas. FSR y la preparación para Smooth Motion quedan desactivados.".to_owned()
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

const CURRENT_CONFIG_VERSION: u32 = 2;

#[derive(Clone, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherConfig {
    #[serde(default)]
    pub config_version: u32,
    pub wizard_completed: bool,
    pub game_dir: String,
    #[serde(flatten)]
    pub settings: LauncherSettings,
    profiles: BTreeMap<String, BTreeMap<String, LauncherSettings>>,
    active_profile: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct LauncherSettings {
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
            config_version: CURRENT_CONFIG_VERSION,
            wizard_completed: false,
            game_dir: String::new(),
            settings: LauncherSettings::default(),
            profiles: BTreeMap::new(),
            active_profile: None,
        }
    }
}

impl Deref for LauncherConfig {
    type Target = LauncherSettings;

    fn deref(&self) -> &Self::Target {
        &self.settings
    }
}

impl DerefMut for LauncherConfig {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.settings
    }
}

impl Default for LauncherSettings {
    fn default() -> Self {
        Self {
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
            ambient_most: 4.0,
            ambient_reflect: 1.2,
            skip_rt_resolve: true,
            skip_raymarched_lights: true,
            port_rt: true,
            port_descriptors: false,
            thin_lists: TriState::Auto,
            ram_low_profile: TriState::Auto,
            send_bar: true,
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
    fn migrate(&mut self) -> bool {
        if self.config_version >= CURRENT_CONFIG_VERSION {
            return false;
        }
        if (self.ambient_most - 2.0).abs() < 0.01 && (self.ambient_reflect - 0.6).abs() < 0.01 {
            self.ambient_most = 4.0;
            self.ambient_reflect = 1.2;
        }
        self.config_version = CURRENT_CONFIG_VERSION;
        true
    }

    pub fn reset_settings(&mut self) {
        self.settings = LauncherSettings::default();
        self.active_profile = None;
    }

    pub fn set_game_dir(&mut self, dir: String) {
        if profile_game_key(&self.game_dir) != profile_game_key(&dir) {
            self.active_profile = None;
        }
        self.game_dir = dir;
    }

    pub fn profile_names(&self) -> impl Iterator<Item = &str> {
        self.profiles
            .get(&profile_game_key(&self.game_dir))
            .into_iter()
            .flat_map(|profiles| profiles.keys().map(String::as_str))
    }

    pub fn matching_profile(&self) -> Option<&str> {
        let name = self.active_profile.as_deref()?;
        let settings = self.profiles.get(&profile_game_key(&self.game_dir))?.get(name)?;
        (settings == &self.settings).then_some(name)
    }

    pub fn create_profile(&mut self, name: &str) -> Result<String, String> {
        let name = valid_profile_name(name)?;
        let key = profile_game_key(&self.game_dir);
        if key.is_empty() {
            return Err("Selecciona la carpeta del juego antes de guardar un perfil.".into());
        }
        let profiles = self.profiles.entry(key).or_default();
        check_profile_name(profiles, name, None)?;
        profiles.insert(name.into(), self.settings.clone());
        self.active_profile = Some(name.into());
        Ok(name.into())
    }

    pub fn load_profile(&mut self, name: &str) -> Result<(), String> {
        let settings = self
            .profiles
            .get(&profile_game_key(&self.game_dir))
            .and_then(|profiles| profiles.get(name))
            .ok_or_else(missing_profile)?;
        self.settings = settings.clone();
        self.active_profile = Some(name.into());
        Ok(())
    }

    pub fn update_profile(&mut self, name: &str) -> Result<(), String> {
        let settings = self
            .profiles
            .get_mut(&profile_game_key(&self.game_dir))
            .and_then(|profiles| profiles.get_mut(name))
            .ok_or_else(missing_profile)?;
        *settings = self.settings.clone();
        self.active_profile = Some(name.into());
        Ok(())
    }

    pub fn rename_profile(&mut self, from: &str, to: &str) -> Result<String, String> {
        let to = valid_profile_name(to)?;
        let profiles = self
            .profiles
            .get_mut(&profile_game_key(&self.game_dir))
            .ok_or_else(missing_profile)?;
        check_profile_name(profiles, to, Some(from))?;
        let settings = profiles.remove(from).ok_or_else(missing_profile)?;
        profiles.insert(to.into(), settings);
        if self.active_profile.as_deref() == Some(from) {
            self.active_profile = Some(to.into());
        }
        Ok(to.into())
    }

    pub fn delete_profile(&mut self, name: &str) -> Result<(), String> {
        let key = profile_game_key(&self.game_dir);
        let profiles = self.profiles.get_mut(&key).ok_or_else(missing_profile)?;
        profiles.remove(name).ok_or_else(missing_profile)?;
        if profiles.is_empty() {
            self.profiles.remove(&key);
        }
        if self.active_profile.as_deref() == Some(name) {
            self.active_profile = None;
        }
        Ok(())
    }

    pub fn apply_preset(&mut self, preset: Preset) {
        self.active_profile = None;
        self.preset = preset;
        if !matches!(preset, Preset::Safe | Preset::Custom) {
            self.ambient_light = true;
            self.ambient_most = 4.0;
            self.ambient_reflect = 1.2;
        }
        match preset {
            Preset::Auto => {
                self.console = ConsoleModel::Auto;
                self.output = OutputRes::Auto;
                self.render_scale = 0.0;
                self.texture_quality = TextureQuality::Auto;
                self.wait_textures = false;
                self.fsr_4k = FsrQuality::Off;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.port_descriptors = false;
                self.thin_lists = TriState::Auto;
                self.ram_low_profile = TriState::Auto;
                self.live_textures = true;
                self.video_planes = true;
                self.send_bar = true;
            }
            Preset::Fluid720p => {
                self.console = ConsoleModel::Auto;
                self.output = OutputRes::P720;
                self.render_scale = 0.0;
                self.fsr_4k = FsrQuality::Off;
                self.wait_textures = false;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.thin_lists = TriState::Auto;
                self.send_bar = true;
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
                self.send_bar = true;
            }
            Preset::QualityRtx | Preset::Native1080p => {
                self.console = ConsoleModel::SeriesX;
                self.output = OutputRes::P1080;
                self.render_scale = if preset == Preset::Native1080p { 1.0 } else { 0.667 };
                if preset == Preset::Native1080p {
                    self.smooth_motion = false;
                }
                self.texture_quality = TextureQuality::Auto;
                self.fsr_4k = FsrQuality::Off;
                self.wait_textures = true;
                self.live_textures = true;
                self.video_planes = true;
                self.skip_rt_resolve = true;
                self.skip_raymarched_lights = true;
                self.port_rt = true;
                self.port_descriptors = false;
                self.thin_lists = TriState::Auto;
                self.ram_low_profile = TriState::Auto;
                self.send_bar = true;
                self.wc_no_back = false;
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
                self.send_bar = true;
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
                self.send_bar = true;
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
        } else if self.preset == Preset::Fluid720p && self.render_scale <= 0.05 {
            Some(1.0)
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
        set("VICTORY_JOIN_PARTS", "8");
        set("VICTORY_INDIRECT_WINDOW", "1");
        set("VICTORY_RELAY_COPY", "1");
        set("VICTORY_INDIRECT_BATCH", "2");
        set("VICTORY_INDIRECT_REST", "0");
        set("VICTORY_LAND_SKIP", "1");
        set("VICTORY_LAND_THREADS", if hw.logical_cpus >= 16 { "8" } else { "6" });
        set("VICTORY_MAP_THREADS", "1");
        set("VICTORY_TITLE_PRIORITIES", "0");
        set("VICTORY_CARD_AWAKE", "idle");
        set("VICTORY_D3D12_DRED", "light");
        set("VICTORY_D3D12_BACKGROUND", "0");
        set("VICTORY_MEMORY_MAP", "0");
        set("VICTORY_GUARD_HOT", "0");
        set("VICTORY_WRITE_JOBS", "1");
        set("VICTORY_WATCH_TEXTURES_JOINED", "1");
        set("VICTORY_READBACK_POOL", "1");
        set("VICTORY_BATCHED_SWEEP", "1");
        set("VICTORY_PIPELINE_SAVE_ON_EXIT_ONLY", "1");

        // --- Heap de vistas NVIDIA 1M ---
        set("VICTORY_TABLES_EARLY", "1");

        // --- Pipelines y audio ---
        set("VICTORY_PIPELINE_LOWEST", "0");
        set("VICTORY_PIPELINE_WORKERS", "8");
        set("VICTORY_PIPELINE_WAIT_ONCE", "1");
        set("VICTORY_PIPELINE_WAIT_AGAIN_MS", "10");
        set("VICTORY_TARGET_VARIANTS", "1");
        set("VICTORY_NO_WATCHDOG", "1");
        set("VICTORY_XMA_REFILL", "1");
        set("VICTORY_FILE_CACHE", "1");
        set("VICTORY_MAP_SAME", "1");
        set("VICTORY_HOLES_ONCE", "30");
        if self.record_pipelines {
            set("VICTORY_PIPELINE_BOOK", "1");
        }

        // --- Claves del traductor Wave32 (deben coincidir con shaders/warm) ---
        set("VICTORY_SHADER_OPT", "191");
        set("VICTORY_SHADER_FAST_BITS", "24");
        set("VICTORY_WAVE_WIDE", "last");
        set("VICTORY_NAN_SCRUB", "1");

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
            // Agrupar clamps indirectos en lote (-87% barreras UAV por cuadro en RTX 4090).
            set("VICTORY_PORT_CONSTANTS_ORDER_ALONE", "0");
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
            if self.send_bar && hw.vram_tier != VramTier::Gb8 {
                set("VICTORY_SEND_BAR", "1");
            }
            if self.wc_no_back {
                set("VICTORY_WC_NO_BACK", "1");
                set("VICTORY_WC_NO_BACK_MIN_KB", "16384");
            }

            set("VICTORY_ASYNC_SUBMIT", "1");
            if hw.logical_cpus >= 12 {
                set("VICTORY_LANE_CORE", "1");
            }
            if self.effective_thin(hw) {
                set("VICTORY_THIN", "1");
                set("VICTORY_THIN_THREADS", "8");
            } else {
                set("VICTORY_THIN", "0");
            }
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

        // --- Iluminación ambiental / reflejos (calibrada para interiores oscuros y exteriores nítidos) ---
        if self.ambient_light {
            set("VICTORY_AMBIENT", "1.0");
            set("VICTORY_AMBIENT_AIR", "1.5");
            set("VICTORY_AMBIENT_MOST", &format!("{:.2}", self.ambient_most.clamp(1.5, 5.0)));
            set("VICTORY_AMBIENT_CLEAN", "1");
            set("VICTORY_AMBIENT_EXPOSURE_FLOOR", "0.0045");
            set("VICTORY_AMBIENT_SHADE", "0.7,0.5");
            set("VICTORY_AMBIENT_REFL", "2.0");
            set("VICTORY_AMBIENT_REFLECT", &format!("{:.2}", self.ambient_reflect.clamp(0.0, 3.0)));
            set("VICTORY_AMBIENT_REFLECT_LEAST", "0.20");
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
        } else if is_series_s {
            set("VICTORY_PLANES", "1");
            set("VICTORY_UPSCALER", "6");
            match output {
                OutputRes::P720 => set("VICTORY_OUTPUT", "1280x720"),
                OutputRes::P1080 => set("VICTORY_OUTPUT", "1920x1080"),
                OutputRes::P1440 => set("VICTORY_OUTPUT", "2560x1440"),
                OutputRes::P4K => set("VICTORY_OUTPUT", "3840x2160"),
                OutputRes::Auto => {}
            }
        } else {
            set("VICTORY_UPSCALER", "6");
            match output {
                OutputRes::P720 => set("VICTORY_OUTPUT", "1280x720"),
                OutputRes::P1080 | OutputRes::Auto => set("VICTORY_OUTPUT", "1920x1080"),
                OutputRes::P1440 => set("VICTORY_OUTPUT", "2560x1440"),
                OutputRes::P4K => set("VICTORY_OUTPUT", "3840x2160"),
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

        // The title's output size and the DXGI display mode are independent.
        // Match them for native 1080p after applying custom environment overrides.
        let setting = |name: &str| {
            env.iter()
                .rev()
                .find_map(|(key, value)| key.eq_ignore_ascii_case(name).then_some(value.as_str()))
        };
        let native_1080p = console == "series-x"
            && setting("VICTORY_OUTPUT")
                .and_then(|value| value.split_once(['x', 'X']))
                .is_some_and(|(width, height)| width.trim() == "1920" && height.trim() == "1080")
            && setting("VICTORY_RENDER_SCALE").and_then(|value| value.parse::<f32>().ok())
                == Some(1.0)
            && setting("VICTORY_UPSCALER").and_then(|value| value.parse::<u32>().ok()) == Some(6)
            && matches!(setting("VICTORY_FSR_4K"), None | Some("0"));
        if native_1080p && setting("VICTORY_DISPLAY_MODE").is_none() {
            env.insert("VICTORY_DISPLAY_MODE".into(), "1920x1080".into());
        }

        env
    }

}

fn profile_game_key(dir: &str) -> String {
    dir.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

fn valid_profile_name(name: &str) -> Result<&str, String> {
    let name = name.trim();
    if name.is_empty() || name.chars().count() > 48 || name.chars().any(char::is_control) {
        return Err("Escribe un nombre de perfil de 1 a 48 caracteres.".into());
    }
    Ok(name)
}

fn check_profile_name(
    profiles: &BTreeMap<String, LauncherSettings>,
    name: &str,
    except: Option<&str>,
) -> Result<(), String> {
    let key = name.to_lowercase();
    if profiles.keys().any(|existing| {
        Some(existing.as_str()) != except && existing.to_lowercase() == key
    }) {
        return Err("Ya hay un perfil con ese nombre para este juego.".into());
    }
    Ok(())
}

fn missing_profile() -> String {
    "Ese perfil ya no está disponible para este juego.".into()
}

pub fn local_app_data() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
}

pub fn launcher_config_path() -> PathBuf {
    local_app_data().join("victory").join("launcher.json")
}

pub fn load_config(repo_root: Option<&Path>) -> (LauncherConfig, Option<String>) {
    let path = launcher_config_path();
    let (mut cfg, load_error) = match read_launcher_config(&path) {
        Ok(config) => (config.unwrap_or_default(), None),
        Err(error) => (LauncherConfig::default(), Some(error)),
    };
    let mut changed = cfg.migrate();

    // Si la carpeta guardada está vacía o ya no existe, buscar automáticamente.
    let valid_saved = !cfg.game_dir.is_empty()
        && GameFilesStatus::inspect(Path::new(&cfg.game_dir)).ready();
    if !valid_saved && let Some(found) = auto_find_game_dir(repo_root) {
        cfg.set_game_dir(found.to_string_lossy().into_owned());
        changed = true;
    }
    let warning = load_error.or_else(|| {
        if changed { save_config(&cfg, repo_root).err() } else { None }
    });
    (cfg, warning)
}

pub fn save_config(cfg: &LauncherConfig, repo_root: Option<&Path>) -> Result<(), String> {
    let path = launcher_config_path();
    // VICTORY_CONFIG is an explicit operator choice. The launcher passes
    // VICTORY_GAME_DIR to its child, so it need not edit that separate file.
    let runtime = if explicit_runtime_config().is_none() {
        default_runtime_config(repo_root)
    } else {
        None
    };
    save_config_to(cfg, &path, runtime.as_deref())
}

fn save_config_to(
    cfg: &LauncherConfig,
    launcher_path: &Path,
    runtime_path: Option<&Path>,
) -> Result<(), String> {
    let previous = read_launcher_config(launcher_path)?;
    let json = serde_json::to_vec_pretty(cfg).map_err(|e| e.to_string())?;
    if !cfg.game_dir.is_empty()
        && previous.as_ref().is_none_or(|saved| saved.game_dir != cfg.game_dir)
        && let Some(runtime_path) = runtime_path
    {
        let on_disk = read_optional(runtime_path)?;
        if let Some(updated) = updated_runtime_config(runtime_path, on_disk.as_deref(), &cfg.game_dir)? {
            write_atomically(runtime_path, updated.as_bytes())?;
            if let Err(error) = write_atomically(launcher_path, &json) {
                let current = read_optional(runtime_path).map_err(|rollback| {
                    format!("{error} No se pudo confirmar el estado del archivo de rutas ({rollback}).")
                })?;
                if current.as_deref() == Some(updated.as_str()) {
                    let restored = if let Some(original) = on_disk {
                        write_atomically(runtime_path, original.as_bytes())
                    } else {
                        std::fs::remove_file(runtime_path).map_err(|e| e.to_string())
                    };
                    restored.map_err(|rollback| {
                        format!("{error} El archivo de rutas también quedó actualizado ({rollback}).")
                    })?;
                }
                return Err(error);
            }
            return Ok(());
        }
    }
    write_atomically(launcher_path, &json)
}

fn read_launcher_config(path: &Path) -> Result<Option<LauncherConfig>, String> {
    read_optional(path)?.map(|raw| {
        serde_json::from_str(&raw)
            .map_err(|e| format!("Corrige el archivo {} antes de guardar ajustes ({e}).", path.display()))
    }).transpose()
}

fn read_optional(path: &Path) -> Result<Option<String>, String> {
    match std::fs::read_to_string(path) {
        Ok(text) => Ok(Some(text)),
        Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("No se pudo leer {} ({e}).", path.display())),
    }
}

fn updated_runtime_config(path: &Path, original: Option<&str>, game_dir: &str) -> Result<Option<String>, String> {
    let mut doc = original.unwrap_or_default().parse::<DocumentMut>()
        .map_err(|e| format!("Corrige el archivo {} antes de guardar la carpeta ({e}).", path.display()))?;
    if !doc.contains_key("paths") {
        doc["paths"] = Item::Table(Table::new());
    }
    let paths = doc["paths"].as_table_like_mut()
        .ok_or_else(|| format!("La sección paths de {} debe ser una tabla.", path.display()))?;
    if paths.get("game_dir").and_then(Item::as_str) == Some(game_dir) {
        return Ok(None);
    }
    if let Some(existing) = paths.get_mut("game_dir").and_then(Item::as_value_mut) {
        let decor = existing.decor().clone();
        *existing = game_dir.into();
        *existing.decor_mut() = decor;
    } else {
        paths.insert("game_dir", value(game_dir));
    }
    Ok(Some(doc.to_string()))
}

fn write_atomically(path: &Path, bytes: &[u8]) -> Result<(), String> {
    static NEXT: AtomicU64 = AtomicU64::new(0);
    let write = || -> io::Result<()> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let mut name = path.file_name().unwrap_or_default().to_os_string();
        let nonce = NEXT.fetch_add(1, Ordering::Relaxed);
        name.push(format!(".tmp-{}-{nonce}", std::process::id()));
        let tmp = path.with_file_name(name);
        let mut file = std::fs::File::create_new(&tmp)?;
        let written = file.write_all(bytes).and_then(|()| file.sync_all());
        drop(file);
        let result = written.and_then(|()| std::fs::rename(&tmp, path));
        if result.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        result
    };
    write().map_err(|e| format!("No se pudo guardar {} ({e}).", path.display()))
}

fn explicit_runtime_config() -> Option<PathBuf> {
    std::env::var_os("VICTORY_CONFIG")
        .filter(|path| !path.is_empty())
        .map(PathBuf::from)
}

fn resolve_from_root(path: PathBuf, root: Option<&Path>) -> PathBuf {
    if path.is_relative() && let Some(root) = root {
        return root.join(path);
    }
    path
}

fn default_runtime_config(repo_root: Option<&Path>) -> Option<PathBuf> {
    repo_root
        .map(|root| root.join("victory.toml"))
        .or_else(|| std::env::current_exe().ok()?.parent().map(|dir| dir.join("victory.toml")))
}

pub fn fidelityfx_dir(repo_root: Option<&Path>) -> Option<PathBuf> {
    let configured = explicit_runtime_config()
        .map(|path| resolve_from_root(path, repo_root))
        .or_else(|| default_runtime_config(repo_root));
    std::env::var_os("VICTORY_FIDELITYFX_DIR")
        .filter(|path| !path.is_empty())
        .map(|dir| resolve_from_root(PathBuf::from(dir), repo_root))
        .or_else(|| {
            let doc = std::fs::read_to_string(configured?).ok()?.parse::<toml::Table>().ok()?;
            let dir = PathBuf::from(doc.get("paths")?.get("fidelityfx_dir")?.as_str()?);
            Some(resolve_from_root(dir, repo_root))
        })
        .or_else(|| {
            let dir = local_app_data().join("victory").join("fidelityfx");
            dir.join("amd_fidelityfx_upscaler_dx12.dll").is_file().then_some(dir)
        })
}

/// Busca automáticamente la carpeta del juego en:
/// 1) `victory.toml` (raíz del repo o junto al exe)
/// 2) `%LOCALAPPDATA%\ScarlettPort\launcher.json` (si probó otro launcher)
/// 3) Carpetas comunes en `Downloads`, `Desktop`, `Games`, `XboxGames` y raíces de disco `C:\..F:\`.
pub fn auto_find_game_dir(repo_root: Option<&Path>) -> Option<PathBuf> {
    // 1. Variables del runtime y victory.toml en repo o junto al ejecutable.
    if let Some(raw) = std::env::var_os("VICTORY_GAME_DIR").filter(|path| !path.is_empty()) {
        let dir = resolve_from_root(PathBuf::from(raw), repo_root);
        if GameFilesStatus::inspect(&dir).ready() {
            return Some(dir);
        }
    }
    let mut toml_candidates = Vec::new();
    if let Some(explicit) = explicit_runtime_config() {
        toml_candidates.push(resolve_from_root(explicit, repo_root));
    } else {
        if let Some(root) = repo_root {
            toml_candidates.push(root.join("victory.toml"));
        }
        if let Some(exe_config) = default_runtime_config(None) {
            toml_candidates.push(exe_config);
        }
    }
    for toml_path in toml_candidates {
        if let Some(dir) = read_game_dir_from_toml(&toml_path)
            .map(|dir| resolve_from_root(dir, repo_root.or_else(|| toml_path.parent())))
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
            probe_done: true,
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
        assert_eq!(env.get("VICTORY_AMBIENT_MOST").map(String::as_str), Some("4.00"));
        assert_eq!(env.get("VICTORY_AMBIENT_REFLECT").map(String::as_str), Some("1.20"));
        assert_eq!(env.get("VICTORY_AMBIENT_EXPOSURE_FLOOR").map(String::as_str), Some("0.0045"));
        assert_eq!(env.get("VICTORY_RAYMARCHED_LOCAL_LIGHTS").map(String::as_str), Some("0"));
        assert_eq!(env.get("VICTORY_PORT_CONSTANTS_ORDER_ALONE").map(String::as_str), Some("0"));
        assert_eq!(env.get("VICTORY_SEND_BAR").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_LANE_CORE").map(String::as_str), Some("1"));
        assert_eq!(env.get("VICTORY_GUARD_HOT").map(String::as_str), Some("0"));
        assert_eq!(env.get("VICTORY_SHADER_OPT").map(String::as_str), Some("191"));
        assert_eq!(env.get("VICTORY_SMALL_RECHECK"), None);
        assert!(!env.contains_key("VICTORY_DYNAMIC_SLOTS"));
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
        assert_eq!(env.get("VICTORY_SEND_BAR"), None);
        assert_eq!(env.get("VICTORY_OUTPUT").map(String::as_str), Some("1280x720"));
        assert_eq!(env.get("VICTORY_CARD_BUFFERS_MB").map(String::as_str), Some("1280"));
        assert_eq!(env.get("VICTORY_CARD_BUFFERS_MIN_KB").map(String::as_str), Some("1024"));
        assert_eq!(env.get("VICTORY_WINDOWS_BAR_MB").map(String::as_str), Some("1024"));
    }

    #[test]
    fn legacy_json_migrates_only_the_old_ambient_defaults() {
        let mut cfg: LauncherConfig =
            serde_json::from_str(r#"{"ambient_most":2.0,"ambient_reflect":0.6}"#).unwrap();
        assert_eq!(cfg.config_version, 0);
        assert!(cfg.migrate());
        assert_eq!(cfg.config_version, 2);
        assert_eq!((cfg.ambient_most, cfg.ambient_reflect), (4.0, 1.2));
        assert!(!cfg.migrate());

        let mut custom: LauncherConfig =
            serde_json::from_str(r#"{"ambient_most":2.0,"ambient_reflect":0.8}"#).unwrap();
        assert!(custom.migrate());
        assert_eq!((custom.ambient_most, custom.ambient_reflect), (2.0, 0.8));
    }

    #[test]
    fn fluid_720p_is_native_on_series_x_with_24gb() {
        let hw = mock_hw("NVIDIA GeForce RTX 4090", 24564, 64.0);
        let mut cfg = LauncherConfig::default();
        cfg.apply_preset(Preset::Fluid720p);
        assert_eq!(cfg.effective_console(&hw), "series-x");
        assert_eq!(cfg.effective_output(&hw), OutputRes::P720);
        assert_eq!(cfg.effective_scale(&hw), Some(1.0));
        assert_eq!(
            cfg.build_run_env(&hw).get("VICTORY_RENDER_SCALE").map(String::as_str),
            Some("1.000")
        );

        let low_vram = mock_hw("NVIDIA GeForce RTX 4060", 8192, 24.0);
        assert_eq!(cfg.effective_scale(&low_vram), Some(0.667));
    }

    #[test]
    fn selecting_native_quality_replaces_previous_graphics_settings_and_preserves_saved_profiles() {
        let hw = mock_hw("NVIDIA GeForce RTX 4090", 24564, 64.0);
        let mut cfg = LauncherConfig::default();
        cfg.set_game_dir("D:/Game".into());
        cfg.apply_preset(Preset::Fluid720p);
        cfg.smooth_motion = true;
        cfg.texture_quality = TextureQuality::Medium;
        cfg.kbm_gta = true;
        let previous = cfg.settings.clone();
        cfg.create_profile("Anterior").unwrap();

        let preset = Preset::quality_for(hw.vram_tier);
        assert_eq!(preset, Preset::Native1080p);
        cfg.apply_preset(preset);
        let env = cfg.build_run_env(&hw);
        assert_eq!(cfg.effective_console(&hw), "series-x");
        assert_eq!(env["VICTORY_OUTPUT"], "1920x1080");
        assert_eq!(env["VICTORY_DISPLAY_MODE"], "1920x1080");
        assert_eq!(env["VICTORY_RENDER_SCALE"], "1.000");
        assert_eq!(env["VICTORY_TEXTURE_NO_WAIT"], "0");
        assert_eq!(env["VICTORY_EXACT_NO_WAIT"], "0");
        assert_eq!(cfg.effective_mip_drop(&hw), 0);
        assert_eq!(cfg.fsr_4k, FsrQuality::Off);
        assert!(!cfg.smooth_motion);
        assert!(cfg.kbm_gta);
        cfg.load_profile("Anterior").unwrap();
        assert_eq!(cfg.settings, previous);
        for tier in [VramTier::Gb8, VramTier::Gb12, VramTier::Gb16] {
            assert_eq!(Preset::quality_for(tier), Preset::QualityRtx);
        }
    }

    #[test]
    fn native_series_x_reports_1080p_display_and_respects_overrides() {
        let hw = mock_hw("NVIDIA GeForce RTX 4090", 24564, 64.0);
        let mut cfg = LauncherConfig::default();
        cfg.apply_preset(Preset::Native1080p);
        assert_eq!(cfg.build_run_env(&hw)["VICTORY_DISPLAY_MODE"], "1920x1080");

        for extra in [
            "VICTORY_OUTPUT=1280x720",
            "VICTORY_RENDER_SCALE=0.667",
            "VICTORY_FSR_4K=quality",
            "VICTORY_UPSCALER=7",
        ] {
            cfg.extra_env = extra.into();
            assert!(
                !cfg.build_run_env(&hw).contains_key("VICTORY_DISPLAY_MODE"),
                "{extra}"
            );
        }

        cfg.extra_env = "victory_display_mode=3840x2160".into();
        let env = cfg.build_run_env(&hw);
        assert!(!env.contains_key("VICTORY_DISPLAY_MODE"));
        assert_eq!(env["victory_display_mode"], "3840x2160");

        cfg.extra_env.clear();
        cfg.apply_preset(Preset::Fluid720p);
        assert!(!cfg.build_run_env(&hw).contains_key("VICTORY_DISPLAY_MODE"));
        cfg.apply_preset(Preset::Native1080p);
        cfg.console = ConsoleModel::SeriesS;
        assert!(!cfg.build_run_env(&hw).contains_key("VICTORY_DISPLAY_MODE"));
    }

    #[test]
    fn scale_clamps_safely_on_series_s_and_4k() {
        let hw = mock_hw("NVIDIA GeForce RTX 4090", 24564, 64.0);
        let mut cfg = LauncherConfig {
            settings: LauncherSettings {
                output: OutputRes::P4K,
                render_scale: 1.0,
                ..LauncherSettings::default()
            },
            ..LauncherConfig::default()
        };
        assert_eq!(cfg.effective_scale(&hw), Some(0.667));
        assert_eq!(cfg.build_run_env(&hw)["VICTORY_OUTPUT"], "3840x2160");
        cfg.output = OutputRes::P1080;
        assert_eq!(cfg.effective_scale(&hw), Some(1.0));
    }

    #[test]
    fn quality_rtx_preserves_series_x_and_enables_texture_waits() {
        let hw = mock_hw("NVIDIA GeForce RTX 4090", 24564, 64.0);
        let mut cfg = LauncherConfig::default();
        cfg.apply_preset(Preset::QualityRtx);
        let env = cfg.build_run_env(&hw);
        assert_eq!(cfg.effective_console(&hw), "series-x");
        assert_eq!(cfg.effective_output(&hw), OutputRes::P1080);
        assert_eq!(cfg.effective_scale(&hw), Some(0.667));
        assert_eq!(cfg.effective_mip_drop(&hw), 0);
        assert_eq!(env["VICTORY_TEXTURE_NO_WAIT"], "0");
        assert_eq!(env["VICTORY_EXACT_NO_WAIT"], "0");
        assert!(!env.contains_key("VICTORY_DYNAMIC_SLOTS"));
        cfg.apply_preset(Preset::Fluid720p);
        assert_eq!(cfg.build_run_env(&hw)["VICTORY_TEXTURE_NO_WAIT"], "1");
    }

    #[test]
    fn profiles_persist_independently_for_each_game() {
        let legacy = r#"{"config_version":2,"wizard_completed":true,"game_dir":"D:/Game A/","fps_cap":45}"#;
        let mut cfg: LauncherConfig = serde_json::from_str(legacy).unwrap();
        assert_eq!(cfg.fps_cap, 45);
        let first = cfg.create_profile(" Calidad ").unwrap();
        assert_eq!(first, "Calidad");
        assert!(cfg.create_profile("calidad").is_err());
        assert!(cfg.create_profile("  ").is_err());
        cfg.set_game_dir("D:/Game B".into());
        cfg.fps_cap = 25;
        assert_eq!(cfg.profile_names().count(), 0);
        cfg.create_profile("Calidad").unwrap();

        let raw = serde_json::to_value(&cfg).unwrap();
        assert_eq!(raw["fps_cap"], 25);
        let mut cfg: LauncherConfig = serde_json::from_value(raw).unwrap();
        assert_eq!(cfg.matching_profile(), Some("Calidad"));
        cfg.set_game_dir("d:\\GAME A".into());
        assert_eq!(cfg.matching_profile(), None);
        cfg.load_profile(&first).unwrap();
        assert_eq!(cfg.fps_cap, 45);
        assert!(cfg.wizard_completed);
        assert_eq!(cfg.matching_profile(), Some("Calidad"));
        cfg.create_profile("Mismos ajustes").unwrap();
        assert_eq!(cfg.matching_profile(), Some("Mismos ajustes"));
        cfg.delete_profile("Mismos ajustes").unwrap();
        cfg.reset_settings();
        assert_eq!(cfg.profile_names().collect::<Vec<_>>(), ["Calidad"]);

        cfg.fps_cap = 30;
        cfg.update_profile(&first).unwrap();
        let renamed = cfg.rename_profile(&first, "Mi perfil RTX").unwrap();
        cfg.fps_cap = 60;
        cfg.load_profile(&renamed).unwrap();
        assert_eq!(cfg.fps_cap, 30);
        cfg.delete_profile(&renamed).unwrap();
        assert!(cfg.load_profile(&renamed).is_err());
        cfg.set_game_dir("D:/Game B".into());
        cfg.load_profile("Calidad").unwrap();
        assert_eq!(cfg.fps_cap, 25);
    }

    #[test]
    fn synchronizing_runtime_config_preserves_comments_and_custom_values() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = dir.path().join("launcher.json");
        let runtime = dir.path().join("victory.toml");
        let original = r#"# Keep this comment
[paths]
game_dir = 'D:\\Games\\Old' # Game comment
cache_dir = 'D:\\CustomCache'
data_dir = 'D:\\Saves'
fidelityfx_dir = 'D:\\FidelityFX'
test_titles = ['D:\\OtherGame']
"#;
        std::fs::write(&runtime, original).unwrap();
        let mut cfg = LauncherConfig {
            game_dir: r"D:\Games\New Game's Folder".into(),
            ..LauncherConfig::default()
        };
        save_config_to(&cfg, &launcher, Some(&runtime)).unwrap();
        let updated = std::fs::read_to_string(&runtime).unwrap();
        let decoded = updated.parse::<toml::Table>().unwrap();
        let original_values = original.parse::<toml::Table>().unwrap();
        assert!(updated.contains("# Keep this comment"));
        assert!(updated.contains("# Game comment"));
        assert_eq!(decoded["paths"]["game_dir"].as_str(), Some(cfg.game_dir.as_str()));
        for key in ["cache_dir", "data_dir", "fidelityfx_dir", "test_titles"] {
            assert_eq!(decoded["paths"][key], original_values["paths"][key]);
        }
        assert_eq!(read_launcher_config(&launcher).unwrap().unwrap().game_dir, cfg.game_dir);

        cfg.create_profile("Mi perfil").unwrap();
        save_config_to(&cfg, &launcher, Some(&runtime)).unwrap();
        assert_eq!(std::fs::read_to_string(&runtime).unwrap(), updated);
        assert_eq!(
            read_launcher_config(&launcher).unwrap().unwrap().profile_names().collect::<Vec<_>>(),
            ["Mi perfil"],
        );
    }

    #[test]
    fn malformed_settings_are_never_replaced() {
        let dir = tempfile::tempdir().unwrap();
        let launcher = dir.path().join("launcher.json");
        let runtime = dir.path().join("victory.toml");
        let cfg = LauncherConfig {
            game_dir: "D:/Example".into(),
            ..LauncherConfig::default()
        };
        std::fs::write(&launcher, "corrupt JSON").unwrap();
        assert!(save_config_to(&cfg, &launcher, Some(&runtime)).is_err());
        assert_eq!(std::fs::read_to_string(&launcher).unwrap(), "corrupt JSON");
        assert!(!runtime.exists());

        std::fs::remove_file(&launcher).unwrap();
        std::fs::write(&runtime, "[unclosed").unwrap();
        assert!(save_config_to(&cfg, &launcher, Some(&runtime)).is_err());
        assert_eq!(std::fs::read_to_string(&runtime).unwrap(), "[unclosed");
        assert!(!launcher.exists());
    }
}
