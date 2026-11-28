#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! VIctory-RTX-Mc3yohan · Launcher Nativo en Español (NVIDIA RTX / Wave32)
//! Flujo guiado en 3 pasos para cualquier usuario + Menú Principal minimalista
//! con arranque instantáneo (escaneo de GPU y caché en segundo plano).

mod art;
mod config;
mod hardware;
mod nvidia;
mod nvidia_session;
mod runner;
mod theme;

use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Frame, Layout, Margin, Rect, RichText, ScrollArea,
    Sense, Stroke, TextureHandle, Vec2, pos2, vec2,
};

use config::{
    ConsoleModel, FsrQuality, GameFilesStatus, LauncherConfig, OutputRes, Preset, TextureQuality,
    TriState,
};
use hardware::{ControllerPoller, HardwareInfo};
use runner::{CacheStatus, EnginePaths, GitInfo, LogFileEntry, ProcessRunner, TaskKind};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Jugar,
    Ajustes,
    Logs,
    Actualizar,
}

impl Tab {
    fn label_es(self) -> &'static str {
        match self {
            Self::Jugar => "Jugar",
            Self::Ajustes => "Ajustes",
            Self::Logs => "Logs",
            Self::Actualizar => "Actualizar",
        }
    }
}

struct VictoryLauncherApp {
    tab: Tab,
    in_wizard: bool,
    wizard_step: u8, // 1, 2, 3
    cfg: LauncherConfig,
    profile_selection: Option<String>,
    profile_name: String,
    confirm_profile_delete: bool,
    hw: HardwareInfo,
    hw_async: Arc<Mutex<Option<HardwareInfo>>>,
    engine: EnginePaths,
    game: GameFilesStatus,
    cache: CacheStatus,
    cache_root: PathBuf,
    cache_async: Arc<Mutex<Option<(u64, CacheStatus)>>>,
    cache_scan_generation: u64,
    cache_exact_started: bool,
    git: Arc<Mutex<GitInfo>>,
    git_checking: Arc<Mutex<bool>>,
    controllers: ControllerPoller,
    runner: ProcessRunner,
    banner_tex: Option<TextureHandle>,
    banner_async: Arc<Mutex<Option<(u64, String, Option<egui::ColorImage>)>>>,
    banner_gen: u64,
    banner_loaded_for: String,
    log_filter: String,
    log_files: Vec<LogFileEntry>,
    logs_scanned: bool,
    viewing_file_name: Option<String>,
    viewing_file_lines: Vec<String>,
    last_finished_seq: u64,
    frames_drawn: u32,
    toast: Option<(String, Color32, Instant)>,
}

