//! Runtime binary discovery, cache inspection, background child-process runner
//! (Build Shaders, Warm Pipelines, Play Game), ANSI stripping, live FPS/progress
//! parsing, and sanitized diagnostic report generation.

use std::collections::{BTreeMap, VecDeque};
use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::time::{Instant, SystemTime, UNIX_EPOCH};

use crate::config::{LauncherConfig, cache_dir, data_dir};
use crate::hardware::HardwareInfo;

const MAX_LOG_LINES: usize = 6000;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TaskKind {
    Shaders,
    Warm,
    PrepareAll,
    Play,
}

impl TaskKind {
    pub fn label(self) -> &'static str {
        match self {
            Self::Shaders => "Building shaders",
            Self::Warm => "Warming pipelines",
            Self::PrepareAll => "Preparing shaders & pipelines",
            Self::Play => "Game running",
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
        // Parse lines like: `25.7 fps, 38.9 ms/frame (...), 3412 draws/frame`
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
        // Parse `victory shaders` progress lines like:
        // `25200 shaders in the cache, 126 s` or `12000 / 26084` or `translated 25784`
        let trimmed = line.trim();
        if let Some(idx) = trimmed.find(" shaders in the cache") {
            let num_str = trimmed[..idx].split_whitespace().last().unwrap_or("");
            if let Ok(n) = num_str.parse::<u64>() {
                self.done = n;
                if self.total < n {
                    self.total = 26_084.max(n);
                }
                self.step_label = format!("Translating Wave32 shaders ({n} / {})", self.total);
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
        if trimmed.contains("pipelines") && (trimmed.contains("warmed") || trimmed.contains("library")) {
            self.last_summary = trimmed.to_owned();
        }
        // Generic `done/total` token scan for progress lines.
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
pub struct CacheStatus {
    pub shader_files: usize,
    pub shader_mb: u64,
    pub pipeline_lib_mb: u64,
    pub pipeline_lib_exists: bool,
    pub ffx_dll_exists: bool,
}

impl CacheStatus {
    pub fn scan() -> Self {
        let root = cache_dir();
        let shaders_dir = root.join("shaders");
        let (mut shader_files, mut shader_bytes) = (0usize, 0u64);
        if let Ok(entries) = std::fs::read_dir(&shaders_dir) {
            for entry in entries.flatten() {
                if let Ok(meta) = entry.metadata()
                    && meta.is_file()
                {
                    shader_files += 1;
                    shader_bytes += meta.len();
                }
            }
        }
        let lib_path = root.join("host-pipelines.bin");
        let (pipeline_lib_exists, pipeline_lib_mb) = match std::fs::metadata(&lib_path) {
            Ok(meta) if meta.is_file() && meta.len() > 0 => (true, meta.len() / (1024 * 1024)),
            _ => (false, 0),
        };
        let ffx_dll = crate::config::local_app_data()
            .join("victory")
            .join("fidelityfx")
            .join("amd_fidelityfx_upscaler_dx12.dll");
        Self {
            shader_files,
            shader_mb: shader_bytes / (1024 * 1024),
            pipeline_lib_mb,
            pipeline_lib_exists,
            ffx_dll_exists: ffx_dll.is_file(),
        }
    }

    pub fn shaders_ready(&self) -> bool {
        self.shader_files >= 100
    }
}

#[derive(Clone, Debug)]
pub struct EnginePaths {
    pub repo_root: Option<PathBuf>,
    pub release_exe: Option<PathBuf>,
    pub diag_exe: Option<PathBuf>,
}

impl EnginePaths {
    pub fn discover() -> Self {
        let exe_dir = std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf));
        let cwd = std::env::current_dir().ok();

        // Find repo root by walking up from exe_dir or cwd looking for `.victory-root` or `Cargo.toml`.
        let mut repo_root = None;
        for start in exe_dir.iter().chain(cwd.iter()) {
            let mut cur = Some(start.as_path());
            while let Some(dir) = cur {
                if dir.join(".victory-root").is_file()
                    || (dir.join("Cargo.toml").is_file() && dir.join("crates").is_dir())
                {
                    repo_root = Some(dir.to_path_buf());
                    break;
                }
                cur = dir.parent();
            }
            if repo_root.is_some() {
                break;
            }
        }

        let mut release_candidates = Vec::new();
        let mut diag_candidates = Vec::new();
        if let Some(d) = &exe_dir {
            release_candidates.push(d.join("victory.exe"));
            release_candidates.push(d.join("runtime").join("victory.exe"));
            diag_candidates.push(d.join("victory-diag.exe"));
            diag_candidates.push(d.join("..").join("diag").join("victory.exe"));
        }
        if let Some(root) = &repo_root {
            release_candidates.push(root.join("target").join("release").join("victory.exe"));
            diag_candidates.push(root.join("target").join("diag").join("victory.exe"));
        }

        let release_exe = release_candidates.into_iter().find(|p| p.is_file());
        let diag_exe = diag_candidates.into_iter().find(|p| p.is_file());
        Self {
            repo_root,
            release_exe,
            diag_exe,
        }
    }

    pub fn active_exe(&self, want_diag: bool) -> Option<&PathBuf> {
        if want_diag {
            self.diag_exe.as_ref().or(self.release_exe.as_ref())
        } else {
            self.release_exe.as_ref().or(self.diag_exe.as_ref())
        }
    }
}

struct SharedState {
    running: Option<TaskKind>,
    started_at: Option<Instant>,
    child: Option<Arc<Mutex<Child>>>,
    logs: VecDeque<String>,
    total_lines: usize,
    progress: PrepProgress,
    metrics: RunMetrics,
    last_exit_note: Option<String>,
    finished_seq: u64,
}

impl Default for SharedState {
    fn default() -> Self {
        Self {
            running: None,
            started_at: None,
            child: None,
            logs: VecDeque::with_capacity(512),
            total_lines: 0,
            progress: PrepProgress::default(),
            metrics: RunMetrics::default(),
            last_exit_note: None,
            finished_seq: 0,
        }
    }
}

pub struct ProcessRunner {
    state: Arc<Mutex<SharedState>>,
}

impl ProcessRunner {
    pub fn new() -> Self {
        Self {
            state: Arc::new(Mutex::new(SharedState::default())),
        }
    }

    pub fn running(&self) -> Option<TaskKind> {
        self.state.lock().unwrap_or_else(|e| e.into_inner()).running
    }

    pub fn elapsed_secs(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .started_at
            .map_or(0, |t| t.elapsed().as_secs())
    }

    pub fn finished_seq(&self) -> u64 {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .finished_seq
    }

    pub fn snapshot_progress(&self) -> PrepProgress {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .progress
            .clone()
    }

    pub fn snapshot_metrics(&self) -> RunMetrics {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .metrics
            .clone()
    }

    pub fn last_exit_note(&self) -> Option<String> {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .last_exit_note
            .clone()
    }

    pub fn total_lines(&self) -> usize {
        self.state
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .total_lines
    }

    pub fn logs_filtered(&self, filter: &str) -> Vec<String> {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        let f = filter.trim().to_ascii_lowercase();
        if f.is_empty() {
            st.logs.iter().cloned().collect()
        } else {
            st.logs
                .iter()
                .filter(|line| line.to_ascii_lowercase().contains(&f))
                .cloned()
                .collect()
        }
    }

    pub fn all_logs_text(&self) -> String {
        let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.logs.iter().cloned().collect::<Vec<_>>().join("\n")
    }

    pub fn clear_logs(&self) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        st.logs.clear();
        st.total_lines = 0;
    }

    pub fn push_note(&self, msg: impl Into<String>) {
        let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
        push_log_locked(&mut st, msg.into());
    }

    pub fn stop(&self) {
        let child_arc = {
            let st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            st.child.clone()
        };
        if let Some(child) = child_arc {
            if let Ok(mut guard) = child.lock() {
                let _ = guard.kill();
            }
            self.push_note("[launcher] Stop requested by user.");
        }
    }

    pub fn start_play(
        &self,
        engine: &EnginePaths,
        cfg: &LauncherConfig,
        hw: &HardwareInfo,
        title_exe: &Path,
    ) -> Result<(), String> {
        let want_diag = cfg.diag_build || cfg.gpu_timing;
        let base_victory = engine
            .active_exe(want_diag)
            .ok_or_else(|| "victory.exe not found. Build with `cargo build --locked --release -p victory`.".to_owned())?
            .clone();

        let victory_exe = if cfg.smooth_motion {
            let sm_copy = base_victory
                .parent()
                .unwrap_or_else(|| Path::new("."))
                .join("victory_sm.exe");
            std::fs::copy(&base_victory, &sm_copy)
                .map_err(|e| format!("Could not create victory_sm.exe for Smooth Motion: {e}"))?;
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
        let stamp = unix_timestamp_string();
        let log_path = log_dir.join(format!("nvidia-run-{stamp}.log"));

        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.running.is_some() {
                return Err("Another task is already running.".to_owned());
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
                    "[launcher] Starting VIctory-RTX-Mc3yohan ({}, {}, {} switches) -> {}",
                    if want_diag { "diag" } else { "release" },
                    console,
                    env.len(),
                    log_path.display()
                ),
            );
        }

        let work_dir = engine
            .repo_root
            .clone()
            .or_else(|| victory_exe.parent().map(Path::to_path_buf));
        let state = self.state.clone();
        std::thread::spawn(move || {
            let log_file = OpenOptions::new()
                .create(true)
                .write(true)
                .truncate(true)
                .open(&log_path)
                .ok();
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
            let note = match code {
                Ok(0) => "Game exited cleanly.".to_owned(),
                Ok(c) => format!("Game exited with code {c}."),
                Err(e) => format!("Failed to start game: {e}"),
            };
            push_log_locked(&mut st, format!("[launcher] {note}"));
            st.last_exit_note = Some(note);
        });
        Ok(())
    }

