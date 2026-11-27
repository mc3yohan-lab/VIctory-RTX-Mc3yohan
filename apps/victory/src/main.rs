//! `victory`: the command-line front end.
//!
//! Every command reads the user's own files and writes only to stdout or to
//! the user's cache directory (outside the repository).

mod census;
mod inspect;
#[cfg(all(windows, feature = "diagnostics"))]
mod replay;
mod run;
mod shader;
mod translate;
#[cfg(windows)]
mod warm;

use std::path::PathBuf;

use anyhow::Result;
use clap::{Parser, Subcommand, ValueEnum};

#[derive(Parser)]
#[command(
    name = "victory",
    version,
    about = "Run an Xbox Series title natively on Windows"
)]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
#[allow(clippy::large_enum_variant)]
enum Command {
    /// Summarise one PE image: headers, sections, imports, TLS, build id.
    Inspect {
        file: PathBuf,
        /// List every imported function.
        #[arg(long)]
        imports: bool,
        /// List every exported function.
        #[arg(long)]
        exports: bool,
    },
    /// Aggregate the imports of every PE image under the given paths and
    /// show which ones our runtime covers. Defaults to the configured
    /// game and test-title directories.
    Census {
        paths: Vec<PathBuf>,
        /// Do not count host pass-through as covered.
        #[arg(long)]
        strict: bool,
        /// List every missing `dll!function`, most used first.
        #[arg(long)]
        missing: bool,
    },
    /// List the `dll!export` pairs an import library describes.
    LibExports {
        lib: PathBuf,
        /// List every export, not just per-DLL counts.
        #[arg(long)]
        list: bool,
    },
    /// Find shader containers in files or directories (also inside engine
    /// archives, if uncompressed) and report their layout.
    ShaderInfo {
        paths: Vec<PathBuf>,
        /// Print one line per container found and show why paths were skipped.
        #[arg(short, long)]
        verbose: bool,
        /// Also list the decoded native instructions (implies --verbose).
        #[arg(long)]
        disasm: bool,
    },
    /// Run every shader found (containers, SGD5 files) through the
    /// translator and report what it cannot handle yet.
    ///
    /// VICTORY_HOST_WAVE overrides the host wave width, including with --warm.
    /// VICTORY_HOST_MAX_WAVE is a compatibility alias. When both values are
    /// valid, VICTORY_HOST_WAVE takes precedence. Zero means unknown width.
    /// For ordinary translation, VICTORY_HOST_RDNA=1 selects RDNA float settings.
    Translate {
        paths: Vec<PathBuf>,
        /// Check emitted DXIL with dxil.dll's validator (`VICTORY_DXIL`,
        /// else the newest Windows SDK's).
        #[arg(long)]
        validate: bool,
        /// Also create a host D3D12 pipeline for each emitted shader.
        #[arg(long)]
        pipelines: bool,
        /// Print one line per program and outcome.
        #[arg(long)]
        list: bool,
        /// Worker threads (default: every logical core).
        #[arg(long)]
        jobs: Option<usize>,
        /// Warm the run's shader cache: a file of shader hashes (the 16
        /// hex digits a run's log names a shader by, anywhere in a line);
        /// the containers with those hashes are translated into the
        /// cache and nothing else is done. Set `VICTORY_HOST_WAVE` and
        /// `VICTORY_VIEW_LIMIT` to what the run's log says of the host.
        #[arg(long)]
        warm: Option<PathBuf>,
    },
    /// Translate every shader found (as `translate` finds them) into the
    /// shader cache, so that no run has to: a few minutes, once per version
    /// of the translator. Translations an earlier build of the same version
    /// cached are made again, and those that came out different counted.
    /// In diagnostics builds, VICTORY_LOG=debug shows skipped paths.
    Shaders {
        paths: Vec<PathBuf>,
        /// Worker threads (default: every logical core).
        #[arg(long)]
        jobs: Option<usize>,
        /// Also write every shader that does not translate to this file,
        /// one line each: hash, where it was found, why.
        #[arg(long)]
        list: Option<PathBuf>,
        /// Apply the VIctory-RTX-Mc3yohan NVIDIA preset defaults for unset
        /// `VICTORY_*` variables (matches `scripts/play-nvidia.ps1` and
        /// `AmericasLauncher` `runners.json`).
        #[arg(long)]
        nvidia_preset: bool,
    },
    /// Make the host pipelines of every pipeline the title ever created
    /// (runs with `VICTORY_PIPELINE_BOOK=1` write them down), so that the
    /// driver's cache on disk has them before a run needs one: once per change
    /// of the translator or the driver. Goes on where it was stopped.
    #[cfg(windows)]
    Warm {
        /// Processes at once (default: two thirds of the logical cores).
        #[arg(long)]
        jobs: Option<usize>,
        /// At most this many pipelines.
        #[arg(long)]
        limit: Option<usize>,
        /// Those made before too.
        #[arg(long)]
        again: bool,
        /// Only the pipelines earlier runs drew with.
        #[arg(long)]
        used: bool,
        /// After warming the driver cache, also populate and save
        /// `host-pipelines.bin` in-process for all non-bad matching pipelines.
        #[arg(long)]
        save_library: bool,
        /// Apply the VIctory-RTX-Mc3yohan NVIDIA preset shader cache defaults
        /// for unset `VICTORY_*` variables.
        #[arg(long)]
        nvidia_preset: bool,
        /// (What the processes it starts are given.)
        #[arg(long, hide = true)]
        these: Option<String>,
    },
    /// Prepare, map and bind a title executable; with --execute, run it.
    Run {
        exe: PathBuf,
        /// What unimplemented imports do when called.
        #[arg(long, value_enum, default_value_t = Unimplemented::Trap)]
        unimplemented: Unimplemented,
        /// Disable host pass-through (everything must be implemented by us).
        #[arg(long)]
        strict: bool,
        /// Actually start the title. Without it, stop after binding.
        #[arg(long)]
        execute: bool,
        /// The console model the title sees (device type, memory).
        #[arg(long, value_enum, default_value_t = Console::SeriesX)]
        console: Console,
        /// Apply the VIctory-RTX-Mc3yohan NVIDIA preset defaults (Ports 1-2-5-6-7,
        /// ReBAR, Wave32, 1080p output on Series X) for any unset `VICTORY_*`
        /// variables so external launchers (`AmericasLauncher`) get full RTX tuning.
        #[arg(long)]
        nvidia_preset: bool,
        /// Arguments for the title, after `--` (it reads the whole command
        /// line; a title that relaunches itself passes some).
        #[arg(last = true)]
        title_args: Vec<String>,
    },
    /// Run a captured frame's GPU work again (diagnostics builds; see
    /// docs/replay.md).
    #[cfg(all(windows, feature = "diagnostics"))]
    Replay(replay::Replay),
    /// Show the resolved configuration and check it.
    Config,
}