impl VictoryLauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply_theme(&cc.egui_ctx);
        let engine = EnginePaths::discover();
        // Arranque instantáneo (< 5 ms CPU antes del primer cuadro):
        // - RAM/CPU/elevación por Win32 nativo (0 subprocesos).
        // - Portada PNG, `nvidia-smi` y `git rev-parse` en hilos separados.
        // - Conteo exacto de caché (~230k shaders) y escaneo de logs diferidos
        //   hasta que el usuario abra Ajustes o Logs.
        let hw = HardwareInfo::probe_fast();
        let (cfg, config_warning) = config::load_config(engine.repo_root.as_deref());
        let game = GameFilesStatus::inspect(Path::new(&cfg.game_dir));
        let cache_root = runner::configured_cache_root(&engine, &cfg, &hw);
        let cache = CacheStatus::scan_fast(&cache_root);
        let git_local = GitInfo::placeholder(engine.repo_root.as_deref());

        let hw_async = Arc::new(Mutex::new(None));
        let cache_async = Arc::new(Mutex::new(None));
        let banner_async = Arc::new(Mutex::new(None));
        let git = Arc::new(Mutex::new(git_local));

        // 1. Sondeo real de GPU (nvidia-smi) en su propio hilo.
        {
            let hw_slot = hw_async.clone();
            let ctx_bg = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let probed = HardwareInfo::probe();
                *hw_slot.lock().unwrap_or_else(|e| e.into_inner()) = Some(probed);
                ctx_bg.request_repaint();
            });
        }

        // 2. Estado Git local en hilo independiente (nunca espera a nvidia-smi).
        if let Some(repo_root) = engine.repo_root.clone() {
            let git_slot = git.clone();
            let ctx_bg = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let git_info = GitInfo::probe_local(Some(&repo_root));
                *git_slot.lock().unwrap_or_else(|e| e.into_inner()) = git_info;
                ctx_bg.request_repaint();
            });
        }

        // 3. Decodificación PNG de portada en segundo plano (sin bloquear el primer cuadro).
        let banner_loaded_for = cfg.game_dir.clone();
        let banner_gen = 1u64;
        if !cfg.game_dir.is_empty() {
            let dir = cfg.game_dir.clone();
            let slot = banner_async.clone();
            let ctx_bg = cc.egui_ctx.clone();
            std::thread::spawn(move || {
                let img = art::load_game_banner_image(Path::new(&dir));
                let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
                if guard.as_ref().is_none_or(|(g, _, _)| banner_gen >= *g) {
                    *guard = Some((banner_gen, dir, img));
                }
                ctx_bg.request_repaint();
            });
        }

        // Mostrar el asistente guiado la primera vez o si falta la carpeta del juego.
        let in_wizard = !cfg.wizard_completed || !game.ready();
        let wizard_step = if !game.ready() {
            1
        } else if !runner::preparation_verified(&engine, &cfg, &hw, game.exe_path.as_deref()) {
            2
        } else {
            3
        };

        Self {
            tab: Tab::Jugar,
            in_wizard,
            wizard_step,
            cfg,
            profile_selection: None,
            profile_name: String::new(),
            confirm_profile_delete: false,
            hw,
            hw_async,
            engine,
            game,
            cache,
            cache_root,
            cache_async,
            cache_scan_generation: 0,
            cache_exact_started: false,
            git,
            git_checking: Arc::new(Mutex::new(false)),
            controllers: ControllerPoller::default(),
            runner: ProcessRunner::default(),
            banner_tex: None,
            banner_async,
            banner_gen,
            banner_loaded_for,
            log_filter: String::new(),
            log_files: Vec::new(),
            logs_scanned: false,
            viewing_file_name: None,
            viewing_file_lines: Vec::new(),
            last_finished_seq: 0,
            frames_drawn: 0,
            toast: config_warning.map(|warning| (warning, theme::STATUS_ERR, Instant::now())),
        }
    }

    fn set_toast(&mut self, msg: impl Into<String>, color: Color32) {
        self.toast = Some((msg.into(), color, Instant::now()));
    }

    fn spawn_banner_load(&mut self, ctx: &egui::Context) {
        let dir = self.cfg.game_dir.clone();
        self.banner_gen = self.banner_gen.saturating_add(1);
        let epoch = self.banner_gen;
        self.banner_loaded_for = dir.clone();
        self.banner_tex = None;
        if dir.is_empty() {
            return;
        }
        let slot = self.banner_async.clone();
        let ctx_bg = ctx.clone();
        std::thread::spawn(move || {
            let img = art::load_game_banner_image(Path::new(&dir));
            let mut guard = slot.lock().unwrap_or_else(|e| e.into_inner());
            if guard.as_ref().is_none_or(|(g, _, _)| epoch >= *g) {
                *guard = Some((epoch, dir, img));
            }
            ctx_bg.request_repaint();
        });
    }

    fn ensure_cache_exact(&mut self, ctx: &egui::Context, force: bool) {
        if self.cache_exact_started && !force {
            return;
        }
        self.cache_exact_started = true;
        self.cache_scan_generation = self.cache_scan_generation.wrapping_add(1);
        let generation = self.cache_scan_generation;
        let root = self.cache_root.clone();
        let cache_slot = self.cache_async.clone();
        let ctx_bg = ctx.clone();
        std::thread::spawn(move || {
            let exact = CacheStatus::scan_exact(&root);
            let mut slot = cache_slot.lock().unwrap_or_else(|e| e.into_inner());
            if slot.as_ref().is_none_or(|(existing, _)| generation >= *existing) {
                *slot = Some((generation, exact));
            }
            ctx_bg.request_repaint();
        });
    }

    fn refresh_cache_root(&mut self) -> bool {
        let root = runner::configured_cache_root(&self.engine, &self.cfg, &self.hw);
        if self.cache_root == root {
            return false;
        }
        self.cache_root = root;
        self.cache = CacheStatus::scan_fast(&self.cache_root);
        self.cache_exact_started = false;
        self.cache_scan_generation = self.cache_scan_generation.wrapping_add(1);
        true
    }

    fn preparation_verified(&self) -> bool {
        runner::preparation_verified(&self.engine, &self.cfg, &self.hw, self.game.exe_path.as_deref())
    }

    fn refresh_status(&mut self, ctx: &egui::Context) {
        self.engine = EnginePaths::discover();
        self.game = GameFilesStatus::inspect(Path::new(&self.cfg.game_dir));
        let _ = self.refresh_cache_root();
        self.cache = CacheStatus::scan_fast(&self.cache_root);
        if self.tab == Tab::Ajustes || self.cache_exact_started {
            self.ensure_cache_exact(ctx, true);
        }
        if self.tab == Tab::Logs || self.logs_scanned {
            self.log_files = runner::scan_data_logs();
            self.logs_scanned = true;
        }
        if self.cfg.game_dir != self.banner_loaded_for {
            self.spawn_banner_load(ctx);
        }
    }

    fn persist(&mut self) -> bool {
        if let Err(e) = config::save_config(&self.cfg, self.engine.repo_root.as_deref()) {
            self.set_toast(e, theme::STATUS_ERR);
            return false;
        }
        let _ = self.refresh_cache_root();
        true
    }

    fn save_candidate(&mut self, candidate: LauncherConfig) -> bool {
        let previous = std::mem::replace(&mut self.cfg, candidate);
        if self.persist() {
            return true;
        }
        self.cfg = previous;
        false
    }

    fn change_game_folder(&mut self, folder: &Path, ctx: &egui::Context) -> bool {
        if let Err(error) = victory_core::ensure_outside_repo(folder) {
            self.set_toast(error.to_string(), theme::STATUS_ERR);
            return false;
        }
        if !GameFilesStatus::inspect(folder).ready() {
            self.set_toast("Esa carpeta no contiene los archivos del juego.", theme::STATUS_WARN);
            return false;
        }
        let mut candidate = self.cfg.clone();
        candidate.set_game_dir(folder.to_string_lossy().into_owned());
        if !self.save_candidate(candidate) {
            return false;
        }
        self.profile_selection = None;
        self.profile_name.clear();
        self.confirm_profile_delete = false;
        self.refresh_status(ctx);
        true
    }

    fn pick_game_folder(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().set_title("Selecciona la carpeta del juego");
        if !self.cfg.game_dir.is_empty() {
            dialog = dialog.set_directory(&self.cfg.game_dir);
        }
        if let Some(folder) = dialog.pick_folder()
            && self.change_game_folder(&folder, ctx)
        {
            self.set_toast("Carpeta del juego lista.", theme::STATUS_OK);
        }
    }

    fn auto_search_game(&mut self, ctx: &egui::Context) {
        match config::auto_find_game_dir(self.engine.repo_root.as_deref()) {
            Some(found) if self.change_game_folder(&found, ctx) => {
                self.set_toast("Juego encontrado automáticamente.", theme::STATUS_OK);
            }
            Some(_) => {}
            None => {
                self.set_toast(
                    "No se encontró automáticamente. Pulsa 'Elegir carpeta' para ubicarlo.",
                    theme::STATUS_WARN,
                );
            }
        }
    }

    fn finish_wizard(&mut self) {
        let mut candidate = self.cfg.clone();
        candidate.wizard_completed = true;
        if self.save_candidate(candidate) {
            self.in_wizard = false;
            self.tab = Tab::Jugar;
        }
    }

    fn edit_game_profiles(
        &mut self,
        edit: impl FnOnce(&mut LauncherConfig) -> Result<Option<String>, String>,
        message: &str,
    ) {
        let mut candidate = self.cfg.clone();
        match edit(&mut candidate) {
            Ok(selection) => {
                if self.save_candidate(candidate) {
                    self.profile_selection = selection;
                    self.profile_name.clear();
                    self.confirm_profile_delete = false;
                    self.set_toast(message, theme::STATUS_OK);
                }
            }
            Err(error) => self.set_toast(error, theme::STATUS_ERR),
        }
    }

    fn apply_game_profile(&mut self, name: &str) {
        self.edit_game_profiles(
            |cfg| {
                cfg.load_profile(name)?;
                Ok(Some(name.into()))
            },
            "Perfil aplicado para el próximo inicio.",
        );
    }

    fn select_builtin_preset(&mut self, preset: Preset) {
        let mut candidate = self.cfg.clone();
        candidate.apply_preset(preset);
        self.save_candidate(candidate);
    }

    fn ui_game_profiles(&mut self, ui: &mut egui::Ui, is_running: bool) {
        theme::bento_card().show(ui, |ui| {
            ui.label(
                RichText::new("Perfiles de este juego")
                    .strong()
                    .color(theme::ACCENT_BLUE_LIGHT)
                    .size(14.0),
            );
            ui.add_space(6.0);
            if !self.game.ready() {
                ui.label("Selecciona la carpeta del juego para guardar sus perfiles.");
                return;
            }

            let names: Vec<_> = self.cfg.profile_names().map(str::to_owned).collect();
            ui.add_enabled_ui(!is_running, |ui| {
                ui.horizontal_wrapped(|ui| {
                    let quality = Preset::quality_for(self.hw.vram_tier);
                    if ui.button(format!("Usar {}", quality.label_es())).on_hover_text(quality.description_es(&self.hw)).clicked() {
                        self.select_builtin_preset(quality);
                    }
                    if !names.is_empty() {
                        egui::ComboBox::from_id_salt("game_profiles")
                            .selected_text(self.profile_selection.as_deref().unwrap_or("Elegir perfil"))
                            .show_ui(ui, |ui| {
                                for name in names {
                                    if ui.selectable_value(&mut self.profile_selection, Some(name.clone()), name).changed() {
                                        self.confirm_profile_delete = false;
                                    }
                                }
                            });
                    }
                    if let Some(name) = self.profile_selection.clone() {
                        if ui.button("Aplicar").clicked() {
                            self.apply_game_profile(&name);
                        }
                        if ui.button("Guardar cambios en el perfil").clicked() {
                            self.edit_game_profiles(
                                |cfg| {
                                    cfg.update_profile(&name)?;
                                    Ok(Some(name))
                                },
                                "Perfil actualizado con los ajustes actuales.",
                            );
                        }
                        if ui.button("Eliminar perfil").clicked() {
                            self.confirm_profile_delete = true;
                        }
                    }
                });
                ui.horizontal_wrapped(|ui| {
                    ui.add(egui::TextEdit::singleline(&mut self.profile_name)
                        .hint_text("Nombre del perfil")
                        .desired_width(172.0)
                        .char_limit(48));
                    let has_name = !self.profile_name.trim().is_empty();
                    if ui.add_enabled(has_name, Button::new("Guardar como perfil")).clicked() {
                        let name = self.profile_name.clone();
                        self.edit_game_profiles(|cfg| cfg.create_profile(&name).map(Some), "Perfil guardado.");
                    }
                    if let Some(previous) = self.profile_selection.clone()
                        && ui.add_enabled(has_name, Button::new("Cambiar nombre")).clicked()
                    {
                        let name = self.profile_name.clone();
                        self.edit_game_profiles(|cfg| cfg.rename_profile(&previous, &name).map(Some), "Nombre actualizado.");
                    }
                });
                if self.confirm_profile_delete && let Some(name) = self.profile_selection.clone() {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(format!("¿Eliminar «{name}»?"));
                        if ui.button("Confirmar eliminación").clicked() {
                            self.edit_game_profiles(|cfg| { cfg.delete_profile(&name)?; Ok(None) }, "Perfil eliminado.");
                        }
                        if ui.button("Cancelar").clicked() {
                            self.confirm_profile_delete = false;
                        }
                    });
                }
            });
        });
    }

    fn trigger_git_check(&mut self) {
        let Some(root) = self.engine.repo_root.clone() else {
            self.set_toast("No hay repositorio Git en esta carpeta.", theme::STATUS_WARN);
            return;
        };
        {
            let mut checking = self.git_checking.lock().unwrap_or_else(|e| e.into_inner());
            if *checking {
                return;
            }
            *checking = true;
        }
        let git_arc = self.git.clone();
        let flag_arc = self.git_checking.clone();
        std::thread::spawn(move || {
            let info = GitInfo::fetch_remote_status(&root);
            *git_arc.lock().unwrap_or_else(|e| e.into_inner()) = info;
            *flag_arc.lock().unwrap_or_else(|e| e.into_inner()) = false;
        });
    }

    // =========================================================================
    // MODO 1: ASISTENTE GUIADO EN 3 PASOS (Cero abrumación para nuevos usuarios)
    // =========================================================================

    fn ui_wizard(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snap: &runner::RunnerSnapshot,
    ) {
        let is_running = snap.running.is_some();

        // Cabecera minimalista del asistente.
        ui.horizontal(|ui| {
            let (mark_rect, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
            theme::paint_brand_mark(ui.painter(), mark_rect);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.spacing_mut().item_spacing.y = 0.0;
                ui.label(RichText::new("VIctory-RTX-Mc3yohan").strong().size(15.5));
                ui.label(
                    RichText::new("CONFIGURACIÓN INICIAL · EDICIÓN NVIDIA RTX")
                        .size(9.5)
                        .strong()
                        .color(theme::ACCENT_BLUE_LIGHT),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if self.game.ready() && !is_running {
                    let skip = Button::new(
                        RichText::new("Ir directo al menú")
                            .color(theme::TEXT_SECONDARY)
                            .size(12.0),
                    )
                    .fill(Color32::TRANSPARENT)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER));
                    if ui.add(skip).clicked() {
                        self.finish_wizard();
                    }
                }
            });
        });

        ui.add_space(22.0);

        // Contenido centrado de ancho cómodo (máx 680 px).
        let card_w = (ui.available_width() - 40.0).clamp(480.0, 680.0);
        let pad_x = ((ui.available_width() - card_w) * 0.5).max(0.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.add_space(pad_x);
                    ui.allocate_ui_with_layout(
                        vec2(card_w, ui.available_height()),
                        Layout::top_down(Align::Min),
                        |ui| {
                            // Indicador limpio de 3 pasos.
                            self.ui_wizard_stepper(ui);
                            ui.add_space(18.0);

                            if self.hw.is_elevated_admin {
                                theme::alert_banner(theme::STATUS_WARN, Color32::from_rgb(34, 25, 12))
                                    .show(ui, |ui| {
                                        ui.label(
                                            RichText::new(
                                                "Consejo: abriste el programa como Administrador. Ábrelo normal para proteger la caché de NVIDIA.",
                                            )
                                            .color(theme::STATUS_WARN)
                                            .size(12.0),
                                        );
                                    });
                                ui.add_space(12.0);
                            }

                            // Tarjeta principal del paso actual.
                            Frame::new()
                                .fill(theme::BG_CARD)
                                .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
                                .corner_radius(CornerRadius::same(16))
                                .inner_margin(Margin::symmetric(28, 24))
                                .show(ui, |ui| {
                                    ui.set_min_width(card_w - 56.0);
                                    match self.wizard_step {
                                        1 => self.ui_wizard_step1(ui, ctx),
                                        2 => self.ui_wizard_step2(ui, snap, is_running),
                                        _ => self.ui_wizard_step3(ui),
                                    }
                                });
                        },
                    );
                });
            });
    }

    fn ui_wizard_stepper(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let steps = [(1u8, "1. Juego"), (2u8, "2. Gráficos"), (3u8, "3. Listo")];
            for (idx, (num, label)) in steps.iter().enumerate() {
                let active = self.wizard_step == *num;
                let done = self.wizard_step > *num;
                let (bg, border, txt) = if active {
                    (theme::ACCENT_BLUE, theme::ACCENT_BLUE_LIGHT, Color32::WHITE)
                } else if done {
                    (
                        Color32::from_rgba_unmultiplied(16, 185, 129, 32),
                        theme::STATUS_OK,
                        theme::STATUS_OK,
                    )
                } else {
                    (theme::BG_CARD, theme::BORDER, theme::TEXT_MUTED)
                };
                let pill = Button::new(RichText::new(*label).color(txt).strong().size(12.5))
                    .fill(bg)
                    .stroke(Stroke::new(1.0_f32, border))
                    .corner_radius(CornerRadius::same(99))
                    .min_size(vec2(115.0, 30.0));
                let can_jump = *num == 1 || self.game.ready();
                if ui.add_enabled(can_jump, pill).clicked() {
                    self.wizard_step = *num;
                }
                if idx < 2 {
                    ui.add_space(6.0);
                    let (line_rect, _) =
                        ui.allocate_exact_size(vec2(36.0, 2.0), Sense::hover());
                    ui.painter().line_segment(
                        [line_rect.left_center(), line_rect.right_center()],
                        Stroke::new(
                            1.5_f32,
                            if done {
                                theme::STATUS_OK
                            } else {
                                theme::BORDER_STRONG
                            },
                        ),
                    );
                    ui.add_space(6.0);
                }
            }
        });
    }

    fn ui_wizard_step1(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        ui.horizontal(|ui| {
            theme::paint_badge_icon(
                ui,
                theme::BadgeKind::Folder,
                if self.game.ready() {
                    theme::STATUS_OK
                } else {
                    theme::ACCENT_BLUE_LIGHT
                },
            );
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(RichText::new("Paso 1 · Ubica tu juego").strong().size(20.0));
                ui.label(
                    RichText::new("Selecciona la carpeta donde guardaste tu copia del juego.")
                        .color(theme::TEXT_SECONDARY)
                        .size(13.0),
                );
            });
        });

        ui.add_space(20.0);

        if self.game.ready() {
            theme::alert_banner(theme::STATUS_OK, Color32::from_rgb(10, 30, 24)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Juego detectado y listo")
                                .strong()
                                .color(theme::STATUS_OK)
                                .size(14.0),
                        );
                        ui.label(
                            RichText::new(&self.cfg.game_dir)
                                .color(theme::TEXT_PRIMARY)
                                .size(12.5),
                        );
                        ui.label(
                            RichText::new(self.game.summary_es())
                                .color(theme::TEXT_SECONDARY)
                                .size(11.5),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.button("Cambiar carpeta").clicked() {
                            self.pick_game_folder(ctx);
                        }
                    });
                });
            });
        } else {
            Frame::new()
                .fill(theme::BG_CARD_ALT)
                .stroke(Stroke::new(1.0_f32, theme::BORDER))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new("Aún no se ha seleccionado la carpeta del juego.")
                            .color(theme::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let search = Button::new(
                            RichText::new("Buscar automáticamente")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(theme::ACCENT_BLUE)
                        .min_size(vec2(190.0, 38.0));
                        if ui.add(search).clicked() {
                            self.auto_search_game(ctx);
                        }
                        let pick = Button::new(RichText::new("Elegir carpeta manualmente"))
                            .min_size(vec2(190.0, 38.0));
                        if ui.add(pick).clicked() {
                            self.pick_game_folder(ctx);
                        }
                    });
                });
        }

        ui.add_space(26.0);
        ui.horizontal(|ui| {
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let next = Button::new(
                    RichText::new("Continuar  ->")
                        .strong()
                        .size(14.0)
                        .color(if self.game.ready() {
                            Color32::WHITE
                        } else {
                            theme::TEXT_MUTED
                        }),
                )
                .fill(if self.game.ready() {
                    theme::ACCENT_BLUE
                } else {
                    theme::BG_CARD_ALT
                })
                .corner_radius(CornerRadius::same(9))
                .min_size(vec2(165.0, 42.0));
                if ui.add_enabled(self.game.ready(), next).clicked() {
                    self.wizard_step = 2;
                }
            });
        });
    }

    fn ui_wizard_step2(
        &mut self,
        ui: &mut egui::Ui,
        snap: &runner::RunnerSnapshot,
        is_running: bool,
    ) {
        if self.cfg.engine != nvidia::Engine::Local {
            ui.label("La precarga de VIctory-NVIDIA se inicia desde Jugar y abre la partida al terminar.");
            if ui.button("Abrir launcher").clicked() {
                self.finish_wizard();
            }
            return;
        }
        let verified = self.preparation_verified();
        ui.horizontal(|ui| {
            theme::paint_badge_icon(
                ui,
                theme::BadgeKind::Bolt,
                if verified {
                    theme::STATUS_OK
                } else {
                    theme::ACCENT_BLUE_LIGHT
                },
            );
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Paso 2 · Prepara los gráficos")
                        .strong()
                        .size(20.0),
                );
                ui.label(
                    RichText::new(
                        "Esto evita congelamientos y tirones la primera vez que entras al mundo.",
                    )
                    .color(theme::TEXT_SECONDARY)
                    .size(13.0),
                );
            });
        });

        ui.add_space(20.0);

        if is_running {
            Frame::new()
                .fill(theme::BG_CARD_ALT)
                .stroke(Stroke::new(1.0_f32, theme::ACCENT_BLUE))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new(&snap.progress.step_label)
                                .strong()
                                .color(theme::ACCENT_BLUE_LIGHT)
                                .size(14.0),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!(
                                    "{:02}:{:02}",
                                    snap.elapsed_secs / 60,
                                    snap.elapsed_secs % 60
                                ))
                                .color(theme::TEXT_SECONDARY),
                            );
                        });
                    });
                    ui.add_space(8.0);
                    ui.add(
                        egui::ProgressBar::new(snap.progress.fraction())
                            .fill(theme::ACCENT_BLUE)
                            .desired_height(16.0)
                            .show_percentage(),
                    );
                    ui.add_space(10.0);
                    let stop = Button::new(RichText::new("Cancelar").color(Color32::WHITE))
                        .fill(theme::STATUS_ERR);
                    if ui.add(stop).clicked() {
                        self.runner.stop();
                    }
                });
        } else if verified {
            theme::alert_banner(theme::STATUS_OK, Color32::from_rgb(10, 30, 24)).show(ui, |ui| {
                ui.label(
                    RichText::new("¡Tus gráficos ya están preparados!")
                        .strong()
                        .color(theme::STATUS_OK)
                        .size(14.5),
                );
                let shader_txt = if self.cache.shader_count_exact {
                    format!("{} shaders listos ({} MB)", self.cache.shader_files, self.cache.shader_mb)
                } else {
                    "Shaders compilados y listos".to_owned()
                };
                ui.label(
                    RichText::new(format!(
                        "{shader_txt}{}",
                        if self.cache.pipeline_lib_exists {
                            format!(" · {} MB de escenas aceleradas", self.cache.pipeline_lib_mb)
                        } else {
                            String::new()
                        }
                    ))
                    .color(theme::TEXT_PRIMARY)
                    .size(12.5),
                );
            });
        } else if let Some(legacy) = self.cache.legacy_importable.clone() {
            theme::alert_banner(theme::ACCENT_BLUE, Color32::from_rgb(11, 24, 44)).show(
                ui,
                |ui| {
                    ui.label(
                        RichText::new("Encontramos gráficos ya compilados en tu PC")
                            .strong()
                            .color(Color32::WHITE)
                            .size(14.5),
                    );
                    let mut extras = Vec::new();
                    if legacy.has_fidelityfx {
                        extras.push("FSR 3.1");
                    }
                    if legacy.has_saves {
                        extras.push("partidas");
                    }
                    let extra_txt = if extras.is_empty() {
                        String::new()
                    } else {
                        format!(" + {}", extras.join(" y "))
                    };
                    let legacy_shaders = if legacy.shader_files >= 500 {
                        "miles de shaders".to_owned()
                    } else {
                        format!("{} shaders", legacy.shader_files)
                    };
                    ui.label(
                        RichText::new(format!(
                            "Detectamos {legacy_shaders} y {} MB de escenas{extra_txt}. Puedes importarlos en segundos en vez de recompilar.",
                            legacy.pipeline_mb
                        ))
                        .color(theme::TEXT_SECONDARY)
                        .size(12.5),
                    );
                    ui.add_space(12.0);
                    ui.horizontal(|ui| {
                        let imp = Button::new(
                            RichText::new("Importar caché existente (recomendado)")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(theme::ACCENT_BLUE)
                        .min_size(vec2(250.0, 38.0));
                        if ui.add(imp).clicked()
                            && let Err(e) = self.runner.start_import_legacy_cache(legacy)
                        {
                            self.set_toast(e, theme::STATUS_ERR);
                        }
                        if ui
                            .add(
                                Button::new("Preparar desde cero")
                                    .min_size(vec2(150.0, 38.0)),
                            )
                            .clicked()
                            && let Err(e) =
                                self.runner.start_prep(&self.engine, &self.cfg, self.hw.clone(), TaskKind::PrepareAll)
                        {
                            self.set_toast(e, theme::STATUS_ERR);
                        }
                    });
                },
            );
        } else {
            Frame::new()
                .fill(theme::BG_CARD_ALT)
                .stroke(Stroke::new(1.0_f32, theme::BORDER))
                .corner_radius(CornerRadius::same(12))
                .inner_margin(Margin::same(18))
                .show(ui, |ui| {
                    ui.label(
                        RichText::new(
                            "Toma entre 2 y 6 minutos una sola vez y deja 2 núcleos libres para que puedas seguir usando tu PC.",
                        )
                        .color(theme::TEXT_SECONDARY)
                        .size(12.5),
                    );
                    ui.add_space(12.0);
                    let prep = Button::new(
                        RichText::new("Preparar gráficos ahora")
                            .strong()
                            .size(14.0)
                            .color(Color32::WHITE),
                    )
                    .fill(theme::ACCENT_BLUE)
                    .min_size(vec2(220.0, 40.0));
                    if ui.add(prep).clicked()
                        && let Err(e) =
                            self.runner.start_prep(&self.engine, &self.cfg, self.hw.clone(), TaskKind::PrepareAll)
                    {
                        self.set_toast(e, theme::STATUS_ERR);
                    }
                });
        }

        ui.add_space(26.0);
        ui.horizontal(|ui| {
            if ui
                .add_enabled(!is_running, Button::new("<-  Atrás").min_size(vec2(105.0, 40.0)))
                .clicked()
            {
                self.wizard_step = 1;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let ready = verified;
                let label = if ready {
                    "Continuar  ->"
                } else {
                    "Hacerlo después  ->"
                };
                let next = Button::new(
                    RichText::new(label)
                        .strong()
                        .size(13.5)
                        .color(if ready {
                            Color32::WHITE
                        } else {
                            theme::TEXT_SECONDARY
                        }),
                )
                .fill(if ready {
                    theme::ACCENT_BLUE
                } else {
                    theme::BG_CARD_ALT
                })
                .corner_radius(CornerRadius::same(9))
                .min_size(vec2(175.0, 40.0));
                if ui.add_enabled(!is_running, next).clicked() {
                    self.wizard_step = 3;
                }
            });
        });
    }

    fn ui_wizard_step3(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            theme::paint_badge_icon(ui, theme::BadgeKind::Sliders, theme::ACCENT_BLUE_LIGHT);
            ui.add_space(4.0);
            ui.vertical(|ui| {
                ui.label(
                    RichText::new("Paso 3 · Elige cómo quieres jugar")
                        .strong()
                        .size(20.0),
                );
                ui.label(
                    RichText::new(format!(
                        "Tarjeta detectada: {}. Elige el perfil que quieres aplicar.",
                        self.hw.gpu_name
                    ))
                    .color(theme::TEXT_SECONDARY)
                    .size(13.0),
                );
            });
        });

        ui.add_space(18.0);

        // 3 tarjetas simples y claras (sin abrumar).
        let quality = Preset::quality_for(self.hw.vram_tier);
        let quality_tip = quality.description_es(&self.hw);
        let choices = [
            (
                Preset::Auto,
                "Recomendado",
                "Ajuste automático ideal según tu gráfica y RAM.",
            ),
            (
                Preset::Fluid720p,
                "Más FPS (720p)",
                "Máxima fluidez en laptops o gráficas de 8 GB.",
            ),
            (quality, quality.label_es(), quality_tip.as_str()),
        ];

        ui.columns(3, |cols| {
            for (i, (preset, title, desc)) in choices.iter().enumerate() {
                let selected = self.cfg.preset == *preset;
                let frame = Frame::new()
                    .fill(if selected {
                        theme::ACCENT_BLUE_SOFT
                    } else {
                        theme::BG_CARD_ALT
                    })
                    .stroke(Stroke::new(
                        if selected { 1.6_f32 } else { 1.0_f32 },
                        if selected {
                            theme::ACCENT_BLUE_LIGHT
                        } else {
                            theme::BORDER
                        },
                    ))
                    .corner_radius(CornerRadius::same(12))
                    .inner_margin(Margin::same(14));
                let resp = frame
                    .show(&mut cols[i], |ui| {
                        ui.set_min_height(92.0);
                        ui.label(
                            RichText::new(*title)
                                .strong()
                                .size(14.5)
                                .color(if selected {
                                    Color32::WHITE
                                } else {
                                    theme::TEXT_PRIMARY
                                }),
                        );
                        ui.add_space(4.0);
                        ui.label(
                            RichText::new(*desc)
                                .color(theme::TEXT_SECONDARY)
                                .size(11.8),
                        );
                    })
                    .response
                    .interact(Sense::click());
                if resp.clicked() {
                    self.select_builtin_preset(*preset);
                }
            }
        });

        ui.add_space(16.0);

        // Control rápido: mando o teclado/ratón (diferido al cuadro 2 para no retrasar apertura).
        let pad = (self.frames_drawn >= 2).then(|| self.controllers.poll().clone());
        let pad_any = pad.as_ref().is_some_and(|p| p.any());
        Frame::new()
            .fill(theme::BG_CARD_ALT)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .corner_radius(CornerRadius::same(10))
            .inner_margin(Margin::symmetric(14, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    theme::paint_badge_icon(
                        ui,
                        theme::BadgeKind::Pad,
                        if pad_any || self.cfg.kbm_gta {
                            theme::STATUS_OK
                        } else {
                            theme::TEXT_MUTED
                        },
                    );
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new(if let Some(p) = pad.as_ref().filter(|p| p.any()) {
                                p.summary_es()
                            } else {
                                "Conecta un mando Xbox/PlayStation o activa Teclado y Ratón.".to_owned()
                            })
                            .color(theme::TEXT_SECONDARY)
                            .size(12.0),
                        );
                        if ui
                            .checkbox(
                                &mut self.cfg.kbm_gta,
                                "Jugar con Teclado y Ratón (F2 fija la cámara)",
                            )
                            .changed()
                        {
                            self.persist();
                        }
                    });
                });
            });

        ui.add_space(24.0);
        ui.horizontal(|ui| {
            if ui
                .add(Button::new("<-  Atrás").min_size(vec2(105.0, 42.0)))
                .clicked()
            {
                self.wizard_step = 2;
            }
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                let finish = Button::new(
                    RichText::new("Ir al Menú Principal  ->")
                        .strong()
                        .size(14.5)
                        .color(Color32::WHITE),
                )
                .fill(theme::ACCENT_BLUE)
                .stroke(Stroke::new(1.0_f32, theme::ACCENT_BLUE_LIGHT))
                .corner_radius(CornerRadius::same(10))
                .min_size(vec2(220.0, 44.0));
                if ui.add(finish).clicked() {
                    self.finish_wizard();
                }
            });
        });
    }

    // =========================================================================
    // MODO 2: MENÚ PRINCIPAL LIMPIO Y ESPACIOSO (Cero sobrecarga)
    // =========================================================================

    fn ui_top_header(&mut self, ui: &mut egui::Ui, snap: &runner::RunnerSnapshot) {
        let git_snapshot = self.git.lock().unwrap_or_else(|e| e.into_inner()).clone();

        Frame::new()
            .fill(theme::BG_HEADER)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .inner_margin(Margin::symmetric(20, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    let (mark_rect, _) = ui.allocate_exact_size(Vec2::splat(32.0), Sense::hover());
                    theme::paint_brand_mark(ui.painter(), mark_rect);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(RichText::new("VIctory-RTX-Mc3yohan").strong().size(15.0));
                        ui.label(
                            RichText::new("EDICIÓN NVIDIA RTX · WAVE32")
                                .size(9.0)
                                .strong()
                                .color(theme::ACCENT_BLUE_LIGHT),
                        );
                    });

                    ui.add_space(22.0);

                    for tab in [Tab::Jugar, Tab::Ajustes, Tab::Logs, Tab::Actualizar] {
                        let active = self.tab == tab;
                        let mut label = tab.label_es().to_owned();
                        if tab == Tab::Actualizar && git_snapshot.behind_count > 0 {
                            label = format!("Actualizar ({})", git_snapshot.behind_count);
                        }
                        let btn = Button::new(
                            RichText::new(label)
                                .color(if active { Color32::WHITE } else { theme::TEXT_SECONDARY })
                                .strong()
                                .size(13.0),
                        )
                        .fill(if active {
                            theme::ACCENT_BLUE_SOFT
                        } else {
                            Color32::TRANSPARENT
                        })
                        .stroke(if active {
                            Stroke::new(1.0_f32, theme::ACCENT_BLUE)
                        } else {
                            Stroke::NONE
                        })
                        .corner_radius(CornerRadius::same(8))
                        .min_size(vec2(82.0, 30.0));
                        if ui.add(btn).clicked() {
                            self.tab = tab;
                            if tab == Tab::Ajustes {
                                self.ensure_cache_exact(ui.ctx(), false);
                            } else if tab == Tab::Logs {
                                self.log_files = runner::scan_data_logs();
                                self.logs_scanned = true;
                            }
                        }
                    }

                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("v{} · {}", env!("CARGO_PKG_VERSION"), env!("VICTORY_GIT_SHA")))
                                .color(theme::TEXT_MUTED)
                                .size(11.0),
                        );
                        ui.add_space(8.0);
                        if let Some(task) = snap.running {
                            theme::status_pill(ui, task.label_es(), theme::ACCENT_BLUE_LIGHT);
                        } else if git_snapshot.behind_count > 0 {
                            theme::status_pill(ui, "Actualización lista", theme::STATUS_OK);
                        }
                    });
                });
            });
    }

    fn ui_jugar_minimal(
        &mut self,
        ui: &mut egui::Ui,
        snap: &runner::RunnerSnapshot,
    ) {
        let is_running = snap.running.is_some();

        if self.hw.is_elevated_admin {
            theme::alert_banner(theme::STATUS_WARN, Color32::from_rgb(34, 25, 12)).show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Aviso: ábrelo sin permisos de Administrador para que NVIDIA guarde bien la caché de shaders.",
                    )
                    .color(theme::STATUS_WARN)
                    .size(12.0),
                );
            });
            ui.add_space(10.0);
        }

        // --- HERO CINEMATOGRÁFICO (ocupa el protagonismo, 0 textos cortados ni centrados) ---
        let avail_w = ui.available_width();
        let avail_h = ui.available_height();
        let reserve_bottom = if is_running && !snap.running.is_some_and(TaskKind::is_play) {
            165.0_f32
        } else {
            92.0_f32
        };
        let banner_h = (avail_h - reserve_bottom).clamp(290.0, 440.0);
        let (banner_rect, _) = ui.allocate_exact_size(vec2(avail_w, banner_h), Sense::hover());

        // Portada local en modo `object-fit: cover` (sin estirar jamás!).
        if let Some(tex) = &self.banner_tex {
            let uv = art::cover_uv(tex.size(), banner_rect);
            ui.painter()
                .image(tex.id(), banner_rect, uv, Color32::WHITE);
        }
        theme::paint_hero_overlay(ui.painter(), banner_rect, self.banner_tex.is_some());

        // Bloque inferior-izquierdo con Rect exacto (jamás choca con el botón derecho ni se sale).
        let left_rect = Rect::from_min_max(
            pos2(banner_rect.left() + 28.0, banner_rect.bottom() - 122.0),
            pos2((banner_rect.right() - 270.0).max(banner_rect.left() + 260.0), banner_rect.bottom() - 22.0),
        );
        let title_size = if left_rect.width() < 520.0 { 30.0 } else { 35.0 };
        ui.allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(left_rect)
                .layout(Layout::bottom_up(Align::Min)),
            |ui| {
                let info_line = if let Some(med) = snap.metrics.median_fps() {
                    format!(
                        "{}  ·  Última partida: {:.1} FPS (máx {:.1})",
                        self.hw.gpu_name, med, snap.metrics.best_fps
                    )
                } else {
                    self.hw.summary_line_es()
                };
                ui.label(
                    RichText::new(info_line)
                        .color(theme::TEXT_SECONDARY)
                        .size(13.0),
                );
                ui.add_space(2.0);
                ui.label(
                    RichText::new(if self.cfg.engine == nvidia::Engine::Local { "VIctory-RTX-Mc3yohan" } else { "VIctory-NVIDIA" })
                        .color(Color32::WHITE)
                        .strong()
                        .size(title_size),
                );
                ui.label(
                    RichText::new("MC3YOHAN  ·  EDICIÓN NVIDIA RTX")
                        .color(theme::ACCENT_BLUE_LIGHT)
                        .strong()
                        .size(11.0),
                );
            },
        );

        // Bloque inferior-derecho con Rect exacto (siempre pegado al borde derecho!).
        let cta_rect = Rect::from_min_max(
            pos2(banner_rect.right() - 246.0, banner_rect.bottom() - 80.0),
            pos2(banner_rect.right() - 26.0, banner_rect.bottom() - 24.0),
        );
        ui.allocate_new_ui(
            egui::UiBuilder::new()
                .max_rect(cta_rect)
                .layout(Layout::centered_and_justified(egui::Direction::LeftToRight)),
            |ui| {
                if is_running {
                    let stop = Button::new(
                        RichText::new("DETENER")
                            .strong()
                            .size(16.0)
                            .color(Color32::WHITE),
                    )
                    .fill(theme::STATUS_ERR)
                    .stroke(Stroke::new(1.0_f32, theme::STATUS_ERR_DARK))
                    .corner_radius(CornerRadius::same(11));
                    if ui.add(stop).clicked() {
                        self.runner.stop();
                    }
                } else {
                    let can_play = self.game.ready() && match self.cfg.engine {
                        nvidia::Engine::Local => self.engine.active_victory(self.cfg.use_diag_build).is_some(),
                        _ => nvidia::executable(&self.cfg).is_file(),
                    };
                    let play = Button::new(
                        RichText::new("JUGAR")
                            .strong()
                            .size(17.5)
                            .color(if can_play { Color32::WHITE } else { theme::TEXT_MUTED }),
                    )
                    .fill(if can_play {
                        theme::ACCENT_BLUE
                    } else {
                        theme::BG_CARD_ALT
                    })
                    .stroke(Stroke::new(
                        1.2_f32,
                        if can_play {
                            theme::ACCENT_BLUE_LIGHT
                        } else {
                            theme::BORDER
                        },
                    ))
                    .corner_radius(CornerRadius::same(11));
                    if ui.add_enabled(can_play, play).clicked() {
                        // Si el usuario pulsa JUGAR en los primeros ~200 ms antes de que
                        // termine `nvidia-smi`, esperar brevemente el perfil real de GPU
                        // para no lanzar RTX 50/8GB con el tier provisional.
                        if !self.hw.probe_done {
                            let deadline = Instant::now() + Duration::from_millis(2000);
                            while Instant::now() < deadline {
                                if let Ok(mut slot) = self.hw_async.try_lock()
                                    && let Some(probed) = slot.take()
                                {
                                    self.hw = probed;
                                    break;
                                }
                                std::thread::sleep(Duration::from_millis(20));
                            }
                        }
                        match self.runner.start_play(&self.engine, &self.cfg, &self.hw).map(drop) {
                            Ok(()) => {
                                self.set_toast(
                                    "Iniciando juego... Puedes ver los FPS en la pestaña Logs.",
                                    theme::ACCENT_BLUE_LIGHT,
                                );
                            }
                            Err(e) => self.set_toast(e, theme::STATUS_ERR),
                        }
                    }
                }
            },
        );

        ui.add_space(14.0);

        // --- UNA sola barra limpia de control rápido debajo del Hero ---
        if self.cfg.engine != nvidia::Engine::Local {
            theme::bento_card().show(ui, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label(self.cfg.engine.label());
                    if ui.add_enabled(!is_running && self.game.ready(), Button::new("Precargar y jugar")).clicked()
                        && let Err(err) = self.runner.start_prep(&self.engine, &self.cfg, self.hw.clone(), TaskKind::PrepareAll)
                    {
                        self.set_toast(err, theme::STATUS_ERR);
                    }
                    if ui.button("Ajustes del motor").clicked() {
                        self.tab = Tab::Ajustes;
                    }
                });
            });
            return;
        }
        let pad = (self.frames_drawn >= 2).then(|| self.controllers.poll().clone());
        Frame::new()
            .fill(theme::BG_CARD)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .corner_radius(CornerRadius::same(12))
            .inner_margin(Margin::symmetric(18, 13))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // 1. Selector compacto de calidad.
                    ui.label(
                        RichText::new("Calidad:")
                            .color(theme::TEXT_SECONDARY)
                            .size(12.5),
                    );
                    let saved_name = self.cfg.matching_profile().map(str::to_owned);
                    let saved_profiles: Vec<_> = self.cfg.profile_names().map(str::to_owned).collect();
                    let preset_tip = if saved_name.is_some() {
                        "Perfil guardado en Ajustes.".to_owned()
                    } else {
                        self.cfg.preset.description_es(&self.hw)
                    };
                    egui::ComboBox::from_id_salt("home_preset_combo")
                        .width(155.0)
                        .selected_text(saved_name.as_deref().unwrap_or(self.cfg.preset.label_es()))
                        .show_ui(ui, |ui| {
                            for p in [
                                Preset::Auto,
                                Preset::Fluid720p,
                                Preset::Balanced1080p,
                                Preset::QualityRtx,
                                Preset::Native1080p,
                                Preset::High1440p,
                                Preset::Fsr4K,
                                Preset::Safe,
                            ] {
                                if ui
                                    .selectable_label(saved_name.is_none() && self.cfg.preset == p, p.label_es())
                                    .clicked()
                                {
                                    self.select_builtin_preset(p);
                                }
                            }
                            if !saved_profiles.is_empty() {
                                ui.separator();
                                for name in saved_profiles {
                                    let active = saved_name.as_deref() == Some(name.as_str());
                                    if ui.selectable_label(active, &name).clicked() {
                                        self.apply_game_profile(&name);
                                    }
                                }
                            }
                        })
                        .response
                        .on_hover_text(preset_tip);

                    ui.add_space(16.0);

                    // 2. Control simple: mando detectado o Teclado y Ratón.
                    if let Some(p) = pad.as_ref().filter(|p| p.any()) {
                        theme::status_pill(ui, &p.badge_es(), theme::STATUS_OK);
                        ui.add_space(6.0);
                    }
                    if ui
                        .checkbox(&mut self.cfg.kbm_gta, "Teclado y Ratón")
                        .changed()
                    {
                        self.persist();
                    }

                    // 3. Derecha: estado limpio de gráficos + acceso rápido al asistente.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let wiz_btn = Button::new(
                            RichText::new("Asistente").color(theme::TEXT_SECONDARY).size(12.0),
                        )
                        .fill(Color32::TRANSPARENT)
                        .stroke(Stroke::new(1.0_f32, theme::BORDER));
                        if ui.add_enabled(!is_running, wiz_btn).clicked() {
                            self.in_wizard = true;
                            self.wizard_step = 1;
                        }

                        ui.add_space(8.0);

                        if self.preparation_verified() {
                            theme::status_pill(ui, "Gráficos listos", theme::STATUS_OK);
                        } else if !is_running {
                            if let Some(legacy) = self.cache.legacy_importable.clone() {
                                let imp = Button::new(
                                    RichText::new("Importar caché previa")
                                        .strong()
                                        .size(12.0)
                                        .color(Color32::WHITE),
                                )
                                .fill(theme::ACCENT_BLUE_STRONG);
                                if ui.add(imp).clicked()
                                    && let Err(e) = self.runner.start_import_legacy_cache(legacy)
                                {
                                    self.set_toast(e, theme::STATUS_ERR);
                                }
                            } else {
                                let prep = Button::new(
                                    RichText::new("Preparar gráficos")
                                        .strong()
                                        .size(12.0)
                                        .color(Color32::WHITE),
                                )
                                .fill(theme::ACCENT_BLUE_STRONG);
                                if ui.add(prep).clicked()
                                    && let Err(e) = self.runner.start_prep(
                                        &self.engine,
                                        &self.cfg,
                                        self.hw.clone(),
                                        TaskKind::PrepareAll,
                                    )
                                {
                                    self.set_toast(e, theme::STATUS_ERR);
                                }
                            }
                        }
                    });
                });
            });

        // Progreso en vivo solo cuando corre una tarea de preparación/importación/actualización.
        if let Some(task) = snap.running
            && !task.is_play()
        {
            ui.add_space(10.0);
            theme::bento_card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&snap.progress.step_label)
                            .strong()
                            .color(theme::ACCENT_BLUE_LIGHT)
                            .size(13.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "{:02}:{:02}",
                                snap.elapsed_secs / 60,
                                snap.elapsed_secs % 60
                            ))
                            .color(theme::TEXT_SECONDARY)
                            .size(12.0),
                        );
                    });
                });
                ui.add_space(5.0);
                ui.add(
                    egui::ProgressBar::new(snap.progress.fraction())
                        .fill(theme::ACCENT_BLUE)
                        .desired_height(14.0)
                        .show_percentage(),
                );
            });
        }
    }

    fn ui_ajustes(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snap: &runner::RunnerSnapshot,
    ) {
        let is_running = snap.running.is_some();
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Ajustes").strong().size(22.0));
                ui.label(
                    RichText::new("Todo lo esencial en un lugar, con opciones avanzadas opcionales.")
                        .color(theme::TEXT_SECONDARY)
                        .size(12.5),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Restaurar recomendados").clicked() {
                    let mut candidate = self.cfg.clone();
                    candidate.reset_settings();
                    if self.save_candidate(candidate) {
                        self.set_toast("Ajustes restaurados al perfil recomendado.", theme::STATUS_OK);
                    }
                }
                if ui
                    .add_enabled(!is_running, Button::new("Repetir asistente inicial"))
                    .clicked()
                {
                    self.in_wizard = true;
                    self.wizard_step = 1;
                }
            });
        });
        ui.add_space(10.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // Carpeta del juego y caché de gráficos.
                self.ui_engine_choice(ui, is_running, &mut changed);
                ui.add_space(8.0);
                if self.cfg.engine != nvidia::Engine::Local {
                    ui.horizontal_wrapped(|ui| {
                        ui.label(&self.cfg.game_dir);
                        if ui.add_enabled(!is_running, Button::new("Elegir carpeta del juego")).clicked() {
                            self.pick_game_folder(ctx);
                        }
                    });
                    self.ui_game_profiles(ui, is_running);
                    return;
                }
                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Carpeta del juego y gráficos")
                            .strong()
                            .color(theme::ACCENT_BLUE_LIGHT)
                            .size(14.0),
                    );
                    ui.add_space(6.0);
                    ui.horizontal_wrapped(|ui| {
                        ui.label(
                            RichText::new(if self.cfg.game_dir.is_empty() {
                                "Sin carpeta seleccionada"
                            } else {
                                &self.cfg.game_dir
                            })
                            .color(theme::TEXT_SECONDARY)
                            .size(12.0),
                        );
                        if ui.button("Buscar automáticamente").clicked() {
                            self.auto_search_game(ctx);
                        }
                        if ui.button("Elegir carpeta").clicked() {
                            self.pick_game_folder(ctx);
                        }
                    });
                    ui.add_space(8.0);
                    ui.horizontal_wrapped(|ui| {
                        let shaders_desc = if self.cache.shader_count_exact {
                            format!("{} ({} MB)", self.cache.shader_files, self.cache.shader_mb)
                        } else {
                            "calculando tamaño".to_owned()
                        };
                        let status_str = format!("Shaders en disco: {shaders_desc}");
                        ui.label(RichText::new(status_str).color(theme::TEXT_SECONDARY).size(12.0));
                        let (status, color) = if self.preparation_verified() {
                            ("Preparación verificada", theme::STATUS_OK)
                        } else {
                            ("Preparación pendiente para estos ajustes", theme::STATUS_WARN)
                        };
                        theme::status_pill(ui, status, color);
                        if ui
                            .add_enabled(
                                !is_running && self.game.ready(),
                                Button::new("Preparar gráficos"),
                            )
                            .clicked()
                            && let Err(e) =
                                self.runner.start_prep(&self.engine, &self.cfg, self.hw.clone(), TaskKind::PrepareAll)
                        {
                            self.set_toast(e, theme::STATUS_ERR);
                        }
                        if self.cache.early_pipelines_count > 0
                            && ui
                                .add_enabled(!is_running, Button::new("Acelerar escenas"))
                                .clicked()
                            && let Err(e) =
                                self.runner.start_prep(&self.engine, &self.cfg, self.hw.clone(), TaskKind::Warm)
                        {
                            self.set_toast(e, theme::STATUS_ERR);
                        }
                    });
                });

                ui.add_space(10.0);

                self.ui_game_profiles(ui, is_running);
                ui.add_space(10.0);

                // Pantalla y calidad.
                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Pantalla y Calidad Gráfica")
                            .strong()
                            .color(theme::ACCENT_BLUE_LIGHT)
                            .size(14.0),
                    );
                    ui.add_space(8.0);

                    egui::Grid::new("easy_settings_grid")
                        .num_columns(2)
                        .spacing([24.0, 10.0])
                        .show(ui, |ui| {
                            ui.label("Resolución de salida:");
                            egui::ComboBox::from_id_salt("out_res")
                                .selected_text(self.cfg.output.label_es())
                                .show_ui(ui, |ui| {
                                    for opt in [
                                        OutputRes::Auto,
                                        OutputRes::P720,
                                        OutputRes::P1080,
                                        OutputRes::P1440,
                                        OutputRes::P4K,
                                    ] {
                                        if ui
                                            .selectable_value(&mut self.cfg.output, opt, opt.label_es())
                                            .changed()
                                        {
                                            self.cfg.preset = Preset::Custom;
                                            changed = true;
                                        }
                                    }
                                });
                            ui.end_row();

                            ui.label("Perfil de consola:");
                            egui::ComboBox::from_id_salt("console_sel")
                                .selected_text(self.cfg.console.label_es())
                                .show_ui(ui, |ui| {
                                    for opt in [
                                        ConsoleModel::Auto,
                                        ConsoleModel::SeriesX,
                                        ConsoleModel::SeriesS,
                                    ] {
                                        if ui
                                            .selectable_value(&mut self.cfg.console, opt, opt.label_es())
                                            .changed()
                                        {
                                            self.cfg.preset = Preset::Custom;
                                            changed = true;
                                        }
                                    }
                                });
                            ui.end_row();

                            ui.label("Detalle de texturas:");
                            egui::ComboBox::from_id_salt("tex_qual")
                                .selected_text(self.cfg.texture_quality.label_es())
                                .show_ui(ui, |ui| {
                                    for opt in [
                                        TextureQuality::Auto,
                                        TextureQuality::Max,
                                        TextureQuality::High,
                                        TextureQuality::Medium,
                                    ] {
                                        if ui
                                            .selectable_value(
                                                &mut self.cfg.texture_quality,
                                                opt,
                                                opt.label_es(),
                                            )
                                            .changed()
                                        {
                                            self.cfg.preset = Preset::Custom;
                                            changed = true;
                                        }
                                    }
                                });
                            ui.end_row();

                            ui.label("Límite de FPS (0 = sin límite):");
                            if ui
                                .add(egui::Slider::new(&mut self.cfg.fps_cap, 0..=120).suffix(" FPS"))
                                .changed()
                            {
                                changed = true;
                            }
                            ui.end_row();

                            ui.label("Escala interna (0 = automático):");
                            if ui
                                .add(
                                    egui::Slider::new(&mut self.cfg.render_scale, 0.0..=1.0)
                                        .fixed_decimals(2),
                                )
                                .changed()
                            {
                                self.cfg.preset = Preset::Custom;
                                changed = true;
                            }
                            ui.end_row();

                            ui.label("Reescalado FSR 3.1 (4K):");
                            egui::ComboBox::from_id_salt("fsr_sel")
                                .selected_text(self.cfg.fsr_4k.label_es())
                                .show_ui(ui, |ui| {
                                    for opt in [
                                        FsrQuality::Off,
                                        FsrQuality::Quality,
                                        FsrQuality::Balanced,
                                        FsrQuality::Performance,
                                    ] {
                                        if ui
                                            .selectable_value(&mut self.cfg.fsr_4k, opt, opt.label_es())
                                            .changed()
                                        {
                                            self.cfg.preset = Preset::Custom;
                                            changed = true;
                                        }
                                    }
                                });
                            ui.end_row();
                        });

                    if self.cfg.fsr_4k != FsrQuality::Off
                        && !config::fidelityfx_dir(self.engine.repo_root.as_deref())
                            .is_some_and(|dir| dir.join("amd_fidelityfx_upscaler_dx12.dll").is_file())
                    {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Falta la DLL de FidelityFX en la ruta configurada para el juego.")
                                .color(theme::STATUS_WARN)
                                .size(11.5),
                        );
                    }

                    ui.add_space(8.0);
                    if self.cfg.effective_console(&self.hw) == "series-x"
                        && ui
                            .add_enabled(
                                !is_running,
                                egui::Checkbox::new(
                                    &mut self.cfg.mesh_indirect_x_only,
                                    "Recuperar terreno en mallas indirectas (Series X)",
                                ),
                            )
                            .on_hover_text("Recupera suelo y objetos que desaparecen en algunas escenas. Se aplica al iniciar el juego.")
                            .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .checkbox(
                            &mut self.cfg.smooth_motion,
                            "Preparar ejecutable para Smooth Motion de NVIDIA App",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .checkbox(
                            &mut self.cfg.kbm_gta,
                            "Controles de PC (Teclado y Ratón, F2 bloquea/desbloquea la cámara)",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if self.cfg.kbm_gta {
                        ui.horizontal(|ui| {
                            ui.label("Sensibilidad del ratón:");
                            if ui
                                .add(egui::Slider::new(&mut self.cfg.mouse_sens, 0.2..=3.0))
                                .changed()
                            {
                                changed = true;
                            }
                            if ui
                                .checkbox(&mut self.cfg.mouse_invert_y, "Invertir eje Y")
                                .changed()
                            {
                                changed = true;
                            }
                        });
                    }
                });

                ui.add_space(10.0);

                // Opciones avanzadas plegadas por defecto.
                theme::bento_card().show(ui, |ui| {
                    egui::CollapsingHeader::new(
                        RichText::new("Opciones avanzadas (para usuarios expertos)")
                            .strong()
                            .size(13.5),
                    )
                    .default_open(false)
                    .show(ui, |ui| {
                        ui.add_space(6.0);
                        ui.horizontal(|ui| {
                            ui.label("Listas rápidas (Thin Lists):");
                            egui::ComboBox::from_id_salt("thin_sel")
                                .selected_text(self.cfg.thin_lists.label_es())
                                .show_ui(ui, |ui| {
                                    for opt in [TriState::Auto, TriState::On, TriState::Off] {
                                        if ui
                                            .selectable_value(&mut self.cfg.thin_lists, opt, opt.label_es())
                                            .changed()
                                        {
                                            self.cfg.preset = Preset::Custom;
                                            changed = true;
                                        }
                                    }
                                });
                            ui.add_space(14.0);
                            ui.label("Ahorro de RAM (<=24 GB):");
                            egui::ComboBox::from_id_salt("ram_sel")
                                .selected_text(self.cfg.ram_low_profile.label_es())
                                .show_ui(ui, |ui| {
                                    for opt in [TriState::Auto, TriState::On, TriState::Off] {
                                        if ui
                                            .selectable_value(
                                                &mut self.cfg.ram_low_profile,
                                                opt,
                                                opt.label_es(),
                                            )
                                            .changed()
                                        {
                                            self.cfg.preset = Preset::Custom;
                                            changed = true;
                                        }
                                    }
                                });
                        });
                        ui.horizontal(|ui| {
                            ui.label("Ventana ReBAR VRAM en MB (0 = auto):");
                            if ui
                                .add(
                                    egui::DragValue::new(&mut self.cfg.windows_bar_mb_override)
                                        .range(0..=16384),
                                )
                                .changed()
                            {
                                self.cfg.preset = Preset::Custom;
                                changed = true;
                            }
                        });
                        let settings = &mut self.cfg.settings;
                        for (val, label) in [
                            (&mut settings.video_planes, "Planos de video en cinemáticas y pantallas"),
                            (&mut settings.live_textures, "Actualización en vivo de texturas dinámicas (VICTORY_LIVE_TEXTURES)"),
                            (&mut settings.port_rt, "Optimizar esperas de memoria (Port RT)"),
                            (&mut settings.skip_rt_resolve, "Omitir CS_RaytracingResolve sin Ray Tracing (+FPS)"),
                            (&mut settings.skip_raymarched_lights, "Omitir luces raymarched sin BVH (+FPS)"),
                            (&mut settings.ambient_light, "Iluminación ambiental sustituta y filtro anti-cuadros magenta"),
                            (&mut settings.tex_store, "Caché de texturas en disco (TexStore)"),
                            (&mut settings.port_descriptors, "Port 2 experimental (VICTORY_PORT_DESCRIPTORS=1)"),
                            (&mut settings.send_bar, "VICTORY_SEND_BAR (envíos en VRAM)"),
                            (&mut settings.wc_no_back, "VICTORY_WC_NO_BACK"),
                            (&mut settings.wait_textures, "Esperar carga completa de texturas"),
                            (&mut settings.record_pipelines, "Grabar catálogo de pipelines (VICTORY_PIPELINE_BOOK)"),
                            (&mut settings.use_diag_build, "Usar compilación de diagnóstico (diag)"),
                            (&mut settings.gpu_timing, "Medir tiempos GPU por shader (VICTORY_GPU_TIMING)"),
                        ] {
                            if ui.checkbox(val, label).changed() {
                                changed = true;
                            }
                        }
                        if self.cfg.ambient_light {
                            ui.horizontal(|ui| {
                                ui.label("Tope luz ambiental:");
                                if ui
                                    .add(egui::Slider::new(&mut self.cfg.ambient_most, 1.5..=4.5))
                                    .changed()
                                {
                                    changed = true;
                                }
                                ui.label("Tope reflejos:");
                                if ui
                                    .add(egui::Slider::new(&mut self.cfg.ambient_reflect, 0.2..=2.5))
                                    .changed()
                                {
                                    changed = true;
                                }
                            });
                        }
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Variables extra (una por línea, ej. VICTORY_DSP_CONVOLUTION=1)")
                                .color(theme::TEXT_SECONDARY)
                                .size(12.0),
                        );
                        if ui
                            .add(
                                egui::TextEdit::multiline(&mut self.cfg.extra_env)
                                    .desired_rows(3)
                                    .desired_width(f32::INFINITY)
                                    .code_editor(),
                            )
                            .changed()
                        {
                            changed = true;
                        }
                    });
                });
            });

        if changed {
            self.persist();
        }
    }

    fn ui_engine_choice(&mut self, ui: &mut egui::Ui, running: bool, changed: &mut bool) {
        use nvidia::Engine::{Local, Nvidia1080, NvidiaDlss2x};
        theme::bento_card().show(ui, |ui| {
            ui.add_enabled_ui(!running, |ui| {
                ui.horizontal_wrapped(|ui| {
                    ui.label("Motor del perfil");
                    egui::ComboBox::from_id_salt("profile-engine").selected_text(self.cfg.engine.label()).show_ui(ui, |ui| {
                        for opt in [Local, Nvidia1080, NvidiaDlss2x] {
                            *changed |= ui.selectable_value(&mut self.cfg.engine, opt, opt.label()).changed();
                        }
                    });
                });
                if self.cfg.engine == Local { return; }
                ui.label(if self.cfg.engine == Nvidia1080 {
                    "Series X · Escena 1920×1080, salida 2560×1440, DLSS y generación apagados."
                } else {
                    "Series X · Salida 2560×1440. DLSS Calidad determina la resolución interna. FG 2× muestra un cuadro generado por cuadro renderizado."
                });
                ui.label("F1 guarda cambios en este modo y tiene prioridad al reiniciar. Ambos modos comparten partidas propias, separadas del motor local.");
                let exe = nvidia::executable(&self.cfg);
                ui.horizontal_wrapped(|ui| {
                    ui.label(exe.display().to_string());
                    if ui.button("Elegir victory.exe").clicked()
                        && let Some(path) = rfd::FileDialog::new().add_filter("Motor oficial", &["exe"]).pick_file()
                    {
                        self.cfg.nvidia_executable = Some(path);
                        *changed = true;
                    }
                });
                if !exe.is_file() {
                    ui.colored_label(theme::STATUS_WARN, "Falta el ejecutable de VIctory-NVIDIA.");
                    ui.hyperlink_to("Versiones oficiales", "https://github.com/trustno1-coder/VIctory-NVIDIA/releases");
                }
                ui.horizontal_wrapped(|ui| {
                    if let Some(dir) = &self.cfg.nvidia_streamline {
                        ui.label(dir.display().to_string());
                    }
                    if ui.button("Elegir carpeta de NVIDIA Streamline").clicked()
                        && let Some(path) = rfd::FileDialog::new().pick_folder()
                    {
                        self.cfg.nvidia_streamline = Some(path);
                        *changed = true;
                    }
                });
            });
        });
    }

    fn ui_logs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snap: &runner::RunnerSnapshot) {
        ui.label(RichText::new("Logs y Soporte").strong().size(22.0));
        ui.label(
            RichText::new(
                "Comparte tus logs en 1 clic sin buscar carpetas ni exponer tu usuario de Windows.",
            )
            .color(theme::TEXT_SECONDARY)
            .size(12.5),
        );
        ui.add_space(10.0);

        theme::bento_card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let copy_btn = Button::new(
                    RichText::new("Copiar reporte para enviar")
                        .strong()
                        .color(Color32::WHITE)
                        .size(13.0),
                )
                .fill(theme::ACCENT_BLUE)
                .corner_radius(CornerRadius::same(8))
                .min_size(vec2(215.0, 36.0));
                if ui.add(copy_btn).clicked() {
                    let report = runner::build_diagnostic_report(
                        &self.hw,
                        &self.cfg,
                        &self.cache,
                        &snap.metrics,
                        &snap.logs,
                    );
                    ctx.copy_text(report);
                    self.set_toast(
                        "Reporte copiado al portapapeles (sin datos privados).",
                        theme::STATUS_OK,
                    );
                }

                let desk_btn = Button::new(
                    RichText::new("Guardar reporte en el Escritorio (.txt)")
                        .strong()
                        .color(theme::TEXT_PRIMARY)
                        .size(13.0),
                )
                .fill(theme::BG_CARD_ALT)
                .stroke(Stroke::new(1.0_f32, theme::ACCENT_BLUE_LIGHT))
                .corner_radius(CornerRadius::same(8))
                .min_size(vec2(250.0, 36.0));
                if ui.add(desk_btn).clicked() {
                    match runner::save_report_to_desktop(
                        &self.hw,
                        &self.cfg,
                        &self.cache,
                        &snap.metrics,
                        &snap.logs,
                    ) {
                        Ok(p) => self.set_toast(
                            format!(
                                "Guardado en tu Escritorio: {}",
                                p.file_name().unwrap_or_default().to_string_lossy()
                            ),
                            theme::STATUS_OK,
                        ),
                        Err(e) => self.set_toast(e, theme::STATUS_ERR),
                    }
                }

                if ui
                    .add(Button::new("Abrir carpeta de logs").min_size(vec2(170.0, 36.0)))
                    .clicked()
                {
                    runner::open_folder(&runner::data_dir());
                }
            });
        });

        ui.add_space(10.0);

        let avail_h = (ui.available_height() - 6.0).max(240.0);
        ui.columns(2, |cols| {
            cols[0].vertical(|ui| {
                ui.horizontal(|ui| {
                    let title = match &self.viewing_file_name {
                        Some(name) => format!("Archivo: {name}"),
                        None => format!("REGISTRO EN VIVO ({} líneas)", snap.logs.len()),
                    };
                    ui.label(
                        RichText::new(title)
                            .strong()
                            .color(theme::ACCENT_BLUE_LIGHT)
                            .size(12.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.log_filter)
                                .hint_text("Filtrar (fps, error, warn)...")
                                .desired_width(175.0),
                        );
                        if self.viewing_file_name.is_some() {
                            if ui.button("Volver al vivo").clicked() {
                                self.viewing_file_name = None;
                                self.viewing_file_lines.clear();
                            }
                        } else if ui.button("Limpiar").clicked() {
                            self.runner.clear_logs();
                        }
                    });
                });
                ui.add_space(6.0);

                Frame::new()
                    .fill(theme::BG_INPUT)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER))
                    .corner_radius(CornerRadius::same(10))
                    .inner_margin(Margin::same(10))
                    .show(ui, |ui| {
                        let source: &[String] = if self.viewing_file_name.is_some() {
                            &self.viewing_file_lines
                        } else {
                            &snap.logs
                        };
                        let needle = self.log_filter.to_ascii_lowercase();
                        ScrollArea::vertical()
                            .max_height((avail_h - 46.0).max(160.0))
                            .stick_to_bottom(self.viewing_file_name.is_none())
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if source.is_empty() {
                                    ui.label(
                                        RichText::new(
                                            "Aquí aparecerán los FPS y mensajes del motor cuando pulses Jugar.",
                                        )
                                        .color(theme::TEXT_MUTED)
                                        .size(12.0),
                                    );
                                }
                                for line in source {
                                    if !needle.is_empty()
                                        && !line.to_ascii_lowercase().contains(&needle)
                                    {
                                        continue;
                                    }
                                    let col = if line.contains("ERROR") || line.contains("error") {
                                        theme::STATUS_ERR
                                    } else if line.contains("WARN") || line.contains("warn") {
                                        theme::STATUS_WARN
                                    } else if line.contains(" fps, ") {
                                        theme::STATUS_OK
                                    } else {
                                        theme::TEXT_SECONDARY
                                    };
                                    ui.label(RichText::new(line).monospace().color(col).size(11.5));
                                }
                            });
                    });
            });

            cols[1].vertical(|ui| {
                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Resumen de la última partida")
                            .strong()
                            .color(theme::ACCENT_BLUE_LIGHT)
                            .size(13.5),
                    );
                    ui.add_space(6.0);
                    let m = &snap.metrics;
                    if let Some(med) = m.median_fps() {
                        ui.label(
                            RichText::new(format!(
                                "{:.1} FPS promedio  (mín {:.1} · máx {:.1})",
                                med, m.worst_fps, m.best_fps
                            ))
                            .strong()
                            .color(theme::STATUS_OK)
                            .size(15.0),
                        );
                        if let Some(ms) = m.last_ms {
                            ui.label(
                                RichText::new(format!(
                                    "Último cuadro: {:.1} ms  ·  Dibujos: {}",
                                    ms,
                                    m.last_draws.unwrap_or(0)
                                ))
                                .color(theme::TEXT_SECONDARY)
                                .size(12.0),
                            );
                        }
                        if !m.last_vram_line.is_empty() {
                            ui.label(
                                RichText::new(&m.last_vram_line)
                                    .color(theme::TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                        if let Some(log_path) = &m.last_log_path
                            && let Some(name) = log_path.file_name().and_then(|s| s.to_str())
                        {
                            ui.label(
                                RichText::new(format!("Archivo: {name}"))
                                    .color(theme::TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                    } else {
                        ui.label(
                            RichText::new("Inicia una partida para ver tus FPS promedio, mínimo, máximo y VRAM.")
                                .color(theme::TEXT_SECONDARY)
                                .size(12.0),
                        );
                    }
                });

                ui.add_space(10.0);

                theme::bento_card().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new("Partidas recientes").strong().size(13.5));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Recargar").clicked() {
                                self.log_files = runner::scan_data_logs();
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ScrollArea::vertical()
                        .max_height((avail_h - 190.0).max(110.0))
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if self.log_files.is_empty() {
                                ui.label(
                                    RichText::new("Todavía no hay archivos de log guardados.")
                                        .color(theme::TEXT_MUTED)
                                        .size(12.0),
                                );
                            }
                            let entries = self.log_files.clone();
                            for entry in entries {
                                ui.horizontal(|ui| {
                                    let col = if entry.is_crash {
                                        theme::STATUS_ERR
                                    } else {
                                        theme::TEXT_PRIMARY
                                    };
                                    ui.label(
                                        RichText::new(format!("{} ({} KB)", entry.name, entry.size_kb))
                                            .color(col)
                                            .size(12.0),
                                    );
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        if ui.small_button("Abrir").clicked() {
                                            runner::open_file(&entry.path);
                                        }
                                        if ui.small_button("Ver").clicked() {
                                            self.viewing_file_name = Some(entry.name.clone());
                                            self.viewing_file_lines =
                                                runner::read_log_tail(&entry.path, 400);
                                        }
                                    });
                                });
                            }
                        });
                });
            });
        });
    }

    fn ui_actualizar(&mut self, ui: &mut egui::Ui, snap: &runner::RunnerSnapshot) {
        let is_running = snap.running.is_some();
        let checking = *self.git_checking.lock().unwrap_or_else(|e| e.into_inner());
        let git_info = self.git.lock().unwrap_or_else(|e| e.into_inner()).clone();

        ui.label(RichText::new("Actualizar").strong().size(22.0));
        ui.label(
            RichText::new(
                "Descarga las últimas mejoras desde GitHub sin borrar tus partidas ni tu caché de shaders.",
            )
            .color(theme::TEXT_SECONDARY)
            .size(12.5),
        );
        ui.add_space(10.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                theme::bento_card().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("Actualización por Git")
                                    .strong()
                                    .color(theme::ACCENT_BLUE_LIGHT)
                                    .size(15.0),
                            );
                            if git_info.available {
                                ui.label(
                                    RichText::new(format!(
                                        "Rama {} · Fuentes {} · Launcher {}",
                                        git_info.branch, git_info.local_sha, env!("VICTORY_GIT_SHA")
                                    ))
                                    .color(theme::TEXT_SECONDARY)
                                    .size(12.5),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Carpeta portable sin repositorio Git local.")
                                        .color(theme::TEXT_SECONDARY)
                                        .size(12.5),
                                );
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if checking {
                                theme::status_pill(ui, "Comprobando...", theme::ACCENT_BLUE_LIGHT);
                            } else if git_info.behind_count > 0 {
                                theme::status_pill(
                                    ui,
                                    &format!("{} nueva(s)", git_info.behind_count),
                                    theme::STATUS_OK,
                                );
                            } else if git_info.up_to_date {
                                theme::status_pill(ui, "Al día", theme::STATUS_OK);
                            }
                        });
                    });

                    ui.add_space(8.0);
                    ui.label(
                        RichText::new(&git_info.last_status_es)
                            .color(theme::TEXT_PRIMARY)
                            .size(12.5),
                    );

                    if !git_info.pending_commits.is_empty() {
                        ui.add_space(6.0);
                        Frame::new()
                            .fill(theme::BG_INPUT)
                            .stroke(Stroke::new(1.0_f32, theme::BORDER))
                            .corner_radius(CornerRadius::same(8))
                            .inner_margin(Margin::same(10))
                            .show(ui, |ui| {
                                for c in git_info.pending_commits.iter().take(10) {
                                    ui.label(
                                        RichText::new(format!("• {c}"))
                                            .monospace()
                                            .color(theme::TEXT_SECONDARY)
                                            .size(11.5),
                                    );
                                }
                            });
                    }

                    ui.add_space(10.0);
                    ui.horizontal_wrapped(|ui| {
                        let check_btn = Button::new(
                            RichText::new(if checking {
                                "Comprobando..."
                            } else {
                                "Buscar actualizaciones"
                            })
                            .strong(),
                        )
                        .min_size(vec2(175.0, 36.0));
                        if ui
                            .add_enabled(git_info.available && !checking && !is_running, check_btn)
                            .clicked()
                        {
                            self.trigger_git_check();
                        }

                        if git_info.available
                            && let Some(root) = self.engine.repo_root.clone()
                        {
                            let pull_btn = Button::new(
                                RichText::new("Descargar y compilar actualización")
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(theme::ACCENT_BLUE)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(vec2(245.0, 36.0));
                            if ui.add_enabled(!is_running && !checking, pull_btn).clicked() {
                                if let Err(e) = self.runner.start_git_update(root, true) {
                                    self.set_toast(e, theme::STATUS_ERR);
                                } else {
                                    self.set_toast(
                                        "Descargando y compilando actualización...",
                                        theme::ACCENT_BLUE_LIGHT,
                                    );
                                }
                            }
                        }

                        if ui.button("Abrir GitHub").clicked() {
                            runner::open_url("https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan");
                        }
                    });
                });

                ui.add_space(10.0);

                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Créditos y Licencia Open Source (GPL-3.0)")
                            .strong()
                            .color(theme::ACCENT_BLUE_LIGHT)
                            .size(14.0),
                    );
                    ui.add_space(6.0);
                    for (name, desc, url) in [
                        (
                            "mc3yohan-lab / VIctory-RTX-Mc3yohan",
                            "Fork NVIDIA RTX Wave32 + Launcher Nativo en Español",
                            "https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan",
                        ),
                        (
                            "Marvisak / VIctory",
                            "Proyecto base open-source GPL-3.0-or-later",
                            "https://github.com/Marvisak/VIctory",
                        ),
                        (
                            "TaynanGT / Vice-Tay",
                            "Alias GPU, colas de texturas y optimizaciones RTX 4090",
                            "https://github.com/TaynanGT/Vice-Tay",
                        ),
                        (
                            "ZigZagBoi1 / ScarlettPort",
                            "Planos de video en cinemáticas, BCPack y formatos",
                            "https://github.com/ZigZagBoi1/ScarlettPort",
                        ),
                        (
                            "avatarmanz / VIctory-7900XTX + Comunidad",
                            "Renderer Ports 1–7, Live Textures #390, VIctory-DXR, Peppacodes, Yaekai, RAM16",
                            "https://github.com/avatarmanz/VIctory-7900XTX",
                        ),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).strong().size(12.5));
                            ui.label(
                                RichText::new(format!("— {desc}"))
                                    .color(theme::TEXT_MUTED)
                                    .size(11.5),
                            );
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Abrir").clicked() {
                                    runner::open_url(url);
                                }
                            });
                        });
                    }
                });
            });
    }
}

