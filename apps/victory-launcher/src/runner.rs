//! Gestión de procesos (`victory.exe`), detección recursiva de caché +
//! importación 1-clic desde caché ScarlettPort, actualización Git en vivo,
//! captura de logs UTF-8 y exportación directa al Escritorio.

use std::collections::{BTreeMap, VecDeque};
use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::ffi::OsStrExt;
use std::os::windows::io::{AsRawHandle, FromRawHandle, OwnedHandle};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant, SystemTime};

use windows_sys::Win32::Foundation::{HWND, LPARAM};
use windows_sys::Win32::Storage::FileSystem::MoveFileW;
use windows_sys::Win32::System::JobObjects::{
    AssignProcessToJobObject, CreateJobObjectW, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
    JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE, JobObjectExtendedLimitInformation,
    SetInformationJobObject,
};
use windows_sys::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetWindowThreadProcessId, IsWindowVisible, PostMessageW, WM_CLOSE,
};
use windows_sys::core::BOOL;

use crate::config::{FsrQuality, GameFilesStatus, LauncherConfig, local_app_data};
use crate::hardware::HardwareInfo;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_LOG_LINES: usize = 3000;
const PREPARATION_STAMPS: &str = ".launcher-prepared";
const GAME_SHUTDOWN_TIMEOUT: Duration = Duration::from_secs(30);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Play,
    NvidiaPlay,
    Warm,
    PrepareAll,
    ImportCache,
    GitUpdate,
}

impl TaskKind {
    pub fn is_play(self) -> bool {
        matches!(self, Self::Play | Self::NvidiaPlay)
    }

