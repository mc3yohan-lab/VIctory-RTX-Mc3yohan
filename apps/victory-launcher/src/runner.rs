//! Gestión de procesos (`victory.exe`), detección recursiva de caché +
//! importación 1-clic desde caché ScarlettPort, actualización Git en vivo,
//! captura de logs UTF-8 y exportación directa al Escritorio.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::os::windows::process::CommandExt;
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime};

use crate::config::{GameFilesStatus, LauncherConfig, local_app_data};
use crate::hardware::HardwareInfo;

const CREATE_NO_WINDOW: u32 = 0x0800_0000;
const MAX_LOG_LINES: usize = 3000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Play,
    Warm,
    PrepareAll,
    ImportCache,
    GitUpdate,
}

impl TaskKind {
    pub fn label_es(self) -> &'static str {
        match self {
            Self::Play => "Juego en ejecución",
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
            self.step_label = "Guardando biblioteca de escenas (host-pipelines.bin)...".to_owned();
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
    pub pipeline_lib_mb: u64,
    pub pipeline_lib_exists: bool,
    pub early_pipelines_count: usize,
    pub ffx_dll_exists: bool,
    pub legacy_importable: Option<LegacyCacheInfo>,
}

impl CacheStatus {
    pub fn scan() -> Self {
        let root = cache_dir();
        let (shader_files, shader_bytes) = count_files_recursive(&root.join("shaders"));

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

        // Si falta caché en `%LOCALAPPDATA%\victory`, buscar caché previa en ScarlettPort
        // (ScarlettPort v0.4.7+ renombra `%LOCALAPPDATA%\victory` -> `ScarlettPort`).
        let legacy_importable = if shader_files == 0 || !pipeline_lib_exists {
            detect_legacy_scarlett_cache()
        } else {
            None
        };

        Self {
            shader_files,
            shader_mb: shader_bytes / (1024 * 1024),
            pipeline_lib_mb,
            pipeline_lib_exists,
            early_pipelines_count,
            ffx_dll_exists: ffx_dll.is_file(),
            legacy_importable,
        }
    }