    pub fn start_prep(
        &self,
        kind: TaskKind,
        engine: &EnginePaths,
        cfg: &LauncherConfig,
        jobs: usize,
    ) -> Result<(), String> {
        if cfg.game_dir.is_empty() && matches!(kind, TaskKind::Shaders | TaskKind::PrepareAll) {
            return Err("Select your game folder first.".to_owned());
        }
        let victory_exe = engine
            .active_exe(false)
            .ok_or_else(|| "victory.exe not found.".to_owned())?
            .clone();
        let env = cfg.build_prep_env();
        let game_dir = cfg.game_dir.clone();
        let jobs = jobs.clamp(1, 32);

        {
            let mut st = self.state.lock().unwrap_or_else(|e| e.into_inner());
            if st.running.is_some() {
                return Err("Another task is already running.".to_owned());
            }
            st.running = Some(kind);
            st.started_at = Some(Instant::now());
            st.last_exit_note = None;
            st.progress = PrepProgress {
                step_label: kind.label().to_owned(),
                done: 0,
                total: if matches!(kind, TaskKind::Warm) { 0 } else { 26_084 },
                unsupported: 0,
                last_summary: String::new(),
            };
            push_log_locked(
                &mut st,
                format!("[launcher] {} using {} threads (exact NVIDIA preset keys)...", kind.label(), jobs),
            );
        }

        let work_dir = engine
            .repo_root
            .clone()
            .or_else(|| victory_exe.parent().map(Path::to_path_buf));
        let state = self.state.clone();
        std::thread::spawn(move || {
            let mut steps: Vec<(TaskKind, Vec<String>)> = Vec::new();
            if matches!(kind, TaskKind::Shaders | TaskKind::PrepareAll) {
                steps.push((
                    TaskKind::Shaders,
                    vec![
                        "shaders".into(),
                        "--nvidia-preset".into(),
                        "--jobs".into(),
                        jobs.to_string(),
                        game_dir.clone(),
                    ],
                ));
            }
            if matches!(kind, TaskKind::Warm | TaskKind::PrepareAll) {
                steps.push((
                    TaskKind::Warm,
                    vec![
                        "warm".into(),
                        "--nvidia-preset".into(),
                        "--used".into(),
                        "--jobs".into(),
                        jobs.to_string(),
                        "--save-library".into(),
                    ],
                ));
            }

            let mut final_res: Result<i32, String> = Ok(0);
            for (step_kind, args) in steps {
                {
                    let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                    st.progress.step_label = step_kind.label().to_owned();
                }
                let res = run_single_process(
                    &state,
                    &victory_exe,
                    &args,
                    &env,
                    work_dir.as_deref(),
                    true,
                    None,
                    step_kind,
                );
                match res {
                    Ok(0) => {}
                    other => {
                        final_res = other;
                        break;
                    }
                }
            }

            let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
            let elapsed = st.started_at.map_or(0, |t| t.elapsed().as_secs());
            st.running = None;
            st.child = None;
            st.finished_seq += 1;
            let note = match final_res {
                Ok(0) => {
                    if st.progress.total > 0 {
                        st.progress.done = st.progress.total;
                    }
                    format!("{} finished in {} s.", kind.label(), elapsed)
                }
                Ok(c) => format!("{} exited with code {c} after {elapsed} s.", kind.label()),
                Err(e) => format!("{} failed: {e}", kind.label()),
            };
            push_log_locked(&mut st, format!("[launcher] {note}"));
            st.last_exit_note = Some(note);
        });
        Ok(())
    }
}