impl eframe::App for VictoryLauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.frames_drawn = self.frames_drawn.saturating_add(1);
        // Recoger resultados asíncronos de GPU, caché exacta y portada sin bloquear el hilo UI.
        let hardware = self.hw_async.try_lock().ok().and_then(|mut slot| slot.take());
        if let Some(probed) = hardware {
            self.hw = probed;
            if self.refresh_cache_root() && self.tab == Tab::Ajustes {
                self.ensure_cache_exact(ctx, false);
            }
        }
        if let Ok(mut slot) = self.cache_async.try_lock()
            && let Some((generation, exact)) = slot.take()
            && generation == self.cache_scan_generation
        {
            self.cache = exact;
        }
        if let Ok(mut slot) = self.banner_async.try_lock()
            && let Some((epoch, dir, maybe_img)) = slot.take()
            && epoch == self.banner_gen
            && dir == self.cfg.game_dir
        {
            self.banner_tex = maybe_img.map(|img| art::upload_banner_texture(ctx, img));
            self.banner_loaded_for = dir;
        }

        let snap = self.runner.snapshot();
        if self.tab == Tab::Ajustes {
            self.ensure_cache_exact(ctx, false);
        }

        if snap.finished_seq != self.last_finished_seq {
            self.last_finished_seq = snap.finished_seq;
            self.refresh_status(ctx);
            if let Some(note) = &snap.last_exit_note {
                let col = if note.contains("correctamente")
                    || note.contains("completad")
                    || note.contains("lista")
                {
                    theme::STATUS_OK
                } else {
                    theme::STATUS_WARN
                };
                self.set_toast(note.clone(), col);
            }
            if let Some(root) = self.engine.repo_root.clone() {
                let git_arc = self.git.clone();
                std::thread::spawn(move || {
                    *git_arc.lock().unwrap_or_else(|e| e.into_inner()) =
                        GitInfo::probe_local(Some(&root));
                });
            }
        }

        if snap.running.is_some()
            || !self.hw.probe_done
            || *self.git_checking.lock().unwrap_or_else(|e| e.into_inner())
        {
            ctx.request_repaint_after(Duration::from_millis(120));
        } else {
            ctx.request_repaint_after(Duration::from_millis(800));
        }

        // Cabecera superior solo en el Menú Principal (el asistente lleva su propia barra limpia).
        if !self.in_wizard {
            egui::TopBottomPanel::top("victory_header")
                .frame(Frame::NONE)
                .show(ctx, |ui| {
                    self.ui_top_header(ui, &snap);
                });
        }

        // Toast inferior discreto.
        if let Some((msg, col, at)) = self.toast.clone() {
            if at.elapsed() > Duration::from_secs(6) {
                self.toast = None;
            } else {
                egui::TopBottomPanel::bottom("victory_toast")
                    .frame(
                        Frame::new()
                            .fill(Color32::from_rgb(13, 18, 28))
                            .stroke(Stroke::new(1.0_f32, col))
                            .inner_margin(Margin::symmetric(18, 8)),
                    )
                    .show(ctx, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&msg).color(col).strong().size(12.5));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Cerrar").clicked() {
                                    self.toast = None;
                                }
                            });
                        });
                    });
            }
        }

        egui::CentralPanel::default()
            .frame(Frame::new().fill(theme::BG_CANVAS).inner_margin(Margin::same(18)))
            .show(ctx, |ui| {
                if self.in_wizard {
                    self.ui_wizard(ui, ctx, &snap);
                } else {
                    match self.tab {
                        Tab::Jugar => self.ui_jugar_minimal(ui, &snap),
                        Tab::Ajustes => self.ui_ajustes(ui, ctx, &snap),
                        Tab::Logs => self.ui_logs(ui, ctx, &snap),
                        Tab::Actualizar => self.ui_actualizar(ui, &snap),
                    }
                }
            });
    }
}