    pub fn shaders_ready(&self) -> bool {
        self.shader_files >= 500
    }
}

fn detect_legacy_scarlett_cache() -> Option<LegacyCacheInfo> {
    let sp_root = local_app_data().join("ScarlettPort");
    if !sp_root.is_dir() {
        return None;
    }
    let (shader_files, _) = count_files_recursive(&sp_root.join("cache").join("shaders"));
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

fn count_files_recursive(dir: &Path) -> (usize, u64) {
    let mut files = 0usize;
    let mut bytes = 0u64;
    let Ok(entries) = std::fs::read_dir(dir) else {
        return (0, 0);
    };
    let mut stack: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
    while let Some(path) = stack.pop() {
        if let Ok(meta) = std::fs::metadata(&path) {
            if meta.is_file() {
                files += 1;
                bytes += meta.len();
            } else if meta.is_dir()
                && let Ok(sub) = std::fs::read_dir(&path)
            {
                for e in sub.flatten() {
                    stack.push(e.path());
                }
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
    pub checked_once: bool,
    pub last_status_es: String,
}

impl GitInfo {
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
            .unwrap_or_else(|| "nvidia-rtx4090".to_owned());
        let local_sha = git_stdout(root, &["rev-parse", "--short", "HEAD"])
            .unwrap_or_else(|| env!("VICTORY_GIT_SHA").to_owned());
        Self {
            available: true,
            branch,
            local_sha,
            behind_count: 0,
            pending_commits: Vec::new(),
            checked_once: false,
            last_status_es: "Pulsa 'Buscar actualizaciones' para comprobar mejoras nuevas en GitHub."
                .to_owned(),
        }
    }

    pub fn fetch_remote_status(repo_root: &Path) -> Self {
        let mut info = Self::probe_local(Some(repo_root));
        if !info.available {
            return info;
        }
        let fetch = Command::new("git")
            .args(["fetch", "origin", "--quiet"])
            .current_dir(repo_root)
            .creation_flags(CREATE_NO_WINDOW)
            .output();
        match fetch {
            Ok(o) if o.status.success() => {
                let range = format!("HEAD..origin/{}", info.branch);
                let commits = git_stdout(repo_root, &["log", "--oneline", &range]).unwrap_or_default();
                let list: Vec<String> = commits
                    .lines()
                    .map(str::trim)
                    .filter(|l| !l.is_empty())
                    .map(ToOwned::to_owned)
                    .collect();
                info.behind_count = list.len();
                info.pending_commits = list;
                info.checked_once = true;
                info.last_status_es = if info.behind_count == 0 {
                    "Estás completamente al día con la última versión.".to_owned()
                } else {
                    format!(
                        "Hay {} mejora(s) nueva(s) lista(s) para descargar y compilar.",
                        info.behind_count
                    )
                };
            }
            Ok(o) => {
                let err = String::from_utf8_lossy(&o.stderr);
                info.checked_once = true;
                info.last_status_es = format!(
                    "No se pudo conectar con GitHub: {}",
                    err.lines().next().unwrap_or("revisa tu conexión o sesión de Git")
                );
            }
            Err(e) => {
                info.checked_once = true;
                info.last_status_es = format!("Git no encontrado en este PC ({e}).");
            }
        }
        info
    }
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
    child: Option<Arc<Mutex<Child>>>,
    logs: VecDeque<String>,
    metrics: RunMetrics,
    progress: PrepProgress,
    last_exit_note: Option<String>,
    finished_seq: u64,
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
        let child = {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            push_log_locked(
                &mut st,
                "[launcher] Deteniendo tarea a petición del usuario...".to_owned(),
            );
            st.child.take()
        };
        if let Some(c) = child
            && let Ok(mut guard) = c.lock()
        {
            let _ = guard.kill();
        }
    }

    pub fn start_play(
        &self,
        engine: &EnginePaths,
        cfg: &LauncherConfig,
        hw: &HardwareInfo,
    ) -> Result<(), String> {
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

        let env = cfg.build_run_env(hw);
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
        let stamp = timestamp_compact();
        let log_path = log_dir.join(format!("nvidia-run-{stamp}.log"));

        // Encabezado limpio UTF-8 para que cualquier log compartido tenga toda la info del PC.
        let header = format!(
            "=== VIctory-RTX-Mc3yohan (Quantico Launcher v0.7.0 @ {}) ===\n\
             GPU      : {} ({} MiB VRAM, Tier {})\n\
             RAM      : {:.1} GB total ({:.1} GB libre, {:.1} GB commit libre) | {} hilos\n\
             Perfil   : {} | Consola: {} | Salida: {} | Escala: {:?}\n\
             Build    : {} | PortRT={} | Thin={} | LiveTex={} | VideoPlanes={}\n\
             ==============================================================\n",
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
            if want_diag { "diag" } else { "release" },
            cfg.port_rt,
            cfg.effective_thin(hw),
            cfg.live_textures,
            cfg.video_planes,
        );

        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.running.is_some() {
                return Err("Ya hay otra tarea ejecutándose.".to_owned());
            }
            st.running = Some(TaskKind::Play);
            st.started_at = Some(Instant::now());
            st.last_exit_note = None;
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
        std::thread::spawn(move || {
            let mut log_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&log_path)
                .ok();
            if let Some(f) = log_file.as_mut() {
                let _ = f.write_all(header.as_bytes());
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
            st.running = None;
            st.child = None;
            st.finished_seq += 1;
            st.last_exit_note = Some(match code {
                Ok(0) => "Partida cerrada correctamente.".to_owned(),
                Ok(c) => format!("El juego terminó con código {c}. Revisa 'Logs y Soporte' si hubo un error."),
                Err(e) => format!("No se pudo iniciar el motor: {e}"),
            });
        });

        Ok(())
    }

    pub fn start_prep(
        &self,
        engine: &EnginePaths,
        cfg: &LauncherConfig,
        kind: TaskKind,
    ) -> Result<(), String> {
        let victory_exe = engine
            .active_victory(false)
            .ok_or_else(|| "No se encontró victory.exe.".to_owned())?
            .clone();
        let game = GameFilesStatus::inspect(Path::new(&cfg.game_dir));
        if !game.ready() && matches!(kind, TaskKind::PrepareAll) {
            return Err("Primero selecciona la carpeta válida del juego.".to_owned());
        }
        let env = cfg.build_prep_env();
        let game_dir = cfg.game_dir.clone();
        let work_dir = engine
            .repo_root
            .clone()
            .or_else(|| victory_exe.parent().map(Path::to_path_buf));

        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.running.is_some() {
                return Err("Ya hay otra tarea ejecutándose.".to_owned());
            }
            st.running = Some(kind);
            st.started_at = Some(Instant::now());
            st.last_exit_note = None;
            st.progress = PrepProgress {
                step_label: kind.label_es().to_owned(),
                ..Default::default()
            };
            push_log_locked(&mut st, format!("[launcher] Iniciando: {}...", kind.label_es()));
        }

        let state = self.state.clone();
        std::thread::spawn(move || {
            let result = match kind {
                TaskKind::Warm => {
                    let args = vec![
                        "warm".to_owned(),
                        "--save-library".to_owned(),
                        "--used".to_owned(),
                        "--nvidia-preset".to_owned(),
                    ];
                    run_single_process(
                        &state,
                        &victory_exe,
                        &args,
                        &env,
                        work_dir.as_deref(),
                        true,
                        None,
                        kind,
                    )
                }
                TaskKind::PrepareAll => {
                    {
                        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                        st.progress.step_label = "Paso 1/2 · Traduciendo shaders Wave32...".to_owned();
                    }
                    let args1 = vec![
                        "shaders".to_owned(),
                        game_dir,
                        "--nvidia-preset".to_owned(),
                    ];
                    match run_single_process(
                        &state,
                        &victory_exe,
                        &args1,
                        &env,
                        work_dir.as_deref(),
                        true,
                        None,
                        kind,
                    ) {
                        Ok(0) => {
                            let early = cache_dir().join("pipelines").join("early-pipelines.txt");
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
                                    "--nvidia-preset".to_owned(),
                                ];
                                run_single_process(
                                    &state,
                                    &victory_exe,
                                    &args2,
                                    &env,
                                    work_dir.as_deref(),
                                    true,
                                    None,
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
            st.running = None;
            st.child = None;
            st.finished_seq += 1;
            st.last_exit_note = Some(match result {
                Ok(0) => format!("{} completado correctamente.", kind.label_es()),
                Ok(c) => format!("{} terminó con código {c}.", kind.label_es()),
                Err(e) => format!("Error en {}: {e}", kind.label_es()),
            });
        });

        Ok(())
    }

    /// Importa en segundo plano caché de shaders, pipelines, FidelityFX y partidas
    /// guardadas desde `%LOCALAPPDATA%\ScarlettPort` hacia `%LOCALAPPDATA%\victory`.
    pub fn start_import_legacy_cache(&self, info: LegacyCacheInfo) -> Result<(), String> {
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.running.is_some() {
                return Err("Ya hay otra tarea ejecutándose.".to_owned());
            }
            st.running = Some(TaskKind::ImportCache);
            st.started_at = Some(Instant::now());
            st.last_exit_note = None;
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
                    st.progress.done = idx as u64;
                    st.progress.step_label = format!("Copiando {label}...");
                }
                if src.is_dir() {
                    let count = copy_dir_non_destructive(&src, &dst, label == "Partidas guardadas");
                    copied_items += count;
                    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                    push_log_locked(
                        &mut st,
                        format!("[launcher] {label}: {count} archivo(s) importado(s)."),
                    );
                }
            }

            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            st.progress.done = 4;
            st.running = None;
            st.finished_seq += 1;
            st.last_exit_note = Some(format!(
                "Importación lista ({copied_items} archivos copiados sin borrar el origen)."
            ));
        });

        Ok(())
    }

    /// Ejecuta `git pull --ff-only` y recompila `victory.exe` en `--release`.
    pub fn start_git_update(&self, repo_root: PathBuf, rebuild_engine: bool) -> Result<(), String> {
        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.running.is_some() {
                return Err("Ya hay otra tarea ejecutándose.".to_owned());
            }
            st.running = Some(TaskKind::GitUpdate);
            st.started_at = Some(Instant::now());
            st.last_exit_note = None;
            st.progress = PrepProgress {
                step_label: "Paso 1/2 · Descargando cambios desde GitHub...".to_owned(),
                done: 1,
                total: if rebuild_engine { 2 } else { 1 },
                ..Default::default()
            };
            push_log_locked(
                &mut st,
                "[actualizar] Ejecutando git pull --ff-only...".to_owned(),
            );
        }

        let state = self.state.clone();
        std::thread::spawn(move || {
            let empty_env = BTreeMap::new();
            let pull_res = run_single_process(
                &state,
                Path::new("git"),
                &["pull".to_owned(), "--ff-only".to_owned()],
                &empty_env,
                Some(&repo_root),
                true,
                None,
                TaskKind::GitUpdate,
            );

            let final_res = match pull_res {
                Ok(0) if rebuild_engine => {
                    {
                        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                        st.progress.done = 2;
                        st.progress.step_label =
                            "Paso 2/2 · Compilando motor actualizado (release)...".to_owned();
                        push_log_locked(
                            &mut st,
                            "[actualizar] Descarga lista. Compilando victory.exe (--release)..."
                                .to_owned(),
                        );
                    }
                    run_single_process(
                        &state,
                        Path::new("cargo"),
                        &[
                            "build".to_owned(),
                            "--locked".to_owned(),
                            "--release".to_owned(),
                            "-p".to_owned(),
                            "victory".to_owned(),
                        ],
                        &empty_env,
                        Some(&repo_root),
                        true,
                        None,
                        TaskKind::GitUpdate,
                    )
                }
                other => other,
            };

            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            st.running = None;
            st.child = None;
            st.finished_seq += 1;
            st.last_exit_note = Some(match final_res {
                Ok(0) => "Actualización y compilación completadas. ¡Ya tienes las mejoras activas!".to_owned(),
                Ok(c) => format!("La actualización terminó con código {c}. Revisa el registro."),
                Err(e) => format!("No se pudo actualizar: {e}"),
            });
        });

        Ok(())
    }
}

fn copy_dir_non_destructive(src: &Path, dst: &Path, skip_logs: bool) -> usize {
    let mut copied = 0usize;
    let _ = std::fs::create_dir_all(dst);
    let Ok(entries) = std::fs::read_dir(src) else {
        return 0;
    };
    for entry in entries.flatten() {
        let from = entry.path();
        let name = entry.file_name();
        if skip_logs {
            let lower = name.to_string_lossy().to_ascii_lowercase();
            if lower.ends_with(".log") {
                continue;
            }
        }
        let to = dst.join(&name);
        if from.is_dir() {
            copied += copy_dir_non_destructive(&from, &to, skip_logs);
        } else if from.is_file() && !to.exists() && std::fs::copy(&from, &to).is_ok() {
            copied += 1;
        }
    }
    copied
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
    cmd.args(args)
        .envs(env)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = work_dir {
        cmd.current_dir(dir);
    }
    if hide_window {
        cmd.creation_flags(CREATE_NO_WINDOW);
    }

    let mut child = cmd
        .spawn()
        .map_err(|e| format!("No se pudo ejecutar {}: {e}", exe.display()))?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child_arc = Arc::new(Mutex::new(child));
    {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.child = Some(child_arc.clone());
    }

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

    let status = loop {
        let exited = {
            let mut guard = child_arc.lock().unwrap_or_else(|e| e.into_inner());
            guard.try_wait()
        };
        match exited {
            Ok(Some(status)) => break status,
            Ok(None) => std::thread::sleep(std::time::Duration::from_millis(80)),
            Err(e) => return Err(e.to_string()),
        }
    };

    if let Some(t) = t_out {
        let _ = t.join();
    }
    if let Some(t) = t_err {
        let _ = t.join();
    }

    Ok(status.code().unwrap_or(-1))
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
                    TaskKind::Play => st.metrics.observe_line(&clean),
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
    out.sort_by(|a, b| b.0.cmp(&a.0));
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
        "### Reporte de Diagnóstico · VIctory-RTX-Mc3yohan (v0.7.0 @ {})\n",
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
        "- **Memoria del PC**: {:.1} GB RAM ({:.1} GB libre · {:.1} GB virtual libre) · {} hilos CPU\n",
        hw.ram_total_gb, hw.ram_free_gb, hw.commit_free_gb, hw.logical_cpus,
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
            r.push_str(&sanitize_line(line));
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

pub fn sanitize_line(line: &str) -> String {
    let mut s = line.to_owned();
    if let Ok(profile) = std::env::var("USERPROFILE")
        && !profile.is_empty()
    {
        s = s.replace(&profile, "<USUARIO>");
    }
    if let Ok(user) = std::env::var("USERNAME")
        && user.len() >= 3
    {
        s = s.replace(&format!("\\{user}\\"), "\\<USUARIO>\\");
    }
    s
}

fn is_victory_already_running() -> bool {
    let out = Command::new("tasklist")
        .args(["/FI", "IMAGENAME eq victory.exe", "/NH"])
        .creation_flags(CREATE_NO_WINDOW)
        .output();
    out.is_ok_and(|o| {
        let s = String::from_utf8_lossy(&o.stdout);
        s.to_ascii_lowercase().contains("victory.exe")
    })
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