    pub fn label_es(self) -> &'static str {
        match self {
            Self::Play => "Juego en ejecución",
            Self::NvidiaPlay => "VIctory-NVIDIA en ejecución",
            Self::Warm => "Acelerando escenas (pipelines)",
            Self::PrepareAll => "Preparando gráficos y escenas",
            Self::ImportCache => "Importando caché existente",
            Self::GitUpdate => "Actualizando y compilando",
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct RunMetrics {
    pub last_fps: Option<f64>,
    pub last_ms: Option<f64>,
    pub last_draws: Option<u32>,
    pub best_fps: f64,
    pub worst_fps: f64,
    pub fps_samples: Vec<f64>,
    pub last_vram_line: String,
    pub last_log_path: Option<PathBuf>,
}

impl RunMetrics {
    pub fn median_fps(&self) -> Option<f64> {
        if self.fps_samples.is_empty() {
            return None;
        }
        let mut sorted = self.fps_samples.clone();
        sorted.sort_by(f64::total_cmp);
        Some(sorted[sorted.len() / 2])
    }

    pub fn observe_line(&mut self, line: &str) {
        if let Some(idx) = line.find(" fps, ") {
            let prefix = &line[..idx];
            let token = prefix.split_whitespace().last().unwrap_or("");
            if let Ok(fps) = token.parse::<f64>()
                && fps.is_finite()
                && fps > 0.1
                && fps < 1000.0
            {
                self.last_fps = Some(fps);
                self.fps_samples.push(fps);
                if self.fps_samples.len() > 2000 {
                    self.fps_samples.remove(0);
                }
                if self.best_fps == 0.0 || fps > self.best_fps {
                    self.best_fps = fps;
                }
                if self.worst_fps == 0.0 || fps < self.worst_fps {
                    self.worst_fps = fps;
                }
            }
            let after = &line[idx + 6..];
            if let Some(ms_idx) = after.find(" ms/frame")
                && let Ok(ms) = after[..ms_idx].trim().parse::<f64>()
            {
                self.last_ms = Some(ms);
            }
            if let Some(d_idx) = after.find(" draws/frame") {
                let before_draws = &after[..d_idx];
                let dtok = before_draws
                    .split(|c: char| c.is_whitespace() || c == ',' || c == ')')
                    .rfind(|s| !s.is_empty())
                    .unwrap_or("");
                if let Ok(draws) = dtok.parse::<u32>() {
                    self.last_draws = Some(draws);
                }
            }
        }
        if line.contains("LOCAL") && line.contains("SHARED") {
            self.last_vram_line = line.trim().to_owned();
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct PrepProgress {
    pub step_label: String,
    pub done: u64,
    pub total: u64,
    pub unsupported: u64,
    pub shaders_cached: Option<u64>,
    pub last_summary: String,
}

impl PrepProgress {
    pub fn fraction(&self) -> f32 {
        if self.total == 0 {
            0.0
        } else {
            (self.done as f32 / self.total as f32).clamp(0.0, 1.0)
        }
    }

    pub fn observe_line(&mut self, line: &str) {
        let trimmed = line.trim();
        if let Some(idx) = trimmed.find(" shaders in the cache") {
            let num_str = trimmed[..idx].split_whitespace().last().unwrap_or("");
            if let Ok(n) = num_str.parse::<u64>() {
                self.done = n;
                if self.total < n {
                    self.total = 26_084.max(n);
                }
                self.step_label = format!("Traduciendo shaders ({n} / {})", self.total);
            }
            if let Some(u_idx) = trimmed.find(" do not translate") {
                let u_num = trimmed[..u_idx].split_whitespace().last().unwrap_or("");
                if let Ok(u) = u_num.parse::<u64>() {
                    self.unsupported = u;
                    self.shaders_cached = Some(self.done);
                }
            }
            self.last_summary = trimmed.to_owned();
            return;
        }
        // `warm.rs`: `25 of 1686, 12 s`
        if let Some((head, _)) = trimmed.split_once(", ")
            && let Some((a, b)) = head.split_once(" of ")
            && let (Ok(done), Ok(total)) = (a.trim().parse::<u64>(), b.trim().parse::<u64>())
            && total > 0
            && done <= total
        {
            self.done = done;
            self.total = total;
            self.step_label = format!("Acelerando escenas ({done} / {total})");
            self.last_summary = trimmed.to_owned();
            return;
        }
        if trimmed.contains("saving ") && trimmed.contains("host-pipelines.bin") {
            self.step_label = "Preparando escenas y guardando pipelines compatibles...".to_owned();
            self.last_summary = trimmed.to_owned();
            return;
        }
        for tok in trimmed.split_whitespace() {
            if let Some((a, b)) = tok.split_once('/') {
                let a = a.trim_matches(|c: char| !c.is_ascii_digit());
                let b = b.trim_matches(|c: char| !c.is_ascii_digit());
                if let (Ok(done), Ok(total)) = (a.parse::<u64>(), b.parse::<u64>())
                    && total >= 10
                    && done <= total
                {
                    self.done = done;
                    self.total = total;
                }
            }
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct LegacyCacheInfo {
    pub source_root: PathBuf,
    pub shader_files: usize,
    pub pipeline_mb: u64,
    pub has_fidelityfx: bool,
    pub has_saves: bool,
}

#[derive(Clone, Debug, Default)]
pub struct CacheStatus {
    pub shader_files: usize,
    pub shader_mb: u64,
    pub shader_count_exact: bool,
    pub pipeline_lib_mb: u64,
    pub pipeline_lib_exists: bool,
    pub early_pipelines_count: usize,
    pub ffx_dll_exists: bool,
    pub legacy_importable: Option<LegacyCacheInfo>,
}

impl CacheStatus {
    /// Escaneo ultrarrápido (< 5 ms) para abrir la ventana al instante sin
    /// recorrer los ~230,000 archivos de shaders en el hilo principal.
    pub fn scan_fast(root: &Path) -> Self {
        let shader_files = count_files_capped(&root.join("shaders"), 500);
        let mut status = Self::scan_metadata_only(root);
        status.shader_files = shader_files;
        status.shader_count_exact = shader_files < 500;
        if shader_files == 0 || !status.pipeline_lib_exists {
            status.legacy_importable = detect_legacy_scarlett_cache_fast();
        }
        status
    }

    /// Escaneo completo (para ejecutar siempre en segundo plano).
    pub fn scan_exact(root: &Path) -> Self {
        let (shader_files, shader_bytes) = count_files_recursive(&root.join("shaders"));
        let mut status = Self::scan_metadata_only(root);
        status.shader_files = shader_files;
        status.shader_mb = shader_bytes / (1024 * 1024);
        status.shader_count_exact = true;
        if shader_files == 0 || !status.pipeline_lib_exists {
            status.legacy_importable = detect_legacy_scarlett_cache_fast();
        }
        status
    }

    fn scan_metadata_only(root: &Path) -> Self {
        let pipes_dir = root.join("pipelines");
        let lib_path = pipes_dir.join("host-pipelines.bin");
        let next_path = pipes_dir.join("host-pipelines.next");
        let active_lib = if next_path.is_file() { &next_path } else { &lib_path };
        let (pipeline_lib_exists, pipeline_lib_mb) = match std::fs::metadata(active_lib) {
            Ok(meta) if meta.is_file() && meta.len() > 0 => (true, meta.len() / (1024 * 1024)),
            _ => (false, 0),
        };
        let early_pipelines_count = std::fs::read_to_string(pipes_dir.join("early-pipelines.txt"))
            .map(|s| s.lines().filter(|l| !l.trim().is_empty()).count())
            .unwrap_or(0);

        let ffx_dll = local_app_data()
            .join("victory")
            .join("fidelityfx")
            .join("amd_fidelityfx_upscaler_dx12.dll");

        Self {
            shader_files: 0,
            shader_mb: 0,
            shader_count_exact: false,
            pipeline_lib_mb,
            pipeline_lib_exists,
            early_pipelines_count,
            ffx_dll_exists: ffx_dll.is_file(),
            legacy_importable: None,
        }
    }

}

fn detect_legacy_scarlett_cache_fast() -> Option<LegacyCacheInfo> {
    let sp_root = local_app_data().join("ScarlettPort");
    if !sp_root.is_dir() {
        return None;
    }
    let shader_files = count_files_capped(&sp_root.join("cache").join("shaders"), 500);
    let pipe_file = sp_root
        .join("cache")
        .join("pipelines")
        .join("host-pipelines.bin");
    let pipeline_mb = std::fs::metadata(&pipe_file)
        .ok()
        .filter(|m| m.is_file())
        .map(|m| m.len() / (1024 * 1024))
        .unwrap_or(0);
    let has_fidelityfx = sp_root
        .join("fidelityfx")
        .join("amd_fidelityfx_upscaler_dx12.dll")
        .is_file();
    let has_saves = sp_root.join("data").is_dir();

    if shader_files > 0 || pipeline_mb > 0 || has_fidelityfx {
        Some(LegacyCacheInfo {
            source_root: sp_root,
            shader_files,
            pipeline_mb,
            has_fidelityfx,
            has_saves,
        })
    } else {
        None
    }
}

/// Cuenta hasta `limit` archivos usando `DirEntry::file_type()` sin `stat`
/// extra por archivo: termina en < 3 ms incluso con 250,000 shaders.
fn count_files_capped(dir: &Path, limit: usize) -> usize {
    let mut files = 0usize;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return 0;
    };
    let mut stack = vec![entries];
    while let Some(mut rd) = stack.pop() {
        for entry in rd.by_ref().flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_file() {
                files += 1;
                if files >= limit {
                    return files;
                }
            } else if ft.is_dir() && entry.file_name() != PREPARATION_STAMPS
                && let Ok(sub) = std::fs::read_dir(entry.path())
            {
                stack.push(sub);
            }
        }
    }
    files
}

fn count_files_recursive(dir: &Path) -> (usize, u64) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut stack = vec![entries];
    while let Some(mut rd) = stack.pop() {
        for entry in rd.by_ref().flatten() {
            let Ok(ft) = entry.file_type() else {
                continue;
            };
            if ft.is_file() {
                files += 1;
                if let Ok(meta) = entry.metadata() {
                    bytes += meta.len();
                }
            } else if ft.is_dir() && entry.file_name() != PREPARATION_STAMPS
                && let Ok(sub) = std::fs::read_dir(entry.path())
            {
                stack.push(sub);
            }
        }
    }
    (files, bytes)
}

#[derive(Clone, Debug, Default)]
pub struct EnginePaths {
    pub repo_root: Option<PathBuf>,
    pub release_victory: Option<PathBuf>,
    pub diag_victory: Option<PathBuf>,
    pub has_git: bool,
}

impl EnginePaths {
    pub fn discover() -> Self {
        let mut out = Self::default();
        let mut candidates = Vec::new();
        if let Ok(exe) = std::env::current_exe()
            && let Some(dir) = exe.parent()
        {
            candidates.push(dir.to_path_buf());
            if let Some(p1) = dir.parent() {
                candidates.push(p1.to_path_buf());
                if let Some(p2) = p1.parent() {
                    candidates.push(p2.to_path_buf());
                }
            }
        }
        if let Ok(cwd) = std::env::current_dir() {
            candidates.push(cwd);
        }

        for dir in &candidates {
            if out.repo_root.is_none()
                && (dir.join(".victory-root").is_file() || dir.join("Cargo.toml").is_file())
            {
                out.repo_root = Some(dir.clone());
                out.has_git = dir.join(".git").exists();
            }
            if out.release_victory.is_none() {
                let side = dir.join("victory.exe");
                let rel = dir.join("target").join("release").join("victory.exe");
                if rel.is_file() {
                    out.release_victory = Some(rel);
                } else if side.is_file()
                    && !dir
                        .file_name()
                        .and_then(|s| s.to_str())
                        .is_some_and(|n| n.eq_ignore_ascii_case("diag"))
                {
                    out.release_victory = Some(side);
                }
            }
            if out.diag_victory.is_none() {
                let diag = dir.join("target").join("diag").join("victory.exe");
                if diag.is_file() {
                    out.diag_victory = Some(diag);
                }
            }
        }
        out
    }

    pub fn active_victory(&self, want_diag: bool) -> Option<&PathBuf> {
        if want_diag {
            self.diag_victory.as_ref().or(self.release_victory.as_ref())
        } else {
            self.release_victory.as_ref().or(self.diag_victory.as_ref())
        }
    }
}

#[derive(Clone, Debug, Default)]
pub struct GitInfo {
    pub available: bool,
    pub branch: String,
    pub local_sha: String,
    pub behind_count: usize,
    pub pending_commits: Vec<String>,
    pub up_to_date: bool,
    pub last_status_es: String,
}

impl GitInfo {
    /// Versión instantánea sin subprocesos para arrancar la ventana en < 100 ms.
    pub fn placeholder(repo_root: Option<&Path>) -> Self {
        let has_git = repo_root.is_some_and(|r| r.join(".git").exists());
        Self {
            available: has_git,
            branch: "nvidia-rtx4090".to_owned(),
            local_sha: "consultando…".to_owned(),
            behind_count: 0,
            pending_commits: Vec::new(),
            up_to_date: false,
            last_status_es: if has_git {
                "Pulsa 'Buscar actualizaciones' para comprobar mejoras nuevas en GitHub."
                    .to_owned()
            } else {
                "Instalación portable (sin repositorio Git local).".to_owned()
            },
        }
    }

    pub fn probe_local(repo_root: Option<&Path>) -> Self {
        let Some(root) = repo_root else {
            return Self {
                last_status_es: "Instalación portable (sin repositorio Git local).".to_owned(),
                ..Default::default()
            };
        };
        if !root.join(".git").exists() {
            return Self {
                last_status_es: "Instalación portable (sin repositorio Git local).".to_owned(),
                ..Default::default()
            };
        }
        let branch = git_stdout(root, &["rev-parse", "--abbrev-ref", "HEAD"])
            .unwrap_or_else(|| "HEAD".to_owned());
        let local_sha = git_stdout(root, &["rev-parse", "--short", "HEAD"])
            .unwrap_or_else(|| "no disponible".to_owned());
        Self {
            available: true,
            branch,
            local_sha,
            behind_count: 0,
            pending_commits: Vec::new(),
            up_to_date: false,
            last_status_es: "Pulsa 'Buscar actualizaciones' para comprobar mejoras nuevas en GitHub."
                .to_owned(),
        }
    }

    pub fn fetch_remote_status(repo_root: &Path) -> Self {
        let mut info = Self::probe_local(Some(repo_root));
        if !info.available {
            return info;
        }
        let remote_key = format!("branch.{}.remote", info.branch);
        let Some(remote) = git_stdout(repo_root, &["config", "--get", &remote_key]) else {
            info.last_status_es = "La rama actual no tiene un remoto de seguimiento.".to_owned();
            return info;
        };
        let fetch = Command::new("git")
            .args(["fetch", "--quiet", "--", &remote])
            .current_dir(repo_root)
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match fetch {
            Ok(o) if o.status.success() => {
                let counts = git_stdout(
                    repo_root,
                    &["rev-list", "--left-right", "--count", "HEAD...@{upstream}"],
                );
                let Some((local, incoming)) = counts.as_deref().and_then(parse_sync_counts) else {
                    info.last_status_es =
                        "No se pudo comparar la rama con su referencia remota.".to_owned();
                    return info;
                };
                if local > 0 {
                    info.last_status_es = if incoming > 0 {
                        format!("La rama y el remoto tienen cambios distintos ({local} locales, {incoming} remotos).")
                    } else {
                        format!("La rama contiene {local} commit(s) locales que el remoto no tiene.")
                    };
                    return info;
                }
                let commits = git_stdout(repo_root, &["log", "--oneline", "HEAD..@{upstream}"])
                    .unwrap_or_default();
                let list: Vec<String> = commits
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                info.behind_count = incoming;
                info.pending_commits = list;
                info.up_to_date = incoming == 0;
                info.last_status_es = if info.behind_count == 0 {
                    "Los archivos fuente del repositorio están al día.".to_owned()
                } else {
                    format!(
                        "Hay {} mejora(s) nueva(s) lista(s) para descargar y compilar.",
                        info.behind_count
                    )
                };
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                info.last_status_es = format!(
                    "No se pudo conectar con GitHub: {}",
                    err.lines().next().unwrap_or("revisa tu conexión o sesión de Git")
                );
            }
            Err(e) => {
                info.last_status_es = format!("Git no encontrado en este PC ({e}).");
            }
        }
        info
    }
}

fn parse_sync_counts(output: &str) -> Option<(usize, usize)> {
    let mut fields = output.split_whitespace();
    Some((fields.next()?.parse().ok()?, fields.next()?.parse().ok()?))
}

fn git_stdout(root: &Path, args: &[&str]) -> Option<String> {
    let out = Command::new("git")
        .args(args)
        .current_dir(root)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    let s = String::from_utf8_lossy(&out.stdout).trim().to_owned();
    (!s.is_empty()).then_some(s)
}

#[derive(Clone, Debug)]
pub struct LogFileEntry {
    pub name: String,
    pub path: PathBuf,
    pub size_kb: u64,
    pub is_crash: bool,
}

#[derive(Default)]
struct SharedState {
    running: Option<TaskKind>,
    started_at: Option<Instant>,
    cancel_requested: bool,
    logs: VecDeque<String>,
    metrics: RunMetrics,
    progress: PrepProgress,
    last_exit_note: Option<String>,
    finished_seq: u64,
}

impl SharedState {
    fn start(&mut self, kind: TaskKind) -> Result<(), String> {
        if self.running.is_some() {
            return Err("Ya hay otra tarea ejecutándose.".to_owned());
        }
        self.running = Some(kind);
        self.started_at = Some(Instant::now());
        self.cancel_requested = false;
        self.last_exit_note = None;
        Ok(())
    }

    fn request_stop(&mut self) -> bool {
        if self.running.is_none() || self.cancel_requested {
            return false;
        }
        self.cancel_requested = true;
        true
    }

    fn finish(&mut self, note: String) {
        self.running = None;
        self.started_at = None;
        self.cancel_requested = false;
        self.finished_seq += 1;
        self.last_exit_note = Some(note);
    }
}

pub struct RunnerSnapshot {
    pub running: Option<TaskKind>,
    pub elapsed_secs: u64,
    pub logs: Vec<String>,
    pub metrics: RunMetrics,
    pub progress: PrepProgress,
    pub last_exit_note: Option<String>,
    pub finished_seq: u64,
}

pub struct ProcessRunner {
    state: Arc<Mutex<SharedState>>,
}

impl Default for ProcessRunner {
    fn default() -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedState::default())),
        }
    }
}

impl ProcessRunner {
    pub fn snapshot(&self) -> RunnerSnapshot {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        RunnerSnapshot {
            running: st.running,
            elapsed_secs: st.started_at.map_or(0, |t| t.elapsed().as_secs()),
            logs: st.logs.iter().cloned().collect(),
            metrics: st.metrics.clone(),
            progress: st.progress.clone(),
            last_exit_note: st.last_exit_note.clone(),
            finished_seq: st.finished_seq,
        }
    }