fn run_current_profile(action: TaskKind) -> Result<(), String> {
    let engine = EnginePaths::discover();
    let (cfg, warning) = config::load_config(engine.repo_root.as_deref());
    if let Some(warning) = warning {
        return Err(warning);
    }
    let runner = ProcessRunner::default();
    let hw = HardwareInfo::probe();
    let task = match action {
        TaskKind::Play => runner.start_play(&engine, &cfg, &hw)?,
        TaskKind::PrepareAll => runner.start_local_preparation(&engine, &cfg, hw, action)?,
        _ => return Err("Acción no admitida para el perfil actual.".into()),
    };
    let result = task.join().map_err(|_| format!("{} terminó inesperadamente.", action.label_es()))?;
    let snapshot = runner.snapshot();
    for line in snapshot.logs {
        println!("{line}");
    }
    if let Some(note) = snapshot.last_exit_note {
        println!("{note}");
    }
    result
}

fn main() -> eframe::Result<()> {
    let mut args = std::env::args_os().skip(1);
    let action = match (args.next().as_deref(), args.next()) {
        (Some(flag), None) if flag == "--prepare-current-profile" => Some(TaskKind::PrepareAll),
        (Some(flag), None) if flag == "--play-current-profile" => Some(TaskKind::Play),
        _ => None,
    };
    if let Some(action) = action {
        if let Err(error) = run_current_profile(action) {
            eprintln!("{error}");
            std::process::exit(1);
        }
        return Ok(());
    }
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VIctory-RTX-Mc3yohan")
            .with_inner_size([1060.0, 680.0])
            .with_min_inner_size([880.0, 580.0])
            .with_icon(art::app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "VIctory-RTX-Mc3yohan",
        options,
        Box::new(|cc| Ok(Box::new(VictoryLauncherApp::new(cc)))),
    )
}