#[derive(Clone, Copy, ValueEnum)]
enum Unimplemented {
    Trap,
    Zero,
}

#[derive(Clone, Copy, ValueEnum)]
enum Console {
    SeriesX,
    SeriesS,
}

fn main() -> Result<()> {
    victory_core::logging::init("info");
    // The audio recorder `run` starts for `VICTORY_AUDIO_CAPTURE`.
    #[cfg(all(windows, feature = "diagnostics"))]
    if victory_hle::modules::audio::capture::child() {
        return Ok(());
    }
    let cli = Cli::parse();
    match cli.command {
        Command::Inspect {
            file,
            imports,
            exports,
        } => inspect::run(&file, imports, exports),
        Command::Census {
            paths,
            strict,
            missing,
        } => census::run(paths, strict, missing),
        Command::LibExports { lib, list } => inspect::lib_exports(&lib, list),
        Command::ShaderInfo {
            paths,
            verbose,
            disasm,
        } => shader::run(paths, verbose || disasm, disasm),
        Command::Translate {
            paths,
            validate,
            pipelines,
            list,
            jobs,
            warm,
        } => {
            // `VICTORY_VIEW_LIMIT=<null slot>`: translate as on a host whose view heap is capped
            if let Some(n) = std::env::var("VICTORY_VIEW_LIMIT").ok().and_then(|v| v.parse().ok()) {
                victory_shader::emit::set_view_heap_limit(n);
            }
            let wave = translate::host_wave::select(
                std::env::var("VICTORY_HOST_WAVE").ok().as_deref(),
                std::env::var("VICTORY_HOST_MAX_WAVE").ok().as_deref(),
                |message| eprintln!("{message}"),
            );
            if let Some(file) = warm {
                return translate::warm(paths, &file, jobs, wave);
            }
            translate::run(paths, validate, pipelines, list, jobs, wave)
        }
        Command::Shaders {
            paths,
            jobs,
            list,
            nvidia_preset,
        } => {
            if nvidia_preset {
                apply_nvidia_preset(None);
            }
            translate::precompile(paths, jobs, list)
        }
        #[cfg(windows)]
        Command::Warm {
            jobs,
            limit,
            again,
            used,
            save_library,
            nvidia_preset,
            these,
        } => {
            if nvidia_preset {
                apply_nvidia_preset(None);
            }
            warm::run(jobs, limit, again, used, save_library, these)
        }
        Command::Run {
            exe,
            unimplemented,
            strict,
            execute,
            console,
            nvidia_preset,
            title_args,
        } => {
            if nvidia_preset {
                apply_nvidia_preset(Some(console));
            }
            victory_hle::runtime::set_title_args(title_args);
            run::run(&exe, unimplemented, strict, execute, console)
        }
        #[cfg(all(windows, feature = "diagnostics"))]
        Command::Replay(options) => replay::run(options),
        Command::Config => show_config(),
    }
}