    pub fn clear_logs(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.logs.clear();
    }

    pub fn stop(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if st.request_stop() {
            let message = if st.running.is_some_and(TaskKind::is_play) {
                "[launcher] Solicitando el cierre del juego para guardar la caché..."
            } else {
                "[launcher] Cancelando la tarea..."
            };
            push_log_locked(
                &mut st,
                message.to_owned(),
            );
        }
    }

    pub fn start_play(
        &self,
        engine: &EnginePaths,
        cfg: &LauncherConfig,
        hw: &HardwareInfo,
    ) -> Result<std::thread::JoinHandle<Result<(), String>>, String> {
        if cfg.engine != crate::nvidia::Engine::Local {
            return self.start_nvidia(cfg, false);
        }
        if is_victory_already_running() {
            return Err("Ya hay una instancia de victory.exe abierta. Ciérrala antes de iniciar otra partida.".to_owned());
        }
        let game = GameFilesStatus::inspect(Path::new(&cfg.game_dir));
        let title_exe = game
            .exe_path
            .ok_or_else(|| "Primero elige la carpeta válida del juego.".to_owned())?;

        let want_diag = cfg.use_diag_build || cfg.gpu_timing;
        let base_victory = engine
            .active_victory(want_diag)
            .ok_or_else(|| "No se encontró victory.exe. Compila primero o usa la pestaña Actualizar.".to_owned())?
            .clone();
        let running_diag = engine.diag_victory.as_ref() == Some(&base_victory);

        let victory_exe = if cfg.smooth_motion {
            let sm_copy = base_victory
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("victory_sm.exe");
            std::fs::copy(&base_victory, &sm_copy)
                .map_err(|e| format!("No se pudo preparar victory_sm.exe para Smooth Motion: {e}"))?;
            sm_copy
        } else {
            base_victory
        };

        let env = runtime_environment(engine, cfg, hw);
        let console = cfg.effective_console(hw).to_owned();
        let args = vec![
            "run".to_owned(),
            title_exe.to_string_lossy().into_owned(),
            "--console".to_owned(),
            console.clone(),
            "--execute".to_owned(),
        ];

        let log_dir = data_dir();
        let _ = std::fs::create_dir_all(&log_dir);
        let stamp = SystemTime::now()
            .duration_since(SystemTime::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis();
        let log_path = log_dir.join(format!("nvidia-run-{stamp}.log"));

        // Encabezado limpio UTF-8 para que cualquier log compartido tenga toda la info del PC.
        let header = format!(
            "=== VIctory-RTX-Mc3yohan (Launcher v{} @ {}) ===\n\
             GPU      : {} ({} MiB VRAM, Tier {})\n\
             RAM      : {:.1} GB total ({:.1} GB libre, {:.1} GB commit libre) | {} hilos\n\
             Perfil   : {} | Consola: {} | Salida: {} | Escala: {:?}\n\
             Ajustes guardados: {:?}\n\
             Build    : {} | PortRT={} | Thin={} | LiveTex={} | VideoPlanes={}\n\
             ==============================================================\n",
            env!("CARGO_PKG_VERSION"),
            env!("VICTORY_GIT_SHA"),
            hw.gpu_name,
            hw.vram_total_mb,
            hw.vram_tier.label(),
            hw.ram_total_gb,
            hw.ram_free_gb,
            hw.commit_free_gb,
            hw.logical_cpus,
            cfg.preset.label_es(),
            console,
            cfg.effective_output(hw).label_es(),
            cfg.effective_scale(hw),
            cfg.matching_profile().unwrap_or("Ajustes sin perfil coincidente"),
            if running_diag { "diag" } else { "release" },
            cfg.port_rt,
            cfg.effective_thin(hw),
            cfg.live_textures,
            cfg.video_planes,
        );

        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.start(TaskKind::Play)?;
            st.metrics = RunMetrics {
                last_log_path: Some(log_path.clone()),
                ..Default::default()
            };
            push_log_locked(
                &mut st,
                format!(
                    "[launcher] Iniciando VIctory-RTX ({}, {}, {} ajustes)...",
                    cfg.preset.label_es(),
                    console,
                    env.len()
                ),
            );
        }

        let work_dir = engine
            .repo_root
            .clone()
            .or_else(|| victory_exe.parent().map(Path::to_path_buf));
        let state = self.state.clone();
        Ok(std::thread::spawn(move || {
            let mut log_file = OpenOptions::new()
                .create_new(true)
                .write(true)
                .open(&log_path)
                .ok();
            if let Some(f) = log_file.as_mut() {
                let _ = f.write_all(header.as_bytes());
                let digest = File::open(&victory_exe).and_then(|file| {
                    let mut hash = blake3::Hasher::new();
                    hash.update_reader(file)?;
                    Ok(hash.finalize())
                });
                match digest {
                    Ok(hash) => {
                        let name = victory_exe.file_name().unwrap_or_default().to_string_lossy();
                        let _ = writeln!(f, "Motor    : {name} | BLAKE3 {hash}");
                    }
                    Err(e) => {
                        let _ = writeln!(f, "Motor    : no se pudo calcular la huella: {e}");
                    }
                }
                let _ = f.flush();
            }
            let code = run_single_process(
                &state,
                &victory_exe,
                &args,
                &env,
                work_dir.as_deref(),
                false,
                log_file,
                TaskKind::Play,
            );
            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            let success = matches!(&code, Ok(0));
            let note = match code {
                Ok(0) => "Partida cerrada correctamente.".to_owned(),
                _ if st.cancel_requested => "Partida detenida. Consulta el registro para comprobar el cierre.".to_owned(),
                Ok(c) => format!("El juego terminó con código {c}. Revisa 'Logs y Soporte' si hubo un error."),
                Err(e) => format!("No se pudo iniciar el motor: {e}"),
            };
            st.finish(note.clone());
            if success { Ok(()) } else { Err(note) }
        }))
    }