#[allow(clippy::too_many_arguments)]
fn run_single_process(
    state: &Arc<Mutex<SharedState>>,
    exe: &Path,
    args: &[String],
    env: &BTreeMap<String, String>,
    work_dir: Option<&Path>,
    no_window: bool,
    log_file: Option<File>,
    kind: TaskKind,
) -> Result<i32, String> {
    let mut cmd = Command::new(exe);
    cmd.args(args)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped());
    if let Some(dir) = work_dir {
        cmd.current_dir(dir);
    }
    // Clear stale VICTORY_* vars except path overrides before applying clean preset env.
    for (k, _) in std::env::vars() {
        if k.starts_with("VICTORY_")
            && !matches!(
                k.as_str(),
                "VICTORY_CONFIG"
                    | "VICTORY_GAME_DIR"
                    | "VICTORY_CACHE_DIR"
                    | "VICTORY_DATA_DIR"
                    | "VICTORY_FIDELITYFX_DIR"
            )
        {
            cmd.env_remove(&k);
        }
    }
    for (k, v) in env {
        cmd.env(k, v);
    }
    #[cfg(windows)]
    if no_window {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }

    let mut child = cmd.spawn().map_err(|e| e.to_string())?;
    let stdout = child.stdout.take();
    let stderr = child.stderr.take();
    let child_arc = Arc::new(Mutex::new(child));
    {
        let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
        st.child = Some(child_arc.clone());
    }

    let file_arc = log_file.map(|f| Arc::new(Mutex::new(f)));

    let spawn_reader = |stream: Box<dyn std::io::Read + Send>,
                        state: Arc<Mutex<SharedState>>,
                        file_arc: Option<Arc<Mutex<File>>>| {
        std::thread::spawn(move || {
            let reader = BufReader::new(stream);
            for line in reader.lines().map_while(Result::ok) {
                let clean = strip_ansi(&line);
                if let Some(f) = &file_arc
                    && let Ok(mut guard) = f.lock()
                {
                    let _ = writeln!(guard, "{clean}");
                }
                let mut st = state.lock().unwrap_or_else(|e| e.into_inner());
                if kind == TaskKind::Play {
                    st.metrics.observe_line(&clean);
                } else {
                    st.progress.observe_line(&clean);
                }
                push_log_locked(&mut st, clean);
            }
        })
    };

    let t_out = stdout.map(|s| spawn_reader(Box::new(s), state.clone(), file_arc.clone()));
    let t_err = stderr.map(|s| spawn_reader(Box::new(s), state.clone(), file_arc));

    let code = loop {
        {
            let mut guard = child_arc.lock().unwrap_or_else(|e| e.into_inner());
            if let Ok(Some(status)) = guard.try_wait() {
                break status.code().unwrap_or(-1);
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(80));
    };

    if let Some(t) = t_out {
        let _ = t.join();
    }
    if let Some(t) = t_err {
        let _ = t.join();
    }
    Ok(code)
}