fn show_config() -> Result<()> {
    let cfg = victory_core::Config::load()?;
    let status = |p: &Option<PathBuf>| match p {
        Some(p) if p.exists() => format!("{}", p.display()),
        Some(p) => format!("{}  (does not exist)", p.display()),
        None => "(not set)".to_owned(),
    };
    println!(
        "config file : {}",
        cfg.source
            .as_ref()
            .map_or("(none)".into(), |p| p.display().to_string())
    );
    println!("game_dir    : {}", status(&cfg.game_dir));
    println!("cache_dir   : {}", cfg.cache_dir.display());
    println!("data_dir    : {}", cfg.data_dir.display());
    for t in &cfg.test_titles {
        println!("test_title  : {}", status(&Some(t.clone())));
    }
    for p in cfg
        .game_dir
        .iter()
        .chain(&cfg.test_titles)
        .chain([&cfg.cache_dir, &cfg.data_dir])
    {
        if let Err(e) = victory_core::ensure_outside_repo(p) {
            println!("POLICY VIOLATION: {e}");
        }
    }
    Ok(())
}

/// Paths given on the command line, or the configured game + test titles.
pub(crate) fn default_paths(paths: Vec<PathBuf>) -> Result<Vec<PathBuf>> {
    if !paths.is_empty() {
        return Ok(paths);
    }
    let cfg = victory_core::Config::load()?;
    let all: Vec<PathBuf> = cfg.game_dir.into_iter().chain(cfg.test_titles).collect();
    anyhow::ensure!(
        !all.is_empty(),
        "no paths given and none configured (see victory.example.toml)"
    );
    Ok(all)
}