    fn start_nvidia(
        &self,
        cfg: &LauncherConfig,
        preload: bool,
    ) -> Result<std::thread::JoinHandle<Result<(), String>>, String> {
        if is_victory_already_running() {
            return Err("Ya hay un motor en ejecución. Termina esa sesión antes de abrir VIctory-NVIDIA.".into());
        }
        let game = GameFilesStatus::inspect(Path::new(&cfg.game_dir));
        let title = game.exe_path.ok_or("Selecciona una carpeta de juego válida.")?;
        let mut command = crate::nvidia::command(cfg, &title, preload)?;
        command.creation_flags(CREATE_NO_WINDOW);
        std::fs::create_dir_all(data_dir()).map_err(|e| e.to_string())?;
        let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
        let path = data_dir().join(format!("nvidia-run-{time}.log"));
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let mut log = File::create_new(&path).map_err(|e| e.to_string())?;
        writeln!(log, "{} | {} | F1 puede modificar los ajustes efectivos", cfg.engine.label(), command.get_program().to_string_lossy())
            .map_err(|e| e.to_string())?;
        writeln!(log, "Ajustes guardados: {:?}", cfg.matching_profile().unwrap_or("Ajustes sin perfil coincidente"))
            .map_err(|e| e.to_string())?;
        st.start(TaskKind::NvidiaPlay)?;
        st.metrics = RunMetrics { last_log_path: Some(path), ..Default::default() };
        push_log_locked(&mut st, if preload { "Precargando gráficos antes de jugar con VIctory-NVIDIA." } else { "Iniciando VIctory-NVIDIA." }.to_owned());
        drop(st);
        let state = self.state.clone();
        Ok(std::thread::spawn(move || {
            let code = run_command(&state, command, Some(log), TaskKind::NvidiaPlay, true);
            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            let success = matches!(&code, Ok(0));
            let note = match code {
                Ok(0) => "Partida de VIctory-NVIDIA cerrada.".to_owned(),
                Ok(code) => format!("VIctory-NVIDIA terminó con código {code}. Consulta el registro."),
                Err(e) => e,
            };
            st.finish(note.clone());
            if success { Ok(()) } else { Err(note) }
        }))
    }

    pub fn start_prep(
        &self,
        engine: &EnginePaths,
        cfg: &LauncherConfig,
        hw: HardwareInfo,
        kind: TaskKind,
    ) -> Result<(), String> {
        if cfg.engine != crate::nvidia::Engine::Local {
            return self.start_nvidia(cfg, true).map(drop);
        }
        self.start_local_preparation(engine, cfg, hw, kind).map(drop)
    }

    pub fn start_local_preparation(
        &self,
        engine: &EnginePaths,
        cfg: &LauncherConfig,
        hw: HardwareInfo,
        kind: TaskKind,
    ) -> Result<std::thread::JoinHandle<Result<(), String>>, String> {
        if cfg.engine != crate::nvidia::Engine::Local {
            return Err("Selecciona un perfil del motor local para preparar sin iniciar el juego.".into());
        }
        let victory_exe = engine
            .active_victory(cfg.use_diag_build || cfg.gpu_timing)
            .ok_or_else(|| "No se encontró victory.exe.".to_owned())?
            .clone();
        let game = GameFilesStatus::inspect(Path::new(&cfg.game_dir));
        if !game.ready() && matches!(kind, TaskKind::PrepareAll) {
            return Err("Primero selecciona la carpeta válida del juego.".to_owned());
        }
        let env = runtime_environment(engine, cfg, &hw);
        let game_dir = cfg.game_dir.clone();
        let work_dir = engine_work_dir(engine, &victory_exe).to_path_buf();
        let cache_root = resolved_cache_root(&work_dir, &env)?;
        let stamp = if matches!(kind, TaskKind::PrepareAll) {
            Some(preparation_stamp(
                &cache_root,
                &victory_exe,
                game.exe_path.as_deref().ok_or("Falta el ejecutable del juego.")?,
                cfg,
                &hw,
                &env,
            ).map_err(|e| format!("No se pudo verificar el perfil gráfico: {e}"))?)
        } else {
            None
        };

        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        if st.running.is_some() {
            return Err("Ya hay otra tarea ejecutándose.".into());
        }
        let dir = data_dir();
        std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
        let time = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_millis();
        let log_path = dir.join(format!("nvidia-prep-{time}-{}.log", std::process::id()));
        let log = File::create_new(&log_path).map_err(|e| e.to_string())?;
        let library_log = log.try_clone().map_err(|e| e.to_string())?;
        st.start(kind)?;
        st.progress = PrepProgress {
            step_label: kind.label_es().to_owned(),
            ..Default::default()
        };
        push_log_locked(&mut st, format!("[launcher] Iniciando {}. Registro: {}", kind.label_es(), log_path.display()));
        drop(st);
        let state = self.state.clone();
        Ok(std::thread::spawn(move || {
            let result = match kind {
                TaskKind::Warm => {
                    let args = vec![
                        "warm".to_owned(),
                        "--save-library".to_owned(),
                        "--used".to_owned(),
                    ];
                    run_single_process(
                        &state,
                        &victory_exe,
                        &args,
                        &env,
                        Some(&work_dir),
                        true,
                        Some(log),
                        kind,
                    )
                }
                TaskKind::PrepareAll => {
                    {
                        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                        st.progress.step_label = "Paso 1/2 · Traduciendo shaders Wave32...".to_owned();
                    }
                    let args1 = vec!["shaders".to_owned(), game_dir];
                    match run_single_process(
                        &state,
                        &victory_exe,
                        &args1,
                        &env,
                        Some(&work_dir),
                        true,
                        Some(log),
                        kind,
                    ) {
                        Ok(0) => {
                            let early = cache_root.join("pipelines").join("early-pipelines.txt");
                            if early.is_file() {
                                {
                                    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                                    st.progress.done = 0;
                                    st.progress.total = 0;
                                    st.progress.step_label =
                                        "Paso 2/2 · Acelerando escenas usadas...".to_owned();
                                    push_log_locked(
                                        &mut st,
                                        "[launcher] Shaders listos. Iniciando calentamiento de pipelines...".to_owned(),
                                    );
                                }
                                let args2 = vec![
                                    "warm".to_owned(),
                                    "--save-library".to_owned(),
                                    "--used".to_owned(),
                                ];
                                run_single_process(
                                    &state,
                                    &victory_exe,
                                    &args2,
                                    &env,
                                    Some(&work_dir),
                                    true,
                                    Some(library_log),
                                    kind,
                                )
                            } else {
                                let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                                push_log_locked(
                                    &mut st,
                                    "[launcher] Shaders traducidos. Los pipelines del mundo se registrarán automáticamente en tu primera partida.".to_owned(),
                                );
                                Ok(0)
                            }
                        }
                        other => other,
                    }
                }
                _ => Ok(0),
            };

            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            let result = match (&result, &stamp) {
                (Ok(0), Some(stamp)) if !st.cancel_requested => {
                    if st.progress.shaders_cached.unwrap_or(0) == 0 {
                        Err("La preparación no confirmó shaders en la caché.".to_owned())
                    } else {
                        record_preparation(stamp).map(|()| 0)
                    }
                }
                _ => result,
            };
            let success = !st.cancel_requested && matches!(&result, Ok(0));
            let note = match result {
                _ if st.cancel_requested => "Preparación cancelada.".to_owned(),
                Ok(0) if st.progress.unsupported > 0 => format!(
                    "{} completado con {} shaders sin traducir. Consulta el registro.",
                    kind.label_es(),
                    st.progress.unsupported
                ),
                Ok(0) => format!("{} completado correctamente.", kind.label_es()),
                Ok(c) => format!("{} terminó con código {c}.", kind.label_es()),
                Err(e) => format!("Error en {}: {e}", kind.label_es()),
            };
            st.finish(note.clone());
            if success { Ok(()) } else { Err(note) }
        }))
    }

    /// Importa en segundo plano caché de shaders, pipelines, FidelityFX y partidas
    /// guardadas desde `%LOCALAPPDATA%\ScarlettPort` hacia `%LOCALAPPDATA%\victory`.
    pub fn start_import_legacy_cache(&self, info: LegacyCacheInfo) -> Result<(), String> {
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.start(TaskKind::ImportCache)?;
            st.progress = PrepProgress {
                step_label: "Importando caché y partidas existentes...".to_owned(),
                done: 0,
                total: 4,
                ..Default::default()
            };
            push_log_locked(
                &mut st,
                format!(
                    "[launcher] Importando caché desde {}...",
                    info.source_root.display()
                ),
            );
        }

        let state = self.state.clone();
        std::thread::spawn(move || {
            let dst_root = local_app_data().join("victory");
            let mut copied_items = 0usize;

            let steps: [(&str, PathBuf, PathBuf); 4] = [
                (
                    "Pipelines precompilados",
                    info.source_root.join("cache").join("pipelines"),
                    dst_root.join("cache").join("pipelines"),
                ),
                (
                    "FidelityFX DLLs",
                    info.source_root.join("fidelityfx"),
                    dst_root.join("fidelityfx"),
                ),
                (
                    "Partidas guardadas",
                    info.source_root.join("data"),
                    dst_root.join("data"),
                ),
                (
                    "Shaders traducidos",
                    info.source_root.join("cache").join("shaders"),
                    dst_root.join("cache").join("shaders"),
                ),
            ];

            for (idx, (label, src, dst)) in steps.into_iter().enumerate() {
                {
                    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                    if st.cancel_requested {
                        break;
                    }
                    st.progress.done = idx as u64;
                    st.progress.step_label = format!("Importando {label}...");
                }
                if src.is_dir() {
                    let count =
                        copy_dir_non_destructive(&state, &src, &dst, label == "Partidas guardadas");
                    copied_items += count;
                    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                    push_log_locked(
                        &mut st,
                        format!("[launcher] {label}: {count} archivo(s) listos."),
                    );
                }
            }

            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            let note = if st.cancel_requested {
                "Importación cancelada. Los archivos ya copiados se conservaron.".to_owned()
            } else {
                st.progress.done = 4;
                format!("Importación lista ({copied_items} archivos copiados sin borrar el origen).")
            };
            st.finish(note);
        });

