//! Profiles for the optional trustno1-coder/VIctory-NVIDIA runtime.

use crate::config::{LauncherConfig, local_app_data};
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::process::Command;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub enum Engine {
    #[default]
    Local,
    Nvidia1080,
    NvidiaDlss2x,
}

impl Engine {
    pub fn label(self) -> &'static str {
        match self {
            Self::Local => "VIctory-RTX local",
            Self::Nvidia1080 => "VIctory-NVIDIA · 1080p → 1440p",
            Self::NvidiaDlss2x => "VIctory-NVIDIA · DLSS Calidad + FG 2×",
        }
    }
}

pub fn root() -> PathBuf {
    local_app_data().join("victory/engines/victory-nvidia")
}

pub fn executable(cfg: &LauncherConfig) -> PathBuf {
    cfg.nvidia_executable
        .clone()
        .unwrap_or_else(|| root().join("victory.exe"))
}

pub fn home(base: &Path, engine: Engine) -> PathBuf {
    base.join(if engine == Engine::NvidiaDlss2x {
        "dlss-2x"
    } else {
        "native"
    })
}

const STREAMLINE_SR: [&str; 4] = [
    "sl.interposer.dll",
    "sl.common.dll",
    "sl.dlss.dll",
    "nvngx_dlss.dll",
];
const STREAMLINE_FG: [&str; 4] = [
    "sl.dlss_g.dll",
    "sl.reflex.dll",
    "sl.pcl.dll",
    "nvngx_dlssg.dll",
];

fn sdk_complete(dir: &Path, framegen: bool) -> bool {
    STREAMLINE_SR
        .iter()
        .chain(STREAMLINE_FG.iter().filter(|_| framegen))
        .all(|file| dir.join(file).is_file())
}

fn play_args(engine: Engine, title: &Path, preload: bool) -> Vec<String> {
    let mut args = vec![
        "play".to_owned(),
        title.to_string_lossy().into_owned(),
        "--res".to_owned(),
        "1080to1440".to_owned(),
        "--dlss".to_owned(),
    ];
    if engine == Engine::NvidiaDlss2x {
        args.extend(["quality", "--framegen", "2", "--fg-type", "dlss"].map(str::to_owned));
    } else {
        args.extend(["off", "--no-framegen-ready"].map(str::to_owned));
    }
    if preload {
        args.push("--preload".to_owned());
    }
    args
}

fn saved<'a>(menu: &'a str, key: &str) -> Option<&'a str> {
    menu.lines()
        .rev()
        .filter_map(|line| line.trim().split_once('='))
        .find_map(|(name, val)| (name.trim() == key).then(|| val.trim()))
}

fn sdk_needs(engine: Engine, menu: &str) -> (bool, bool) {
    let preset = engine == Engine::NvidiaDlss2x;
    let dlss = saved(menu, "VICTORY_MENU_DLSS")
        .filter(|mode| {
            matches!(
                *mode,
                "off" | "quality" | "balanced" | "performance" | "dlaa"
            )
        })
        .map_or(preset, |s| s != "off");
    let framegen = saved(menu, "VICTORY_MENU_FRAMEGEN").map_or(preset, |s| {
        s.parse::<u32>().is_ok_and(|n| (1..=6).contains(&n))
    });
    (
        dlss,
        dlss && framegen && saved(menu, "VICTORY_MENU_FG_TYPE") != Some("fsr"),
    )
}

pub fn command(cfg: &LauncherConfig, title: &Path, preload: bool) -> Result<Command, String> {
    build_command(cfg, title, preload, &root(), &executable(cfg))
}