/// Applies the `VIctory-RTX-Mc3yohan` NVIDIA RTX defaults for any unset
/// `VICTORY_*` environment variables so launchers without an env block
/// (such as `AmericasLauncher`) get identical shader-cache keys and runtime
/// tuning as `scripts/play-nvidia.ps1`, while still honoring user overrides.
fn apply_nvidia_preset(run_console: Option<Console>) {
    let set_default = |k: &str, v: &str| {
        if std::env::var_os(k).is_none() {
            // SAFETY: called on the main thread during CLI startup before any
            // worker threads or title code exist.
            unsafe { std::env::set_var(k, v) };
        }
    };
    let rt_on = std::env::var("VICTORY_BVH_BUILDER").is_ok_and(|v| v == "1");
    // Shader-cache keys (shared by `shaders`, `warm`, and `run`).
    set_default("VICTORY_SHADER_OPT", "63");
    set_default("VICTORY_WAVE_WIDE", "last");
    set_default("VICTORY_NAN_SCRUB", "1");
    set_default("VICTORY_PORT_CBV", "1");
    set_default("VICTORY_CONSTANT_MARKS", "1");
    if !rt_on {
        set_default("VICTORY_BVH_NO_HIT", "1");
    }
    let Some(console) = run_console else {
        return;
    };
    for (k, v) in [
        ("VICTORY_TEXTURE_NO_WAIT", "1"),
        ("VICTORY_TEXTURE_STAGE_MS", "25"),
        ("VICTORY_TEXTURE_CATCHUP", "1"),
        ("VICTORY_EARLY_TEXTURES_MS", "12"),
        ("VICTORY_BAKE_EXACT", "1"),
        ("VICTORY_BAKE_SOURCES", "1"),
        ("VICTORY_FAIR_DUE", "1"),
        ("VICTORY_SHOW_NEW_LATER", "1"),
        ("VICTORY_UPLOAD_AHEAD", "1"),
        ("VICTORY_SHOW_AFTER", "1"),
        ("VICTORY_VIDEO_PLANES", "1"),
        ("VICTORY_RELAY", "2"),
        ("VICTORY_CARD_STEPS", "2"),
        ("VICTORY_WIDE_BACK", "1"),
        ("VICTORY_CARD_REHASH_RUNS", "1"),
        ("VICTORY_JOIN_COPIES", "1"),
        ("VICTORY_JOIN_BARRIERS", "1"),
        ("VICTORY_INDIRECT_WINDOW", "1"),
        ("VICTORY_RELAY_COPY", "1"),
        ("VICTORY_INDIRECT_BATCH", "2"),
        ("VICTORY_INDIRECT_REST", "0"),
        ("VICTORY_LAND_SKIP", "1"),
        ("VICTORY_LAND_THREADS", "6"),
        ("VICTORY_MAP_THREADS", "1"),
        ("VICTORY_TITLE_PRIORITIES", "0"),
        ("VICTORY_CARD_AWAKE", "idle"),
        ("VICTORY_D3D12_DRED", "light"),
        ("VICTORY_D3D12_BACKGROUND", "0"),
        ("VICTORY_MEMORY_MAP", "0"),
        ("VICTORY_TABLES_EARLY", "1"),
        ("VICTORY_DYNAMIC_SLOTS", "393216"),
        ("VICTORY_PIPELINE_LOWEST", "0"),
        ("VICTORY_PIPELINE_WORKERS", "8"),
        ("VICTORY_PIPELINE_WAIT_ONCE", "1"),
        ("VICTORY_TARGET_VARIANTS", "1"),
        ("VICTORY_NO_WATCHDOG", "1"),
        ("VICTORY_XMA_REFILL", "1"),
        ("VICTORY_PORT_CONSTANTS", "1"),
        // Renderer Port 2 (`VICTORY_PORT_DESCRIPTORS`) stays opt-in: 6 unpatched
        // writers (including external CRT/DLL memcpy) miss thousands of live view
        // records and corrupt one-shot NPC face bakes.
        ("VICTORY_PORT_GUARD", "1"),
        ("VICTORY_PORT_JOURNAL", "1"),
        ("VICTORY_PORT_RT", "1"),
        ("VICTORY_FEED_ONE_CALL", "1"),
        ("VICTORY_CARD_BUFFERS_MIN_KB", "64"),
        ("VICTORY_WINDOWS_BAR_MB", "4096"),
        ("VICTORY_WINDOW_KB", "4096"),
        ("VICTORY_SMALL_RECHECK", "0"),
        ("VICTORY_ASYNC_SUBMIT", "1"),
        ("VICTORY_THIN", "1"),
        ("VICTORY_THIN_THREADS", "8"),
        ("VICTORY_JOIN_PARTS", "8"),
    ] {
        set_default(k, v);
    }
    if !rt_on {
        set_default("VICTORY_AMBIENT", "1.0");
        set_default("VICTORY_AMBIENT_AIR", "1.5");
        set_default("VICTORY_AMBIENT_MOST", "2.0");
        set_default("VICTORY_AMBIENT_SHADE", "0.7,0.5");
        set_default("VICTORY_AMBIENT_REFL", "2.0");
    }
    let fsr_on = std::env::var("VICTORY_FSR_4K").is_ok_and(|v| !v.is_empty() && v != "0");
    if !fsr_on {
        set_default("VICTORY_UPSCALER", "6");
    }
    if matches!(console, Console::SeriesX) && !fsr_on {
        set_default("VICTORY_OUTPUT", "1920x1080");
        set_default("VICTORY_DISPLAY_MODE", "1920x1080");
    }
}