        Ok(())
    }

    /// Avanza las fuentes por fast-forward y recompila el launcher y el motor.
    pub fn start_git_update(&self, repo_root: PathBuf, rebuild_engine: bool) -> Result<(), String> {
        let rebuild_diag =
            rebuild_engine && repo_root.join("target").join("diag").join("victory.exe").is_file();
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.start(TaskKind::GitUpdate)?;
            st.progress = PrepProgress {
                step_label: "Comprobando cambios locales...".to_owned(),
                total: 2 + 2 * u64::from(rebuild_engine) + u64::from(rebuild_diag),
                ..Default::default()
            };
        }

        let state = self.state.clone();
        std::thread::spawn(move || {
            let mut sources_ready = false;
            let mut update = || -> Result<(), String> {
                require_clean_tree(&repo_root)?;
                update_step(&state, 0, "Descargando cambios de la rama remota...");
                git_update_command(&state, &repo_root, &["fetch", "--quiet"], true)?;

                require_clean_tree(&repo_root)?;
                update_step(&state, 1, "Aplicando cambios a los archivos fuente...");
                // Git debe terminar de actualizar el árbol aunque se cancele esta etapa.
                git_update_command(&state, &repo_root, &["merge", "--ff-only", "@{upstream}"], false)?;
                sources_ready = true;

                if rebuild_engine {
                    update_step(&state, 2, "Compilando el motor en release...");
                    build_engine(&state, &repo_root, "release")?;
                }
                if rebuild_diag {
                    update_step(&state, 3, "Compilando el motor con diagnóstico...");
                    build_engine(&state, &repo_root, "diag")?;
                }
                if rebuild_engine {
                    update_step(&state, 3 + u64::from(rebuild_diag), "Compilando el launcher...");
                    build_launcher(&state, &repo_root)?;
                }
                Ok(())
            };
            let result = update();

            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            let note = match result {
                _ if st.cancel_requested && sources_ready =>
                    "Actualización cancelada. Las fuentes ya están listas. Vuelve a compilar para usar el motor actualizado.".to_owned(),
                _ if st.cancel_requested => "Actualización cancelada.".to_owned(),
                Ok(()) => {
                    st.progress.done = st.progress.total;
                    if rebuild_engine {
                        let variants = if rebuild_diag { "release y diag" } else { "release" };
                        format!("Motor {variants} y launcher compilados. Vuelve a abrir el launcher para cargar la nueva versión.")
                    } else {
                        "Fuentes actualizadas. Queda pendiente compilar el motor y el launcher.".to_owned()
                    }
                }
                Err(e) if sources_ready => format!("Las fuentes avanzaron, pero la compilación falló: {e}. Reintenta para completar la actualización."),
                Err(e) => format!("No se pudo actualizar: {e}"),
            };
            st.finish(note);
        });

        Ok(())
    }
}

fn require_clean_tree(root: &Path) -> Result<(), String> {
    let output = Command::new("git")
        .args(["status", "--porcelain", "--untracked-files=normal"])
        .current_dir(root)
        .creation_flags(CREATE_NO_WINDOW)
        .output()
        .map_err(|e| format!("No se pudo consultar Git: {e}"))?;
    if !output.status.success() {
        return Err("Git no pudo comprobar los cambios locales.".to_owned());
    }
    if !output.stdout.is_empty() {
        return Err("Hay cambios locales. Guárdalos antes de actualizar las fuentes.".to_owned());
    }
    Ok(())
}

fn update_step(state: &Mutex<SharedState>, done: u64, message: &str) {
    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
    st.progress.done = done;
    st.progress.step_label = message.to_owned();
    push_log_locked(&mut st, format!("[actualizar] {message}"));
}

fn git_update_command(
    state: &Arc<Mutex<SharedState>>,
    root: &Path,
    args: &[&str],
    interruptible: bool,
) -> Result<(), String> {
    let mut cmd = Command::new("git");
    cmd.args(args).current_dir(root).creation_flags(CREATE_NO_WINDOW);
    let status = run_command(state, cmd, None, TaskKind::GitUpdate, interruptible)?;
    if status == 0 {
        Ok(())
    } else {
        Err(format!("git {} terminó con código {status}. Consulta el registro", args[0]))
    }
}

fn build_engine(state: &Arc<Mutex<SharedState>>, root: &Path, profile: &str) -> Result<(), String> {
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--locked", "--profile", profile, "-p", "victory"])
        .current_dir(root)
        .creation_flags(CREATE_NO_WINDOW);
    if profile == "diag" {
        cmd.args(["--features", "diagnostics"]);
    }
    let status = run_command(state, cmd, None, TaskKind::GitUpdate, true)?;
    if status == 0 {
        Ok(())
    } else {
        Err(format!("cargo build ({profile}) terminó con código {status}. Consulta el registro"))
    }
}

fn build_launcher(state: &Arc<Mutex<SharedState>>, root: &Path) -> Result<(), String> {
    let launcher = root.join("target").join("release").join("VIctory-RTX.exe");
    let mut cmd = Command::new("cargo");
    cmd.args(["build", "--locked", "--release", "-p", "victory-launcher"])
        .current_dir(root)
        .creation_flags(CREATE_NO_WINDOW);
    with_launcher_backup(&launcher, || {
        let status = run_command(state, cmd, None, TaskKind::GitUpdate, true)?;
        if status == 0 {
            Ok(())
        } else {
            Err(format!("cargo build del launcher terminó con código {status}"))
        }
    })
}

/// Windows permite renombrar la imagen del launcher que sigue abierta. Cargo
/// puede entonces instalar el nuevo exe, y podemos restaurar el anterior si falla.
fn with_launcher_backup(launcher: &Path, build: impl FnOnce() -> Result<(), String>) -> Result<(), String> {
    let backup = if launcher.is_file() {
        let id = SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default().as_nanos();
        let backup = launcher.with_extension(format!("previous-{}-{id}.exe", std::process::id()));
        std::fs::rename(launcher, &backup)
            .map_err(|e| format!("No se pudo conservar el launcher anterior: {e}"))?;
        Some(backup)
    } else {
        None
    };
    let install = build().and_then(|()| {
        launcher.is_file().then_some(())
            .ok_or_else(|| "Cargo no instaló el ejecutable del launcher.".to_owned())
    });
    match (install, backup) {
        (Err(problem), Some(backup)) => {
            std::fs::rename(&backup, launcher).map_err(|e| format!(
                "{problem}. Falló la restauración: {e}. El launcher anterior queda en {}", backup.display()
            ))?;
            Err(problem)
        }
        (result, _) => result,
    }
}

fn copy_dir_non_destructive(
    state: &Mutex<SharedState>,
    src: &Path,
    dst: &Path,
    skip_logs: bool,
) -> usize {
    let mut copied = 0usize;
    if std::fs::create_dir_all(dst).is_err() {
        return 0;
    }
    let Ok(dst) = dst.canonicalize() else {
        return 0;
    };
    let Ok(entries) = std::fs::read_dir(src) else {
        return 0;
    };
    for entry in entries.flatten() {
        if stop_requested(state) {
            break;
        }
        let from = entry.path();
        let name = entry.file_name();
        if skip_logs {
            let lower = name.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(".log") {
                continue;
            }
        }
        let to = dst.join(&name);
        let Ok(kind) = entry.file_type() else {
            continue;
        };
        if kind.is_dir() {
            copied += copy_dir_non_destructive(state, &from, &to, skip_logs);
        } else if kind.is_file() && !to.exists() && copy_new_file(&from, &to).is_ok() {
            copied += 1;
        }
    }
    copied
}

fn copy_new_file(from: &Path, to: &Path) -> std::io::Result<()> {
    static NEXT_COPY: AtomicU64 = AtomicU64::new(0);
    let index = NEXT_COPY.fetch_add(1, Ordering::Relaxed);
    let staging = to.with_file_name(format!(".victory-import-{}-{index}", std::process::id()));
    File::create_new(&staging)?;
    let result = std::fs::copy(from, &staging).and_then(|_| {
        let source: Vec<u16> = staging.as_os_str().encode_wide().chain([0]).collect();
        let target: Vec<u16> = to.as_os_str().encode_wide().chain([0]).collect();
        // SAFETY: both paths are valid UTF-16 strings with a terminating NUL.
        // MoveFileW refuses to replace a destination created by another import.
        if unsafe { MoveFileW(source.as_ptr(), target.as_ptr()) } == 0 {
            return Err(std::io::Error::last_os_error());
        }
        Ok(())
    });
    if result.is_err() {
        let _ = std::fs::remove_file(staging);
    }
    result
}

#[allow(clippy::too_many_arguments)]
fn run_single_process(
    state: &Arc<Mutex<SharedState>>,
    exe: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    work_dir: Option<&Path>,
    hide_window: bool,
    log_file: Option<File>,
    kind: TaskKind,
) -> Result<i32, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args).envs(env);
    if let Some(dir) = work_dir {
        cmd.current_dir(dir);
    }
    if hide_window {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    run_command(state, cmd, log_file, kind, true)
}