fn push_log_locked(st: &mut SharedState, line: String) {
    st.total_lines += 1;
    if st.logs.len() >= MAX_LOG_LINES {
        st.logs.pop_front();
    }
    st.logs.push_back(line);
}

/// Strip VT/ANSI SGR escape codes so logs are clean plain text in the GUI and files.
pub fn strip_ansi(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut chars = input.chars().peekable();
    while let Some(c) = chars.next() {
        if c == '\x1b' && chars.peek() == Some(&'[') {
            let _ = chars.next();
            for next in chars.by_ref() {
                if ('@'..='~').contains(&next) {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

/// Recent crash/run logs found inside `%LOCALAPPDATA%\victory\data`.
#[derive(Clone, Debug)]
pub struct LogFileEntry {
    pub name: String,
    pub path: PathBuf,
    pub size_kb: u64,
    pub is_crash: bool,
}

pub fn scan_data_logs() -> Vec<LogFileEntry> {
    let dir = data_dir();
    let mut out = Vec::new();
    let Ok(entries) = std::fs::read_dir(&dir) else {
        return Vec::new();
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let lower = name.to_ascii_lowercase();
        let is_crash = lower.contains("crash");
        if is_crash || lower.ends_with(".log") {
            let meta = entry.metadata().ok();
            let size_kb = meta.as_ref().map_or(0, |m| m.len().div_ceil(1024));
            let modified = meta
                .and_then(|m| m.modified().ok())
                .unwrap_or(UNIX_EPOCH);
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
    out.into_iter().take(20).map(|(_, e)| e).collect()
}

/// Build a sanitized Markdown report safe to paste into GitHub issues (strips user profile paths).
pub fn build_diagnostic_report(
    hw: &HardwareInfo,
    cfg: &LauncherConfig,
    cache: &CacheStatus,
    metrics: &RunMetrics,
    recent_logs: &[String],
) -> String {
    let mut r = String::new();
    r.push_str("### VIctory-RTX-Mc3yohan Diagnostic Report\n");
    r.push_str(&format!(
        "- **GPU**: {} ({} MiB total, {} MiB used, {}% util, Tier `{}`)\n",
        hw.gpu_name,
        hw.vram_total_mb,
        hw.vram_used_mb,
        hw.gpu_util_pct,
        hw.vram_tier.label(),
    ));
    r.push_str(&format!(
        "- **System RAM**: {:.1} GB total ({:.1} GB free RAM, {:.1} GB free commit) · {} CPU threads\n",
        hw.ram_total_gb,
        hw.ram_free_gb,
        hw.commit_free_gb,
        hw.logical_cpus,
    ));
    r.push_str(&format!("- **Preset**: `{}` · Console `{}` · Output `{}` · ReBAR `{} MiB` · MipDrop `{}`\n",
        cfg.preset.label(),
        cfg.effective_console(hw),
        cfg.output.label(),
        cfg.effective_windows_bar_mb(hw),
        cfg.effective_mip_drop(hw),
    ));
    r.push_str(&format!(
        "- **Cache**: {} shaders ({} MiB), pipeline library {} ({} MiB), FSR DLL {}\n",
        cache.shader_files,
        cache.shader_mb,
        if cache.pipeline_lib_exists { "present" } else { "missing" },
        cache.pipeline_lib_mb,
        if cache.ffx_dll_exists { "present" } else { "missing" },
    ));
    if let Some(med) = metrics.median_fps() {
        r.push_str(&format!(
            "- **Measured FPS**: {:.1} median ({:.1}..{:.1} min/max), last {:.1} ms/frame\n",
            med,
            metrics.worst_fps,
            metrics.best_fps,
            metrics.last_ms.unwrap_or(0.0),
        ));
    }
    if !recent_logs.is_empty() {
        r.push_str("\n```text\n");
        let start = recent_logs.len().saturating_sub(35);
        for line in &recent_logs[start..] {
            r.push_str(&sanitize_line(line));
            r.push('\n');
        }
        r.push_str("```\n");
    }
    r
}

pub fn sanitize_line(line: &str) -> String {
    let mut s = line.to_owned();
    if let Ok(profile) = std::env::var("USERPROFILE")
        && !profile.is_empty()
    {
        s = s.replace(&profile, "<USER>");
    }
    if let Ok(user) = std::env::var("USERNAME")
        && user.len() >= 3
    {
        s = s.replace(&format!("\\{user}\\"), "\\<USER>\\");
    }
    s
}

pub fn open_folder(path: &Path) {
    let _ = std::fs::create_dir_all(path);
    #[cfg(windows)]
    {
        let _ = Command::new("explorer").arg(path).spawn();
    }
}

pub fn open_url(url: &str) {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        let _ = Command::new("cmd")
            .args(["/C", "start", "", url])
            .creation_flags(0x0800_0000)
            .spawn();
    }
}

fn unix_timestamp_string() -> String {
    let secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |d| d.as_secs());
    format!("{secs}")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ansi_stripping_removes_color_sequences() {
        let raw = "\x1b[32mINFO\x1b[0m 27.4 fps, 36.5 ms/frame";
        assert_eq!(strip_ansi(raw), "INFO 27.4 fps, 36.5 ms/frame");
    }

    #[test]
    fn metrics_parser_extracts_fps_ms_and_draws() {
        let mut m = RunMetrics::default();
        m.observe_line("2026-11-26 INFO 28.4 fps, 35.2 ms/frame (12.1 preparing), 3410 draws/frame");
        m.observe_line("2026-11-26 INFO 24.0 fps, 41.6 ms/frame (14.0 preparing), 5800 draws/frame");
        assert_eq!(m.last_fps, Some(24.0));
        assert_eq!(m.last_ms, Some(41.6));
        assert_eq!(m.last_draws, Some(5800));
        assert_eq!(m.best_fps, 28.4);
        assert_eq!(m.worst_fps, 24.0);
    }

    #[test]
    fn progress_parser_reads_shader_counts() {
        let mut p = PrepProgress::default();
        p.observe_line("  25200 shaders in the cache, 126 s");
        assert_eq!(p.done, 25_200);
        assert!(p.fraction() > 0.95);
    }
}