fn build_command(
    cfg: &LauncherConfig,
    title: &Path,
    preload: bool,
    base: &Path,
    exe: &Path,
) -> Result<Command, String> {
    if !exe.is_absolute()
        || !exe.is_file()
        || !exe
            .file_name()
            .is_some_and(|n| n.eq_ignore_ascii_case("victory.exe"))
    {
        return Err("Selecciona victory.exe de VIctory-NVIDIA en Ajustes o instala la versión oficial en la ruta indicada.".into());
    }
    victory_core::ensure_outside_repo(exe)
        .map_err(|e| format!("Selecciona la instalación externa de VIctory-NVIDIA: {e}"))?;
    let home = home(base, cfg.engine);
    let cache = home.join("cache");
    let data = base.join("data");
    for path in [&home, &cache, &data] {
        victory_core::ensure_outside_repo(path).map_err(|e| e.to_string())?;
    }
    std::fs::create_dir_all(&home)
        .map_err(|e| format!("No se pudo abrir la carpeta de VIctory-NVIDIA: {e}"))?;
    let mut cmd = Command::new(exe);
    cmd.args(play_args(cfg.engine, title, preload))
        .current_dir(&home);
    // Start independent of local renderer options, including VICTORY_CONFIG.
    // F1 restart and the preload subprocess inherit these same storage paths.
    for (key, _) in std::env::vars_os() {
        if key
            .to_string_lossy()
            .to_ascii_uppercase()
            .starts_with("VICTORY_")
        {
            cmd.env_remove(key);
        }
    }
    cmd.env("VICTORY_HOME", &home)
        .env("VICTORY_CACHE_DIR", cache)
        .env("VICTORY_DATA_DIR", data)
        .env("VICTORY_GAME_DIR", &cfg.game_dir);
    let menu_file = home.join("victory-settings.txt");
    let menu = match std::fs::read_to_string(menu_file) {
        Ok(menu) => menu,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => String::new(),
        Err(e) => return Err(format!("No se pudo leer la configuración F1: {e}")),
    };
    let (dlss, framegen) = sdk_needs(cfg.engine, &menu);
    let beside = exe.parent().expect("absolute file");
    let suffix = "nvidia-sdk/Streamline/bin/x64";
    let candidates = [
        cfg.nvidia_streamline.clone(),
        std::env::var_os("VICTORY_STREAMLINE_DIR").map(PathBuf::from),
        Some(base.join(suffix)),
        Some(home.join(suffix)),
        Some(local_app_data().join("victory").join(suffix)),
        Some(beside.join(suffix)),
        Some(beside.to_owned()),
    ];
    if let Some(dir) = candidates
        .into_iter()
        .flatten()
        .find(|dir| sdk_complete(dir, framegen))
    {
        cmd.env("VICTORY_STREAMLINE_DIR", dir);
    } else if dlss {
        return Err("Faltan las bibliotecas oficiales de NVIDIA Streamline para los ajustes DLSS guardados. Selecciona la carpeta bin/x64 en Ajustes.".into());
    }
    Ok(cmd)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_engine_roundtrips_and_f1_state_is_preserved() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("victory.exe");
        std::fs::write(&exe, []).unwrap();
        let title = tmp.path().join("game_scarlett_master.exe");
        let mut cfg: LauncherConfig = serde_json::from_str(r#"{"game_dir":"D:/Game"}"#).unwrap();
        assert_eq!(cfg.engine, Engine::Local);
        cfg.engine = Engine::NvidiaDlss2x;
        cfg.create_profile("Calidad").unwrap();
        cfg.engine = Engine::Nvidia1080;
        let mut cfg: LauncherConfig =
            serde_json::from_value(serde_json::to_value(cfg).unwrap()).unwrap();
        cfg.load_profile("Calidad").unwrap();
        assert_eq!(cfg.engine, Engine::NvidiaDlss2x);
        let mode = home(tmp.path(), cfg.engine);
        std::fs::create_dir_all(&mode).unwrap();
        let f1 = mode.join("victory-settings.txt");
        let changes = "# Saved by F1\r\nVICTORY_MENU_DLSS = off\r\n";
        std::fs::write(&f1, changes).unwrap();
        let cmd = build_command(&cfg, &title, true, tmp.path(), &exe).unwrap();
        assert_eq!(cmd.get_current_dir(), Some(mode.as_path()));
        assert_eq!(cmd.get_program(), exe.as_os_str());
        let vars: std::collections::BTreeMap<_, _> = cmd.get_envs().collect();
        for (key, path) in [
            ("VICTORY_HOME", mode.clone()),
            ("VICTORY_CACHE_DIR", mode.join("cache")),
            ("VICTORY_DATA_DIR", tmp.path().join("data")),
            ("VICTORY_GAME_DIR", PathBuf::from(cfg.game_dir)),
        ] {
            assert_eq!(
                vars.get(std::ffi::OsStr::new(key)),
                Some(&Some(path.as_os_str()))
            );
        }
        assert_eq!(std::fs::read_to_string(f1).unwrap(), changes);
    }

    #[test]
    fn preloading_and_play_share_the_same_profile_arguments() {
        let title = Path::new(r"C:\Game files\game_scarlett_master.exe");
        for profile in [Engine::Nvidia1080, Engine::NvidiaDlss2x] {
            let play = play_args(profile, title, false);
            let prep = play_args(profile, title, true);
            assert_eq!(&prep[..play.len()], play);
            assert_eq!(prep.last().unwrap(), "--preload");
            assert_eq!(&play[2..4], ["--res", "1080to1440"]);
            assert_eq!(
                play.contains(&"--framegen".to_owned()),
                profile == Engine::NvidiaDlss2x
            );
        }
        assert_ne!(
            home(Path::new("base"), Engine::Nvidia1080),
            home(Path::new("base"), Engine::NvidiaDlss2x)
        );
    }

    #[test]
    fn sdk_check_tracks_the_settings_saved_in_f1() {
        assert_eq!(
            sdk_needs(Engine::NvidiaDlss2x, "VICTORY_MENU_DLSS=off\n"),
            (false, false)
        );
        assert_eq!(
            sdk_needs(
                Engine::Nvidia1080,
                "VICTORY_MENU_DLSS=quality\nVICTORY_MENU_FRAMEGEN=2\n"
            ),
            (true, true)
        );
        let sdk = tempfile::tempdir().unwrap();
        for file in STREAMLINE_SR {
            std::fs::write(sdk.path().join(file), []).unwrap();
        }
        assert!(sdk_complete(sdk.path(), false));
        assert!(!sdk_complete(sdk.path(), true));
    }
}