fn run_command(
    state: &Arc<Mutex<SharedState>>,
    mut cmd: Command,
    log_file: Option<File>,
    kind: TaskKind,
    interruptible: bool,
) -> Result<i32, String> {
    cmd.stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    let mut child = {
        // Mantiene la comprobación y el arranque en el mismo orden que Detener.
        let st = state.lock().unwrap_or_else(|e| e.into_inner());
        if st.cancel_requested {
            return Err("Tarea cancelada antes de iniciar el proceso.".to_owned());
        }
        cmd.spawn()
            .map_err(|e| format!("No se pudo ejecutar {}: {e}", cmd.get_program().display()))?
    };
    let mut job = if kind == TaskKind::Play {
        None
    } else {
        match child_job(&child) {
            Ok(job) => Some(job),
            Err(e) => {
                if child.try_wait().map_err(|e| e.to_string())?.is_none() {
                    kill_and_wait(&mut child).map_err(|e| e.to_string())?;
                    return Err(format!("No se pudo administrar el grupo de procesos: {e}"));
                }
                None
            }
        }
    };
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();

    let shared_file = log_file.map(|f| Arc::new(Mutex::new(f)));

    let t_out = stdout.map(|pipe| {
        let st = state.clone();
        let lf = shared_file.clone();
        std::thread::spawn(move || pump_reader(pipe, st, lf, kind))
    });
    let t_err = stderr.map(|pipe| {
        let st = state.clone();
        let lf = shared_file.clone();
        std::thread::spawn(move || pump_reader(pipe, st, lf, kind))
    });

    let result = if kind == TaskKind::NvidiaPlay && let Some(group) = &job {
        crate::nvidia_session::wait(&mut child, group, || stop_requested(state), GAME_SHUTDOWN_TIMEOUT)
    } else {
        let mut close_started = None;
        let mut close_sent = false;
        loop {
            if let Some(status) = child.try_wait().map_err(|e| e.to_string())? {
                break Ok(status.code().unwrap_or(-1));
            }
            if interruptible && stop_requested(state) {
                if kind != TaskKind::Play {
                    drop(job.take());
                    child.wait().map_err(|e| e.to_string())?;
                    break Err("Tarea cancelada en ejecución.".to_owned());
                }
                let start = close_started.get_or_insert_with(Instant::now);
                if !close_sent {
                    close_sent = request_game_close(child.id());
                }
                if start.elapsed() >= GAME_SHUTDOWN_TIMEOUT {
                    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                    push_log_locked(&mut st, "[launcher] El juego no terminó a tiempo. Cerrando de forma forzada.".to_owned());
                    drop(st);
                    kill_and_wait(&mut child).map_err(|e| e.to_string())?;
                    break Err("Tarea cancelada en ejecución.".to_owned());
                }
            }
            std::thread::sleep(Duration::from_millis(80));
        }
    };
    // Cierra también cualquier descendiente residual que retenga las tuberías.
    drop(job);

    if let Some(t) = t_out {
        let _ = t.join();
    }
    if let Some(t) = t_err {
        let _ = t.join();
    }

    result
}

fn stop_requested(state: &Mutex<SharedState>) -> bool {
    state
        .lock()
        .unwrap_or_else(|e| e.into_inner())
        .cancel_requested
}

fn kill_and_wait(child: &mut Child) -> std::io::Result<std::process::ExitStatus> {
    if let Err(error) = child.kill() {
        return child.try_wait()?.ok_or(error);
    }
    child.wait()
}

pub(crate) fn child_job(child: &Child) -> std::io::Result<OwnedHandle> {
    // SAFETY: null asks for an unnamed job with the default security attributes.
    let handle = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if handle.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    // SAFETY: CreateJobObjectW returned a fresh handle owned only by this worker.
    let job = unsafe { OwnedHandle::from_raw_handle(handle) };
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    // SAFETY: both handles are live, and limits is the value type required by
    // JobObjectExtendedLimitInformation for the duration of the call.
    let assigned = unsafe {
        SetInformationJobObject(
            job.as_raw_handle(),
            JobObjectExtendedLimitInformation,
            std::ptr::from_ref(&limits).cast(),
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        ) != 0
            && AssignProcessToJobObject(job.as_raw_handle(), child.as_raw_handle()) != 0
    };
    if !assigned {
        return Err(std::io::Error::last_os_error());
    }
    Ok(job)
}

fn request_game_close(pid: u32) -> bool {
    struct CloseRequest {
        pid: u32,
        posted: bool,
    }

    unsafe extern "system" fn close_window(hwnd: HWND, data: LPARAM) -> BOOL {
        // SAFETY: EnumWindows invokes the callback synchronously. data points
        // to the unique request on the calling thread's stack.
        let request = unsafe { &mut *(data as *mut CloseRequest) };
        let mut owner = 0;
        // SAFETY: hwnd is supplied by EnumWindows; owner is valid output storage.
        unsafe {
            GetWindowThreadProcessId(hwnd, &mut owner);
            if owner == request.pid && IsWindowVisible(hwnd) != 0 {
                request.posted |= PostMessageW(hwnd, WM_CLOSE, 0, 0) != 0;
            }
        }
        1
    }

    let mut request = CloseRequest { pid, posted: false };
    // SAFETY: request remains live for the synchronous enumeration and is not
    // accessed by this thread again until the call returns.
    unsafe { EnumWindows(Some(close_window), std::ptr::from_mut(&mut request) as LPARAM) };
    request.posted
}

fn pump_reader<R: std::io::Read>(
    reader: R,
    state: Arc<Mutex<SharedState>>,
    log_file: Option<Arc<Mutex<File>>>,
    kind: TaskKind,
) {
    let mut br = BufReader::new(reader);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match br.read_until(b'\n', &mut buf) {
            Ok(0) => break,
            Ok(_) => {
                let raw = String::from_utf8_lossy(&buf);
                let clean = strip_ansi(raw.trim_end_matches(['\r', '\n']));
                if let Some(lf) = &log_file
                    && let Ok(mut f) = lf.lock()
                {
                    let _ = writeln!(f, "{clean}");
                }
                let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                match kind {
                    TaskKind::Play | TaskKind::NvidiaPlay => st.metrics.observe_line(&clean),
                    TaskKind::Warm | TaskKind::PrepareAll => {
                        st.progress.observe_line(&clean)
                    }
                    TaskKind::ImportCache | TaskKind::GitUpdate => {}
                }
                push_log_locked(&mut st, clean);
            }
            Err(_) => break,
        }
    }
}

fn push_log_locked(st: &mut SharedState, line: String) {
    if st.logs.len() >= MAX_LOG_LINES {
        st.logs.pop_front();
    }
    st.logs.push_back(line);
}

pub fn strip_ansi(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' {
            if chars.peek() == Some(&'[') {
                chars.next();
                for ch in chars.by_ref() {
                    if ch.is_ascii_alphabetic() {
                        break;
                    }
                }
            }
            continue;
        }
        out.push(c);
    }
    out
}

pub fn cache_dir() -> PathBuf {
    local_app_data().join("victory").join("cache")
}

fn engine_work_dir<'a>(engine: &'a EnginePaths, exe: &'a Path) -> &'a Path {
    engine.repo_root.as_deref().or_else(|| exe.parent()).unwrap_or_else(|| Path::new("."))
}

fn runtime_environment(
    engine: &EnginePaths,
    cfg: &LauncherConfig,
    hw: &HardwareInfo,
) -> BTreeMap<String, String> {
    let mut env = cfg.build_run_env(hw);
    if cfg.fsr_4k != FsrQuality::Off
        && !env.contains_key("VICTORY_FIDELITYFX_DIR")
        && let Some(dir) = crate::config::fidelityfx_dir(engine.repo_root.as_deref())
    {
        env.insert("VICTORY_FIDELITYFX_DIR".into(), dir.to_string_lossy().into_owned());
    }
    env
}

fn resolved_cache_root(
    work_dir: &Path,
    env: &BTreeMap<String, String>,
) -> Result<PathBuf, String> {
    let config = victory_core::Config::load_in(work_dir, |key| {
        env.get(key).map(OsString::from).or_else(|| std::env::var_os(key))
    }).map_err(|e| format!("No se pudo leer la configuración del motor: {e:#}"))?;
    let root = work_dir.join(config.cache_dir);
    victory_core::ensure_outside_repo(&root)
        .map_err(|e| format!("Directorio de caché inválido: {e}"))?;
    Ok(root)
}

pub fn configured_cache_root(
    engine: &EnginePaths,
    cfg: &LauncherConfig,
    hw: &HardwareInfo,
) -> PathBuf {
    if cfg.engine != crate::nvidia::Engine::Local {
        return crate::nvidia::home(&crate::nvidia::root(), cfg.engine).join("cache");
    }
    let Some(exe) = engine.active_victory(cfg.use_diag_build || cfg.gpu_timing) else {
        return cache_dir();
    };
    resolved_cache_root(engine_work_dir(engine, exe), &runtime_environment(engine, cfg, hw))
        .unwrap_or_else(|_| cache_dir())
}

fn preparation_stamp(
    root: &Path,
    exe: &Path,
    title: &Path,
    cfg: &LauncherConfig,
    hw: &HardwareInfo,
    env: &BTreeMap<String, String>,
) -> std::io::Result<PathBuf> {
    let mut hash = blake3::Hasher::new();
    hash.update(b"victory-launcher-prepared-1");
    for path in [exe, title] {
        let metadata = path.metadata()?;
        hash_prep_part(&mut hash, path.as_os_str().as_encoded_bytes());
        hash.update(&metadata.len().to_le_bytes());
        let mtime = metadata.modified()?.duration_since(SystemTime::UNIX_EPOCH).unwrap_or_default();
        hash.update(&mtime.as_nanos().to_le_bytes());
    }
    hash_prep_part(&mut hash, hw.gpu_name.as_bytes());
    hash.update(&hw.vram_total_mb.to_le_bytes());
    hash_prep_part(&mut hash, cfg.effective_console(hw).as_bytes());
    let mut settings: BTreeMap<String, OsString> = std::env::vars_os()
        .filter_map(|(key, value)| {
            let key = key.to_str()?.to_ascii_uppercase();
            key.starts_with("VICTORY_").then_some((key, value))
        })
        .collect();
    for (key, value) in env {
        settings.insert(key.to_ascii_uppercase(), OsString::from(value));
    }
    for (key, value) in settings {
        hash_prep_part(&mut hash, key.as_bytes());
        hash_prep_part(&mut hash, value.as_encoded_bytes());
    }
    Ok(root.join("shaders").join(PREPARATION_STAMPS).join(hash.finalize().to_hex().as_str()))
}

fn hash_prep_part(hash: &mut blake3::Hasher, bytes: &[u8]) {
    hash.update(&(bytes.len() as u64).to_le_bytes());
    hash.update(bytes);
}

fn record_preparation(stamp: &Path) -> Result<(), String> {
    let folder = stamp.parent().ok_or("Ruta del registro de preparación inválida.")?;
    victory_core::ensure_outside_repo(folder).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(folder)
        .and_then(|()| OpenOptions::new().create(true).write(true).truncate(false).open(stamp))
        .map(|_| ())
        .map_err(|e| format!("No se pudo guardar la verificación de la caché: {e}"))
}

pub fn preparation_verified(
    engine: &EnginePaths,
    cfg: &LauncherConfig,
    hw: &HardwareInfo,
    title: Option<&Path>,
) -> bool {
    if cfg.engine != crate::nvidia::Engine::Local {
        return false;
    }
    let (Some(exe), Some(title)) = (engine.active_victory(cfg.use_diag_build || cfg.gpu_timing), title)
    else { return false };
    let env = runtime_environment(engine, cfg, hw);
    let Ok(root) = resolved_cache_root(engine_work_dir(engine, exe), &env) else {
        return false;
    };
    preparation_stamp(&root, exe, title, cfg, hw, &env).is_ok_and(|stamp| stamp.is_file())
        && count_files_capped(&root.join("shaders"), 1) > 0
}

pub fn data_dir() -> PathBuf {
    local_app_data().join("victory").join("data")
}

pub fn scan_data_logs() -> Vec<LogFileEntry> {
    let dir = data_dir();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut out: Vec<(SystemTime, LogFileEntry)> = Vec::new();
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_owned();
        let lower = name.to_ascii_lowercase();
        if lower.ends_with(".log") || lower.ends_with(".dmp") || lower.contains("crash") {
            let meta = entry.metadata().ok();
            let modified = meta
                .as_ref()
                .and_then(|m| m.modified().ok())
                .unwrap_or(SystemTime::UNIX_EPOCH);
            let size_kb = meta.as_ref().map_or(0, |m| (m.len() / 1024).max(1));
            let is_crash = lower.contains("crash") || lower.ends_with(".dmp");
            out.push((
                modified,
                LogFileEntry {
                    name,
                    path,
                    size_kb,
                    is_crash,
                },
            ));
        }
    }
    out.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    out.into_iter().take(25).map(|(_, e)| e).collect()
}

pub fn read_log_tail(path: &Path, max_lines: usize) -> Vec<String> {
    let Ok(raw) = std::fs::read(path) else {
        return vec!["No se pudo leer el archivo seleccionado.".to_owned()];
    };
    let text = String::from_utf8_lossy(&raw);
    let lines: Vec<String> = text.lines().map(strip_ansi).collect();
    let start = lines.len().saturating_sub(max_lines);
    lines[start..].to_vec()
}

/// Reporte sanitario listo para compartir en WhatsApp, Discord o GitHub (sin rutas privadas).
pub fn build_diagnostic_report(
    hw: &HardwareInfo,
    cfg: &LauncherConfig,
    cache: &CacheStatus,
    metrics: &RunMetrics,
    recent_logs: &[String],
) -> String {
    let mut r = String::new();
    r.push_str(&format!(
        "### Reporte de Diagnóstico · VIctory-RTX-Mc3yohan (v{} @ {})\n",
        env!("CARGO_PKG_VERSION"),
        env!("VICTORY_GIT_SHA")
    ));
    r.push_str(&format!(
        "- **Tarjeta Gráfica**: {} ({} MiB VRAM total, {} MiB en uso, {}% carga · Tier `{}`)\n",
        hw.gpu_name,
        hw.vram_total_mb,
        hw.vram_used_mb,
        hw.gpu_util_pct,
        hw.vram_tier.label(),
    ));
    r.push_str(&format!(
        "- **Memoria del PC**: {:.1} GB RAM ({:.1} GB libre · {:.1} GB virtual libre · ahorro RAM auto: {}) · {} hilos CPU\n",
        hw.ram_total_gb,
        hw.ram_free_gb,
        hw.commit_free_gb,
        if hw.low_ram_auto() { "sí" } else { "no" },
        hw.logical_cpus,
    ));
    r.push_str(&format!(
        "- **Perfil**: `{}` · Consola `{}` · Salida `{}` · ReBAR `{} MiB` · MipDrop `{}` · PortRT `{}`\n",
        cfg.preset.label_es(),
        cfg.effective_console(hw),
        cfg.effective_output(hw).label_es(),
        cfg.effective_windows_bar_mb(hw),
        cfg.effective_mip_drop(hw),
        cfg.port_rt,
    ));
    r.push_str(&format!(
        "- **Caché**: {} shaders ({} MiB) · Biblioteca de escenas: {} ({} MiB) · FSR DLL: {}\n",
        cache.shader_files,
        cache.shader_mb,
        if cache.pipeline_lib_exists { "lista" } else { "pendiente" },
        cache.pipeline_lib_mb,
        if cache.ffx_dll_exists { "presente" } else { "no instalada" },
    ));
    if let Some(med) = metrics.median_fps() {
        r.push_str(&format!(
            "- **Rendimiento medido**: {:.1} FPS mediana ({:.1} mín / {:.1} máx), último {:.1} ms/frame\n",
            med,
            metrics.worst_fps,
            metrics.best_fps,
            metrics.last_ms.unwrap_or(0.0),
        ));
    }
    let tail_lines: Vec<String> = if !recent_logs.is_empty() {
        let start = recent_logs.len().saturating_sub(60);
        recent_logs[start..].to_vec()
    } else if let Some(latest) = scan_data_logs().first() {
        read_log_tail(&latest.path, 60)
    } else {
        Vec::new()
    };

    if !tail_lines.is_empty() {
        r.push_str("\n```text\n");
        for line in &tail_lines {
            r.push_str(&sanitize_line_with_dir(line, &cfg.game_dir));
            r.push('\n');
        }
        r.push_str("```\n");
    }
    r
}

/// Guarda el reporte sanitario en UTF-8 directamente en el Escritorio del usuario.
pub fn save_report_to_desktop(
    hw: &HardwareInfo,
    cfg: &LauncherConfig,
    cache: &CacheStatus,
    metrics: &RunMetrics,
    recent_logs: &[String],
) -> Result<PathBuf, String> {
    let desktop = std::env::var_os("USERPROFILE")
        .map(|p| PathBuf::from(p).join("Desktop"))
        .filter(|p| p.is_dir())
        .unwrap_or_else(data_dir);
    let path = desktop.join(format!("Reporte-VIctory-RTX-{}.txt", timestamp_compact()));
    let body = build_diagnostic_report(hw, cfg, cache, metrics, recent_logs);
    std::fs::write(&path, body.as_bytes())
        .map_err(|e| format!("No se pudo guardar en {}: {e}", path.display()))?;
    Ok(path)
}

fn replace_ascii_case_insensitive(haystack: &str, needle: &str, replacement: &str) -> String {
    if needle.is_empty() || !needle.is_ascii() {
        return haystack.replace(needle, replacement);
    }
    let hay_bytes = haystack.as_bytes();
    let needle_bytes = needle.as_bytes();
    if needle_bytes.len() > hay_bytes.len() {
        return haystack.to_owned();
    }
    let mut out = String::with_capacity(haystack.len());
    let mut i = 0;
    while i + needle_bytes.len() <= hay_bytes.len() {
        if hay_bytes[i..i + needle_bytes.len()].eq_ignore_ascii_case(needle_bytes) {
            out.push_str(replacement);
            i += needle_bytes.len();
        } else {
            let ch = haystack[i..].chars().next().unwrap();
            out.push(ch);
            i += ch.len_utf8();
        }
    }
    out.push_str(&haystack[i..]);
    out
}

fn redact_path_variants(mut s: String, raw_path: &str, placeholder: &str) -> String {
    let trimmed = raw_path.trim().trim_end_matches(['\\', '/']);
    if trimmed.is_empty() {
        return s;
    }
    let backslash = trimmed.replace('/', "\\");
    let escaped = backslash.replace('\\', "\\\\");
    let forward = backslash.replace('\\', "/");
    for variant in [escaped.as_str(), backslash.as_str(), forward.as_str()] {
        s = replace_ascii_case_insensitive(&s, variant, placeholder);
    }
    s
}

fn sanitize_line_with_dir(line: &str, game_dir: &str) -> String {
    let mut s = redact_path_variants(line.to_owned(), game_dir, "<GAME_DIR>");
    if let Ok(profile) = std::env::var("USERPROFILE")
        && !profile.is_empty()
    {
        s = redact_path_variants(s, &profile, "<USUARIO>");
    }
    if let Ok(user) = std::env::var("USERNAME")
        && user.len() >= 3
    {
        for (pat, rep) in [
            (format!("\\\\{user}\\\\"), "\\\\<USUARIO>\\\\"),
            (format!("\\{user}\\"), "\\<USUARIO>\\"),
            (format!("/{user}/"), "/<USUARIO>/"),
        ] {
            s = replace_ascii_case_insensitive(&s, &pat, rep);
        }
    }
    s
}

fn is_victory_already_running() -> bool {
    for img in ["victory.exe", "victory_sm.exe"] {
        let out = Command::new("tasklist")
            .args(["/FI", &format!("IMAGENAME eq {img}"), "/NH"])
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        if out.is_ok_and(|o| String::from_utf8_lossy(&o.stdout).to_ascii_lowercase().contains(img)) {
            return true;
        }
    }
    false
}

pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    let _ = Command::new("explorer").arg(path).spawn();
}

pub fn open_file(path: &Path) {
    let _ = Command::new("cmd")
        .args(["/C", "start", "", &path.to_string_lossy()])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

pub fn open_url(url: &str) {
    let _ = Command::new("cmd")
        .args(["/C", "start", "", url])
        .creation_flags(CREATE_NO_WINDOW)
        .spawn();
}

fn timestamp_compact() -> String {
    let secs = SystemTime::now()
        .duration_since(SystemTime::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn failed_launcher_update_restores_previous_binary() {
        let temp = tempfile::tempdir().unwrap();
        let launcher = temp.path().join("VIctory-RTX.exe");
        std::fs::write(&launcher, "old").unwrap();
        assert!(with_launcher_backup(&launcher, || {
            assert!(!launcher.exists());
            Err("compilación cancelada".to_owned())
        }).is_err());
        assert_eq!(std::fs::read(&launcher).unwrap(), b"old");

        with_launcher_backup(&launcher, || {
            std::fs::write(&launcher, "new").map_err(|e| e.to_string())
        }).unwrap();
        assert_eq!(std::fs::read(&launcher).unwrap(), b"new");
    }

    #[test]
    fn recorded_preparation_is_specific_to_settings_and_engine() {
        let temp = tempfile::tempdir().unwrap();
        let root = temp.path();
        let exe = root.join("engine.exe");
        let title = root.join("title.exe");
        std::fs::write(&exe, "engine").unwrap();
        std::fs::write(&title, "title").unwrap();
        let cfg = LauncherConfig::default();
        let hw = HardwareInfo::probe_fast();
        let mut env = BTreeMap::from([("VICTORY_SHADER_OPT".to_owned(), "191".to_owned())]);
        let stamp = preparation_stamp(root, &exe, &title, &cfg, &hw, &env).unwrap();
        record_preparation(&stamp).unwrap();
        assert!(stamp.is_file());
        assert_eq!(count_files_capped(&root.join("shaders"), 1), 0);
        env.insert("VICTORY_SHADER_OPT".to_owned(), "32".to_owned());
        assert!(!preparation_stamp(root, &exe, &title, &cfg, &hw, &env).unwrap().is_file());
        env.insert("VICTORY_SHADER_OPT".to_owned(), "191".to_owned());
        std::fs::write(&exe, "updated engine").unwrap();
        assert!(!preparation_stamp(root, &exe, &title, &cfg, &hw, &env).unwrap().is_file());
    }

    #[test]
    fn stop_between_stages_prevents_another_spawn() {
        let runner = ProcessRunner::default();
        runner.state.lock().unwrap().start(TaskKind::PrepareAll).unwrap();
        runner.stop();
        let err = run_single_process(
            &runner.state,
            Path::new("unused-when-cancelled.exe"),
            &[],
            &BTreeMap::new(),
            None,
            true,
            None,
            TaskKind::PrepareAll,
        )
        .unwrap_err();
        assert_eq!(err, "Tarea cancelada antes de iniciar el proceso.");
        let mut st = runner.state.lock().unwrap();
        st.finish("Preparación cancelada.".to_owned());
        st.start(TaskKind::PrepareAll).unwrap();
        assert!(!st.cancel_requested);
    }

    #[test]
    fn stop_of_an_active_stage_releases_the_child_and_its_output() {
        let runner = ProcessRunner::default();
        runner.state.lock().unwrap().start(TaskKind::PrepareAll).unwrap();
        let state = runner.state.clone();
        let (tx, rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let mut cmd = Command::new("powershell.exe");
            cmd.args([
                "-NoProfile",
                "-NonInteractive",
                "-Command",
                "$ErrorActionPreference='Stop'; Start-Process powershell.exe -NoNewWindow -ArgumentList '-NoProfile -Command Start-Sleep -Seconds 30'; Write-Output 'runner-ready'; Start-Sleep -Seconds 30",
            ])
            .creation_flags(CREATE_NO_WINDOW);
            let _ = tx.send(run_command(&state, cmd, None, TaskKind::PrepareAll, true));
        });
        let start = Instant::now();
        let ready = loop {
            if runner.snapshot().logs.iter().any(|line| line == "runner-ready") {
                break true;
            }
            if start.elapsed() >= Duration::from_secs(10) {
                break false;
            }
            std::thread::sleep(Duration::from_millis(25));
        };
        runner.stop();
        let exit = rx.recv_timeout(Duration::from_secs(5)).expect("child processes did not stop");
        assert!(ready, "synthetic child never reached its ready signal: {exit:?}");
        assert_eq!(exit.unwrap_err(), "Tarea cancelada en ejecución.");
    }

    #[test]
    fn remote_comparison_requires_two_commit_counts() {
        assert_eq!(parse_sync_counts("0\t17"), Some((0, 17)));
        assert_eq!(parse_sync_counts("3\t0"), Some((3, 0)));
        assert_eq!(parse_sync_counts("0"), None);
    }

    #[test]
    fn imported_file_is_published_once_and_keeps_an_existing_destination() {
        let temp = tempfile::tempdir().unwrap();
        let from = temp.path().join("source");
        let to = temp.path().join("destination");
        std::fs::write(&from, "complete").unwrap();
        copy_new_file(&from, &to).unwrap();
        std::fs::write(&from, "changed").unwrap();
        assert!(copy_new_file(&from, &to).is_err());
        assert_eq!(std::fs::read_to_string(&to).unwrap(), "complete");
        assert_eq!(std::fs::read_dir(temp.path()).unwrap().count(), 2);
    }

    #[test]
    fn diagnostic_lines_redact_the_game_dir() {
        let game_dir = r"D:\Private\Title\";
        assert_eq!(
            sanitize_line_with_dir(
                r#"loading d:\private\title\data and "D:\\Private\\Title\\data" and D:/Private/Title/cfg"#,
                game_dir
            ),
            r#"loading <GAME_DIR>\data and "<GAME_DIR>\\data" and <GAME_DIR>/cfg"#
        );
        assert_eq!(sanitize_line_with_dir("loading data", ""), "loading data");
    }

    #[test]
    fn ansi_stripping_removes_color_sequences() {
        assert_eq!(strip_ansi("\x1b[32mINFO\x1b[0m hola"), "INFO hola");
    }

    #[test]
    fn metrics_parser_extracts_fps_ms_and_draws() {
        let mut m = RunMetrics::default();
        m.observe_line("  26.4 fps, 37.9 ms/frame (prep 18.2 ms), 3412 draws/frame");
        assert_eq!(m.last_fps, Some(26.4));
        assert_eq!(m.last_ms, Some(37.9));
        assert_eq!(m.last_draws, Some(3412));
    }

    #[test]
    fn progress_parser_reads_shaders_and_warm_counts() {
        let mut p = PrepProgress::default();
        p.observe_line("14200 shaders in the cache, 64 s");
        assert_eq!(p.done, 14200);
        assert!(p.fraction() > 0.5);

        p.observe_line("350 of 1686, 48 s");
        assert_eq!(p.done, 350);
        assert_eq!(p.total, 1686);
    }
}
