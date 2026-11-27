#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

//! VIctory-RTX · Launcher Nativo Quantico Systems (100% Español)
//! Estética oficial Quantico (#007BFF + #4DA3FF + carbón profundo + barra 53.8°),
//! diseñado para cualquier usuario: búsqueda automática del juego, importador de
//! caché, precarga guiada, logs fáciles para enviar y actualización por Git.

mod art;
mod config;
mod hardware;
mod runner;
mod theme;

use std::path::Path;
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
    Inicio,
    Configuracion,
    Logs,
    Actualizar,
}

impl Tab {
    fn label_es(self) -> &'static str {
        match self {
            Self::Inicio => "Inicio",
            Self::Configuracion => "Configuración",
            Self::Logs => "Logs y Soporte",
            Self::Actualizar => "Actualizar",
        }
    }
}

struct VictoryLauncherApp {
    tab: Tab,
    cfg: LauncherConfig,
    hw: HardwareInfo,
    engine: EnginePaths,
    game: GameFilesStatus,
    cache: CacheStatus,
    git: Arc<Mutex<GitInfo>>,
    git_checking: Arc<Mutex<bool>>,
    controllers: ControllerPoller,
    runner: ProcessRunner,
    banner_tex: Option<TextureHandle>,
    banner_loaded_for: String,
    log_filter: String,
    log_files: Vec<LogFileEntry>,
    viewing_file_name: Option<String>,
    viewing_file_lines: Vec<String>,
    last_finished_seq: u64,
    toast: Option<(String, Color32, Instant)>,
}

impl VictoryLauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply_quantico_theme(&cc.egui_ctx);
        let engine = EnginePaths::discover();
        let hw = HardwareInfo::probe();
        let cfg = config::load_config(engine.repo_root.as_deref());
        let game = GameFilesStatus::inspect(Path::new(&cfg.game_dir));
        let cache = CacheStatus::scan();
        let git_local = GitInfo::probe_local(engine.repo_root.as_deref());
        let log_files = runner::scan_data_logs();
        let banner_tex = if !cfg.game_dir.is_empty() {
            art::load_game_banner_texture(&cc.egui_ctx, Path::new(&cfg.game_dir))
        } else {
            None
        };
        let banner_loaded_for = cfg.game_dir.clone();

        Self {
            tab: Tab::Inicio,
            cfg,
            hw,
            engine,
            game,
            cache,
            git: Arc::new(Mutex::new(git_local)),
            git_checking: Arc::new(Mutex::new(false)),
            controllers: ControllerPoller::default(),
            runner: ProcessRunner::default(),
            banner_tex,
            banner_loaded_for,
            log_filter: String::new(),
            log_files,
            viewing_file_name: None,
            viewing_file_lines: Vec::new(),
            last_finished_seq: 0,
            toast: None,
        }
    }

    fn set_toast(&mut self, msg: impl Into<String>, color: Color32) {
        self.toast = Some((msg.into(), color, Instant::now()));
    }

    fn refresh_status(&mut self, ctx: &egui::Context) {
        self.engine = EnginePaths::discover();
        self.game = GameFilesStatus::inspect(Path::new(&self.cfg.game_dir));
        self.cache = CacheStatus::scan();
        self.log_files = runner::scan_data_logs();
        if self.cfg.game_dir != self.banner_loaded_for {
            self.banner_tex = if !self.cfg.game_dir.is_empty() {
                art::load_game_banner_texture(ctx, Path::new(&self.cfg.game_dir))
            } else {
                None
            };
            self.banner_loaded_for = self.cfg.game_dir.clone();
        }
    }

    fn persist(&mut self) {
        if let Err(e) = config::save_config(&self.cfg, self.engine.repo_root.as_deref()) {
            self.set_toast(e, theme::STATUS_ERR);
        }
    }

    fn pick_game_folder(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().set_title("Selecciona la carpeta del juego");
        if !self.cfg.game_dir.is_empty() {
            dialog = dialog.set_directory(&self.cfg.game_dir);
        }
        if let Some(folder) = dialog.pick_folder() {
            self.cfg.game_dir = folder.to_string_lossy().into_owned();
            self.persist();
            self.refresh_status(ctx);
            if self.game.ready() {
                self.set_toast("Carpeta del juego detectada correctamente.", theme::STATUS_OK);
            } else {
                self.set_toast(
                    "Esa carpeta no contiene los archivos del juego (MicrosoftGame.config / .exe).",
                    theme::STATUS_WARN,
                );
            }
        }
    }

    fn auto_search_game(&mut self, ctx: &egui::Context) {
        match config::auto_find_game_dir(self.engine.repo_root.as_deref()) {
            Some(found) => {
                self.cfg.game_dir = found.to_string_lossy().into_owned();
                self.persist();
                self.refresh_status(ctx);
                self.set_toast(
                    format!("Juego encontrado en {}", self.cfg.game_dir),
                    theme::STATUS_OK,
                );
            }
            None => {
                self.set_toast(
                    "No se encontró automáticamente. Pulsa 'Elegir carpeta' para seleccionarlo.",
                    theme::STATUS_WARN,
                );
            }
        }
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

    fn ui_top_header(&mut self, ui: &mut egui::Ui, snap: &runner::RunnerSnapshot) {
        let git_snapshot = self.git.lock().unwrap_or_else(|e| e.into_inner()).clone();

        Frame::new()
            .fill(theme::BG_HEADER)
            .stroke(Stroke::new(1.0_f32, theme::BORDER))
            .inner_margin(Margin::symmetric(18, 10))
            .show(ui, |ui| {
                ui.horizontal(|ui| {
                    // Emblema Quantico + nombre.
                    let (mark_rect, _) = ui.allocate_exact_size(Vec2::splat(34.0), Sense::hover());
                    theme::paint_quantico_mark(ui.painter(), mark_rect);
                    ui.add_space(4.0);
                    ui.vertical(|ui| {
                        ui.spacing_mut().item_spacing.y = 0.0;
                        ui.label(
                            RichText::new("VIctory-RTX")
                                .strong()
                                .size(15.5)
                                .color(theme::TEXT_PRIMARY),
                        );
                        ui.label(
                            RichText::new("QUANTICO SYSTEMS")
                                .size(9.5)
                                .strong()
                                .color(theme::QUANTICO_BLUE_LIGHT),
                        );
                    });

                    ui.add_space(24.0);

                    // Pestañas claras horizontales (diseño propio, cero sidebar copiado).
                    for tab in [Tab::Inicio, Tab::Configuracion, Tab::Logs, Tab::Actualizar] {
                        let active = self.tab == tab;
                        let fill = if active {
                            theme::QUANTICO_BLUE_SOFT
                        } else {
                            Color32::TRANSPARENT
                        };
                        let stroke = if active {
                            Stroke::new(1.0_f32, theme::QUANTICO_BLUE)
                        } else {
                            Stroke::NONE
                        };
                        let text_col = if active {
                            Color32::WHITE
                        } else {
                            theme::TEXT_SECONDARY
                        };
                        let mut label = tab.label_es().to_owned();
                        if tab == Tab::Actualizar && git_snapshot.behind_count > 0 {
                            label = format!("Actualizar ({})", git_snapshot.behind_count);
                        }
                        let btn = Button::new(RichText::new(label).color(text_col).strong().size(13.0))
                            .fill(fill)
                            .stroke(stroke)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(vec2(88.0, 32.0));
                        if ui.add(btn).clicked() {
                            self.tab = tab;
                            if tab == Tab::Logs {
                                self.log_files = runner::scan_data_logs();
                            }
                        }
                    }

                    // Derecha: estado rápido y versión.
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let ver = format!("v0.7.0 · {}", env!("VICTORY_GIT_SHA"));
                        ui.label(RichText::new(ver).color(theme::TEXT_MUTED).size(11.0));
                        ui.add_space(8.0);
                        if let Some(task) = snap.running {
                            theme::status_pill(ui, task.label_es(), theme::QUANTICO_BLUE_LIGHT);
                        } else if git_snapshot.behind_count > 0 {
                            theme::status_pill(ui, "Actualización lista", theme::STATUS_OK);
                        } else if self.game.ready() && self.cache.shaders_ready() {
                            theme::status_pill(ui, "Listo para jugar", theme::STATUS_OK);
                        }
                    });
                });
            });
    }

    fn ui_inicio(
        &mut self,
        ui: &mut egui::Ui,
        ctx: &egui::Context,
        snap: &runner::RunnerSnapshot,
    ) {
        let is_running = snap.running.is_some();

        // --- Banner Hero Quantico (215 px, amplio, cero superposiciones) ---
        let avail_w = ui.available_width();
        let banner_h = 212.0_f32;
        let (banner_rect, _) = ui.allocate_exact_size(vec2(avail_w, banner_h), Sense::hover());

        if let Some(tex) = &self.banner_tex {
            let uv = Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0));
            ui.painter()
                .image(tex.id(), banner_rect, uv, Color32::WHITE);
        }
        theme::paint_quantico_banner_overlay(ui.painter(), banner_rect, self.banner_tex.is_some());

        // Contenido dentro del banner con márgenes holgados.
        let inner = banner_rect.shrink2(vec2(24.0, 20.0));
        let mut child_ui = ui.new_child(
            egui::UiBuilder::new()
                .max_rect(inner)
                .layout(Layout::left_to_right(Align::Center)),
        );

        child_ui.horizontal(|ui| {
            // Bloque izquierdo: identidad + hardware en lenguaje humano.
            ui.allocate_ui_with_layout(
                vec2((inner.width() - 250.0).max(260.0), inner.height()),
                Layout::top_down(Align::Min),
                |ui| {
                    ui.add_space(10.0);
                    ui.label(
                        RichText::new("QUANTICO SYSTEMS  ·  EDICIÓN NVIDIA RTX")
                            .color(theme::QUANTICO_BLUE_LIGHT)
                            .strong()
                            .size(11.0),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new("VIctory-RTX")
                            .color(Color32::WHITE)
                            .strong()
                            .size(34.0),
                    );
                    ui.add_space(4.0);
                    ui.label(
                        RichText::new(self.hw.summary_line_es())
                            .color(theme::TEXT_SECONDARY)
                            .size(13.0),
                    );
                    ui.add_space(10.0);
                    let subtitle = if let Some(task) = snap.running {
                        format!(
                            "{} · {:02}:{:02}",
                            task.label_es(),
                            snap.elapsed_secs / 60,
                            snap.elapsed_secs % 60
                        )
                    } else if !self.game.ready() {
                        "Selecciona o busca la carpeta de tu juego para empezar.".to_owned()
                    } else if !self.cache.shaders_ready() {
                        "Te recomendamos preparar los gráficos (1 sola vez) para jugar sin tirones.".to_owned()
                    } else if let Some(med) = snap.metrics.median_fps() {
                        format!(
                            "Última partida: {:.1} FPS promedio (máx {:.1} FPS) · {} shaders listos",
                            med, snap.metrics.best_fps, self.cache.shader_files
                        )
                    } else {
                        format!(
                            "{} shaders listos · Perfil activo: {}",
                            self.cache.shader_files,
                            self.cfg.preset.label_es()
                        )
                    };
                    ui.label(RichText::new(subtitle).color(theme::TEXT_PRIMARY).size(12.5));
                },
            );

            // Bloque derecho: CTA gigante azul Quantico + botón carpeta.
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                ui.vertical(|ui| {
                    ui.spacing_mut().item_spacing.y = 10.0;
                    if is_running {
                        let stop = Button::new(
                            RichText::new("DETENER").strong().size(16.0).color(Color32::WHITE),
                        )
                        .fill(theme::STATUS_ERR)
                        .stroke(Stroke::new(1.0_f32, theme::STATUS_ERR_DARK))
                        .corner_radius(CornerRadius::same(10))
                        .min_size(vec2(210.0, 50.0));
                        if ui.add(stop).clicked() {
                            self.runner.stop();
                        }
                    } else {
                        let can_play = self.game.ready()
                            && self.engine.active_victory(self.cfg.use_diag_build).is_some();
                        let fill = if can_play {
                            theme::QUANTICO_BLUE
                        } else {
                            Color32::from_rgb(38, 46, 62)
                        };
                        let play = Button::new(
                            RichText::new("JUGAR AHORA")
                                .strong()
                                .size(16.0)
                                .color(if can_play { Color32::WHITE } else { theme::TEXT_MUTED }),
                        )
                        .fill(fill)
                        .stroke(Stroke::new(
                            1.0_f32,
                            if can_play {
                                theme::QUANTICO_BLUE_LIGHT
                            } else {
                                theme::BORDER
                            },
                        ))
                        .corner_radius(CornerRadius::same(10))
                        .min_size(vec2(210.0, 50.0));
                        if ui.add_enabled(can_play, play).clicked() {
                            match self.runner.start_play(&self.engine, &self.cfg, &self.hw) {
                                Ok(()) => {
                                    self.set_toast(
                                        "Iniciando juego... Puedes ver los FPS en vivo en 'Logs y Soporte'.",
                                        theme::QUANTICO_BLUE_LIGHT,
                                    );
                                }
                                Err(e) => self.set_toast(e, theme::STATUS_ERR),
                            }
                        }
                    }

                    let folder_btn = Button::new(
                        RichText::new("Elegir carpeta del juego")
                            .size(12.5)
                            .color(theme::TEXT_PRIMARY),
                    )
                    .fill(Color32::from_rgba_unmultiplied(15, 22, 34, 225))
                    .stroke(Stroke::new(1.0_f32, theme::BORDER_STRONG))
                    .corner_radius(CornerRadius::same(8))
                    .min_size(vec2(210.0, 34.0));
                    if ui.add(folder_btn).clicked() {
                        self.pick_game_folder(ctx);
                    }
                });
            });
        });

        ui.add_space(10.0);

        // --- Alertas contextuales útiles (solo aparecen cuando hacen falta) ---
        if self.hw.is_elevated_admin {
            theme::alert_banner(theme::STATUS_WARN, Color32::from_rgb(34, 25, 12)).show(ui, |ui| {
                ui.label(
                    RichText::new(
                        "Aviso: abriste el launcher como Administrador. Ciérralo y ábrelo de forma normal para que NVIDIA guarde bien la caché de shaders.",
                    )
                    .color(theme::STATUS_WARN)
                    .size(12.0),
                );
            });
            ui.add_space(6.0);
        }

        if let Some(legacy) = self.cache.legacy_importable.clone()
            && !is_running
        {
            theme::alert_banner(theme::QUANTICO_BLUE, Color32::from_rgb(12, 24, 44)).show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label(
                            RichText::new("Detectamos caché previa de gráficos en tu PC")
                                .strong()
                                .color(Color32::WHITE)
                                .size(13.0),
                        );
                        let mut extras = Vec::new();
                        if legacy.has_fidelityfx {
                            extras.push("FidelityFX DLL");
                        }
                        if legacy.has_saves {
                            extras.push("partidas guardadas");
                        }
                        let extra_str = if extras.is_empty() {
                            String::new()
                        } else {
                            format!(" + {}", extras.join(" y "))
                        };
                        ui.label(
                            RichText::new(format!(
                                "Encontramos {} shaders y {} MB de escenas ya compiladas{extra_str} en ScarlettPort. Puedes importarlas en 1 clic sin esperar.",
                                legacy.shader_files, legacy.pipeline_mb
                            ))
                            .color(theme::TEXT_SECONDARY)
                            .size(12.0),
                        );
                    });
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn = Button::new(
                            RichText::new("Importar caché ahora")
                                .strong()
                                .color(Color32::WHITE),
                        )
                        .fill(theme::QUANTICO_BLUE)
                        .corner_radius(CornerRadius::same(8));
                        if ui.add(btn).clicked() {
                            if let Err(e) = self.runner.start_import_legacy_cache(legacy) {
                                self.set_toast(e, theme::STATUS_ERR);
                            }
                        }
                    });
                });
            });
            ui.add_space(6.0);
        }

        // Barra de progreso en vivo cuando se preparan gráficos / importa caché / actualiza.
        if let Some(task) = snap.running
            && task != TaskKind::Play
        {
            theme::bento_card().show(ui, |ui| {
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(&snap.progress.step_label)
                            .strong()
                            .color(theme::QUANTICO_BLUE_LIGHT)
                            .size(13.5),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!(
                                "Tiempo: {:02}:{:02}",
                                snap.elapsed_secs / 60,
                                snap.elapsed_secs % 60
                            ))
                            .color(theme::TEXT_SECONDARY)
                            .size(12.0),
                        );
                    });
                });
                ui.add_space(4.0);
                let frac = snap.progress.fraction();
                let bar = egui::ProgressBar::new(frac)
                    .fill(theme::QUANTICO_BLUE)
                    .desired_height(16.0)
                    .show_percentage();
                ui.add(bar);
                if !snap.progress.last_summary.is_empty() {
                    ui.label(
                        RichText::new(&snap.progress.last_summary)
                            .color(theme::TEXT_MUTED)
                            .size(11.5),
                    );
                }
            });
            ui.add_space(8.0);
        }

        // --- Cuadrícula Bento 2×2 limpia (sin superposición de texto jamás) ---
        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                ui.columns(2, |cols| {
                    // TARJETA 1: Archivos del juego
                    theme::bento_card().show(&mut cols[0], |ui| {
                        ui.horizontal(|ui| {
                            theme::paint_badge_icon(
                                ui,
                                theme::BadgeKind::Folder,
                                if self.game.ready() {
                                    theme::STATUS_OK
                                } else {
                                    theme::STATUS_WARN
                                },
                            );
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new("Archivos del juego").strong().size(14.5));
                                ui.label(
                                    RichText::new("Tu copia legal del juego de consola")
                                        .color(theme::TEXT_MUTED)
                                        .size(11.5),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if self.game.ready() {
                                    theme::status_pill(ui, "Listo", theme::STATUS_OK);
                                } else {
                                    theme::status_pill(ui, "Falta carpeta", theme::STATUS_WARN);
                                }
                            });
                        });
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(self.game.summary_es())
                                .color(theme::TEXT_SECONDARY)
                                .size(12.0),
                        );
                        if !self.cfg.game_dir.is_empty() {
                            ui.label(
                                RichText::new(&self.cfg.game_dir)
                                    .color(theme::TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            if ui.button("Buscar automáticamente").clicked() {
                                self.auto_search_game(ctx);
                            }
                            if ui.button("Elegir carpeta").clicked() {
                                self.pick_game_folder(ctx);
                            }
                        });
                    });

                    // TARJETA 2: Preparación de gráficos (Shaders + Escenas)
                    theme::bento_card().show(&mut cols[1], |ui| {
                        let ready = self.cache.shaders_ready();
                        ui.horizontal(|ui| {
                            theme::paint_badge_icon(
                                ui,
                                theme::BadgeKind::Bolt,
                                if ready {
                                    theme::QUANTICO_BLUE_LIGHT
                                } else {
                                    theme::STATUS_WARN
                                },
                            );
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new("Gráficos pre-cargados").strong().size(14.5));
                                ui.label(
                                    RichText::new("Evita congelamientos y tirones al jugar")
                                        .color(theme::TEXT_MUTED)
                                        .size(11.5),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ready && self.cache.pipeline_lib_exists {
                                    theme::status_pill(ui, "Completo", theme::STATUS_OK);
                                } else if ready {
                                    theme::status_pill(ui, "Shaders listos", theme::QUANTICO_BLUE_LIGHT);
                                } else {
                                    theme::status_pill(ui, "Pendiente", theme::STATUS_WARN);
                                }
                            });
                        });
                        ui.add_space(8.0);
                        let shaders_txt = if ready {
                            format!(
                                "Shaders: {} listos ({} MB) · Escenas pre-calentadas: {}",
                                self.cache.shader_files,
                                self.cache.shader_mb,
                                if self.cache.pipeline_lib_exists {
                                    format!("{} MB", self.cache.pipeline_lib_mb)
                                } else if self.cache.early_pipelines_count > 0 {
                                    format!("{} listas para acelerar", self.cache.early_pipelines_count)
                                } else {
                                    "se crean tras entrar 1 vez al juego".to_owned()
                                }
                            )
                        } else {
                            "Aún no preparados. Ejecútalo 1 sola vez (2–6 min) dejando 2 núcleos libres para tu PC.".to_owned()
                        };
                        ui.label(RichText::new(shaders_txt).color(theme::TEXT_SECONDARY).size(12.0));
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
                            let prep_btn = Button::new(
                                RichText::new("Preparar gráficos (1 clic)")
                                    .strong()
                                    .color(Color32::WHITE),
                            )
                            .fill(theme::QUANTICO_BLUE_STRONG);
                            if ui
                                .add_enabled(!is_running && self.game.ready(), prep_btn)
                                .clicked()
                            {
                                if let Err(e) =
                                    self.runner.start_prep(&self.engine, &self.cfg, TaskKind::PrepareAll)
                                {
                                    self.set_toast(e, theme::STATUS_ERR);
                                }
                            }
                            if self.cache.early_pipelines_count > 0
                                && ui
                                    .add_enabled(!is_running, Button::new("Acelerar escenas"))
                                    .clicked()
                            {
                                if let Err(e) =
                                    self.runner.start_prep(&self.engine, &self.cfg, TaskKind::Warm)
                                {
                                    self.set_toast(e, theme::STATUS_ERR);
                                }
                            }
                        });
                    });
                });

                ui.add_space(10.0);

                ui.columns(2, |cols| {
                    // TARJETA 3: Perfil rápido de calidad (rejilla ordenada, cero solapamientos)
                    theme::bento_card().show(&mut cols[0], |ui| {
                        ui.horizontal(|ui| {
                            theme::paint_badge_icon(
                                ui,
                                theme::BadgeKind::Sliders,
                                theme::QUANTICO_BLUE_LIGHT,
                            );
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new("Calidad y Rendimiento").strong().size(14.5));
                                ui.label(
                                    RichText::new("Elige cómo quieres priorizar FPS vs nitidez")
                                        .color(theme::TEXT_MUTED)
                                        .size(11.5),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                theme::status_pill(
                                    ui,
                                    self.cfg.preset.label_es(),
                                    theme::QUANTICO_BLUE_LIGHT,
                                );
                            });
                        });
                        ui.add_space(8.0);
                        ui.horizontal_wrapped(|ui| {
                            ui.spacing_mut().item_spacing = vec2(6.0, 6.0);
                            for preset in [
                                Preset::Auto,
                                Preset::Fluid720p,
                                Preset::Balanced1080p,
                                Preset::Native1080p,
                                Preset::High1440p,
                                Preset::Fsr4K,
                                Preset::Safe,
                            ] {
                                let active = self.cfg.preset == preset;
                                let btn = Button::new(
                                    RichText::new(preset.label_es())
                                        .size(12.0)
                                        .color(if active { Color32::WHITE } else { theme::TEXT_SECONDARY })
                                        .strong(),
                                )
                                .fill(if active { theme::QUANTICO_BLUE } else { theme::BG_CARD_ALT })
                                .stroke(Stroke::new(
                                    1.0_f32,
                                    if active { theme::QUANTICO_BLUE_LIGHT } else { theme::BORDER },
                                ))
                                .corner_radius(CornerRadius::same(7));
                                if ui.add(btn).clicked() {
                                    self.cfg.apply_preset(preset);
                                    self.persist();
                                }
                            }
                        });
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new(self.cfg.preset.description_es(&self.hw))
                                .color(theme::TEXT_SECONDARY)
                                .size(11.8),
                        );
                    });

                    // TARJETA 4: Control y Teclado
                    let pad = self.controllers.poll().clone();
                    theme::bento_card().show(&mut cols[1], |ui| {
                        ui.horizontal(|ui| {
                            theme::paint_badge_icon(
                                ui,
                                theme::BadgeKind::Pad,
                                if pad.any() || self.cfg.kbm_gta {
                                    theme::STATUS_OK
                                } else {
                                    theme::TEXT_MUTED
                                },
                            );
                            ui.vertical(|ui| {
                                ui.spacing_mut().item_spacing.y = 2.0;
                                ui.label(RichText::new("Control y Teclado").strong().size(14.5));
                                ui.label(
                                    RichText::new("Compatible con Xbox, PlayStation y PC")
                                        .color(theme::TEXT_MUTED)
                                        .size(11.5),
                                );
                            });
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                let badge = if self.cfg.kbm_gta {
                                    "Teclado + Ratón".to_owned()
                                } else {
                                    pad.badge_es()
                                };
                                let col = if pad.any() || self.cfg.kbm_gta {
                                    theme::STATUS_OK
                                } else {
                                    theme::TEXT_MUTED
                                };
                                theme::status_pill(ui, &badge, col);
                            });
                        });
                        ui.add_space(8.0);
                        ui.label(
                            RichText::new(pad.summary_es())
                                .color(theme::TEXT_SECONDARY)
                                .size(12.0),
                        );
                        ui.add_space(8.0);
                        ui.horizontal(|ui| {
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
                        if self.hw.low_ram_auto() {
                            ui.add_space(4.0);
                            ui.label(
                                RichText::new("Modo de ahorro de RAM activado automáticamente para tu memoria.")
                                    .color(theme::QUANTICO_BLUE_LIGHT)
                                    .size(11.5),
                            );
                        }
                    });
                });
            });
    }

    fn ui_configuracion(&mut self, ui: &mut egui::Ui) {
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Configuración").strong().size(22.0));
                ui.label(
                    RichText::new("Ajustes claros arriba y control total para usuarios expertos abajo.")
                        .color(theme::TEXT_SECONDARY)
                        .size(12.5),
                );
            });
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Restaurar recomendados").clicked() {
                    let saved_dir = self.cfg.game_dir.clone();
                    self.cfg = LauncherConfig::default();
                    self.cfg.game_dir = saved_dir;
                    changed = true;
                    self.set_toast("Ajustes restaurados al perfil recomendado.", theme::STATUS_OK);
                }
            });
        });
        ui.add_space(10.0);

        ScrollArea::vertical()
            .auto_shrink([false, false])
            .show(ui, |ui| {
                // --- Bloque 1: Ajustes fáciles ---
                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Pantalla y Calidad Gráfica")
                            .strong()
                            .color(theme::QUANTICO_BLUE_LIGHT)
                            .size(14.5),
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
                            ui.add(
                                egui::Slider::new(&mut self.cfg.fps_cap, 0..=120)
                                    .suffix(" FPS"),
                            )
                            .changed()
                            .then(|| changed = true);
                            ui.end_row();

                            ui.label("Escala interna (0 = automático):");
                            ui.add(
                                egui::Slider::new(&mut self.cfg.render_scale, 0.0..=1.0)
                                    .fixed_decimals(2),
                            )
                            .changed()
                            .then(|| {
                                self.cfg.preset = Preset::Custom;
                                changed = true;
                            });
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

                    if self.cfg.fsr_4k != FsrQuality::Off && !self.cache.ffx_dll_exists {
                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Falta amd_fidelityfx_upscaler_dx12.dll en %LOCALAPPDATA%\\victory\\fidelityfx para usar FSR 4K.")
                                .color(theme::STATUS_WARN)
                                .size(11.5),
                        );
                    }

                    ui.add_space(6.0);
                    if ui
                        .checkbox(
                            &mut self.cfg.smooth_motion,
                            "NVIDIA Smooth Motion (duplica FPS visuales usando victory_sm.exe en NVIDIA App)",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .checkbox(
                            &mut self.cfg.tex_store,
                            "Guardar caché de texturas en disco (acelera al volver a zonas ya visitadas)",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                });

                ui.add_space(10.0);

                // --- Bloque 2: Controles ---
                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Teclado, Ratón y Escenas")
                            .strong()
                            .color(theme::QUANTICO_BLUE_LIGHT)
                            .size(14.5),
                    );
                    ui.add_space(6.0);
                    if ui
                        .checkbox(
                            &mut self.cfg.kbm_gta,
                            "Activar controles de PC estilo GTA (F2 bloquea/desbloquea la cámara con ratón)",
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
                    if ui
                        .checkbox(
                            &mut self.cfg.video_planes,
                            "Pantallas y videos en cinemáticas (teléfono encendido, TVs e intros)",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                    if ui
                        .checkbox(
                            &mut self.cfg.live_textures,
                            "Actualización en vivo de texturas dinámicas (evita pantallas congeladas)",
                        )
                        .changed()
                    {
                        changed = true;
                    }
                });

                ui.add_space(10.0);

                // --- Bloque 3: Opciones avanzadas (desplegable para no abrumar a principiantes) ---
                theme::bento_card().show(ui, |ui| {
                    egui::CollapsingHeader::new(
                        RichText::new("Opciones avanzadas (para usuarios expertos y pruebas)")
                            .strong()
                            .size(14.0),
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
                            ui.add_space(16.0);
                            ui.label("Perfil ahorro de RAM (<=24 GB):");
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

                        ui.add_space(4.0);
                        ui.horizontal(|ui| {
                            ui.label("Ventana ReBAR VRAM en MB (0 = automático):");
                            if ui
                                .add(egui::DragValue::new(&mut self.cfg.windows_bar_mb_override).range(0..=16384))
                                .changed()
                            {
                                self.cfg.preset = Preset::Custom;
                                changed = true;
                            }
                        });

                        ui.add_space(4.0);
                        if ui
                            .checkbox(
                                &mut self.cfg.port_rt,
                                "Port RT (elimina esperas mem_read en RTX 30/40/50 — muy recomendado)",
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(
                                &mut self.cfg.skip_rt_resolve,
                                "Omitir CS_RaytracingResolve cuando Ray Tracing está apagado (+FPS)",
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(
                                &mut self.cfg.skip_raymarched_lights,
                                "Omitir luces locales raymarched sin BVH (+FPS en RTX)",
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(
                                &mut self.cfg.ambient_light,
                                "Iluminación ambiental sustituta y filtro anti-cuadros magenta",
                            )
                            .changed()
                        {
                            changed = true;
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
                        if ui
                            .checkbox(
                                &mut self.cfg.port_descriptors,
                                "Port 2 experimental (VICTORY_PORT_DESCRIPTORS=1 — desactivado por defecto)",
                            )
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(&mut self.cfg.send_bar, "VICTORY_SEND_BAR (memoria de envío en VRAM)")
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(&mut self.cfg.wc_no_back, "VICTORY_WC_NO_BACK (saltar readback de buffers grandes)")
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(&mut self.cfg.wait_textures, "Esperar carga completa de texturas (más lento, menos pop-in)")
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(&mut self.cfg.record_pipelines, "Grabar catálogo de pipelines (VICTORY_PIPELINE_BOOK=1)")
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(&mut self.cfg.use_diag_build, "Usar compilación de diagnóstico (diag/victory.exe)")
                            .changed()
                        {
                            changed = true;
                        }
                        if ui
                            .checkbox(&mut self.cfg.gpu_timing, "Medir tiempos GPU por shader (VICTORY_GPU_TIMING=1)")
                            .changed()
                        {
                            changed = true;
                        }

                        ui.add_space(6.0);
                        ui.label(
                            RichText::new("Variables de entorno extra (una por línea, ej. VICTORY_FPS=30):")
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

    fn ui_logs(&mut self, ui: &mut egui::Ui, ctx: &egui::Context, snap: &runner::RunnerSnapshot) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                ui.label(RichText::new("Logs y Soporte").strong().size(22.0));
                ui.label(
                    RichText::new(
                        "Comparte tus logs en 1 clic sin buscar carpetas ni exponer tu nombre de usuario.",
                    )
                    .color(theme::TEXT_SECONDARY)
                    .size(12.5),
                );
            });
        });
        ui.add_space(8.0);

        // --- Barra gigante de 3 acciones claras (nadie vuelve a confundirse para pasar logs) ---
        theme::bento_card().show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                let copy_btn = Button::new(
                    RichText::new("Copiar reporte para enviar")
                        .strong()
                        .color(Color32::WHITE)
                        .size(13.0),
                )
                .fill(theme::QUANTICO_BLUE)
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
                        "Reporte copiado al portapapeles (sin datos privados). ¡Pégalo en el chat!",
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
                .stroke(Stroke::new(1.0_f32, theme::QUANTICO_BLUE_LIGHT))
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

                let open_btn = Button::new(
                    RichText::new("Abrir carpeta de logs").size(13.0),
                )
                .min_size(vec2(170.0, 36.0));
                if ui.add(open_btn).clicked() {
                    runner::open_folder(&runner::data_dir());
                }
            });
        });

        ui.add_space(10.0);

        // --- Panel de 2 columnas: consola a la izquierda, última partida y archivos a la derecha ---
        let avail_h = (ui.available_height() - 6.0).max(260.0);
        ui.columns(2, |cols| {
            // Izquierda: Consola en vivo o archivo seleccionado.
            cols[0].vertical(|ui| {
                ui.horizontal(|ui| {
                    let title = match &self.viewing_file_name {
                        Some(name) => format!("Viendo archivo: {name}"),
                        None => format!("REGISTRO EN VIVO ({} líneas)", snap.logs.len()),
                    };
                    ui.label(
                        RichText::new(title)
                            .strong()
                            .color(theme::QUANTICO_BLUE_LIGHT)
                            .size(12.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.log_filter)
                                .hint_text("Buscar (ej: fps, error, warn)...")
                                .desired_width(180.0),
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
                            .max_height((avail_h - 46.0).max(180.0))
                            .stick_to_bottom(self.viewing_file_name.is_none())
                            .auto_shrink([false, false])
                            .show(ui, |ui| {
                                if source.is_empty() {
                                    ui.label(
                                        RichText::new(
                                            "Aquí aparecerán los FPS y mensajes del motor en tiempo real cuando pulses Jugar.",
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

            // Derecha: Última partida + archivos recientes.
            cols[1].vertical(|ui| {
                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Resumen de la última partida")
                            .strong()
                            .color(theme::QUANTICO_BLUE_LIGHT)
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
                                RichText::new(format!("Archivo guardado: {name}"))
                                    .color(theme::TEXT_MUTED)
                                    .size(11.0),
                            );
                        }
                    } else {
                        ui.label(
                            RichText::new("Juega una partida para ver aquí tus FPS promedio, mínimo, máximo y uso de memoria.")
                                .color(theme::TEXT_SECONDARY)
                                .size(12.0),
                        );
                    }
                });

                ui.add_space(10.0);

                theme::bento_card().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(
                            RichText::new("Partidas y cierres recientes")
                                .strong()
                                .size(13.5),
                        );
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if ui.button("Recargar").clicked() {
                                self.log_files = runner::scan_data_logs();
                            }
                        });
                    });
                    ui.add_space(6.0);
                    ScrollArea::vertical()
                        .max_height((avail_h - 195.0).max(120.0))
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

        ui.label(RichText::new("Actualizaciones y Créditos").strong().size(22.0));
        ui.label(
            RichText::new(
                "Mantén VIctory-RTX al día desde GitHub en 1 clic sin borrar tus partidas ni tu caché de shaders.",
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
                                RichText::new("Actualización automática por Git")
                                    .strong()
                                    .color(theme::QUANTICO_BLUE_LIGHT)
                                    .size(15.0),
                            );
                            if git_info.available {
                                ui.label(
                                    RichText::new(format!(
                                        "Rama activa: {}  ·  Versión instalada: {}",
                                        git_info.branch, git_info.local_sha
                                    ))
                                    .color(theme::TEXT_SECONDARY)
                                    .size(12.5),
                                );
                            } else {
                                ui.label(
                                    RichText::new("Carpeta portable sin Git detectado.")
                                        .color(theme::TEXT_SECONDARY)
                                        .size(12.5),
                                );
                            }
                        });
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            if checking {
                                theme::status_pill(ui, "Comprobando...", theme::QUANTICO_BLUE_LIGHT);
                            } else if git_info.behind_count > 0 {
                                theme::status_pill(
                                    ui,
                                    &format!("{} nueva(s)", git_info.behind_count),
                                    theme::STATUS_OK,
                                );
                            } else if git_info.checked_once {
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
                                "Comprobando GitHub..."
                            } else {
                                "Buscar actualizaciones"
                            })
                            .strong(),
                        )
                        .min_size(vec2(180.0, 36.0));
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
                            .fill(theme::QUANTICO_BLUE)
                            .corner_radius(CornerRadius::same(8))
                            .min_size(vec2(245.0, 36.0));
                            if ui.add_enabled(!is_running && !checking, pull_btn).clicked() {
                                if let Err(e) = self.runner.start_git_update(root, true) {
                                    self.set_toast(e, theme::STATUS_ERR);
                                } else {
                                    self.set_toast(
                                        "Descargando y compilando actualización en segundo plano...",
                                        theme::QUANTICO_BLUE_LIGHT,
                                    );
                                }
                            }
                        }

                        if ui.button("Abrir repositorio en GitHub").clicked() {
                            runner::open_url("https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan");
                        }
                    });
                });

                ui.add_space(10.0);

                theme::bento_card().show(ui, |ui| {
                    ui.label(
                        RichText::new("Créditos y Licencia Open Source (GPL-3.0-or-later)")
                            .strong()
                            .color(theme::QUANTICO_BLUE_LIGHT)
                            .size(14.5),
                    );
                    ui.add_space(6.0);
                    ui.label(
                        RichText::new(
                            "VIctory-RTX-Mc3yohan · Interfaz diseñada con la identidad Quantico Systems. \
                             Este proyecto no incluye archivos, ejecutables ni imágenes con derechos de autor del juego.",
                        )
                        .color(theme::TEXT_SECONDARY)
                        .size(12.0),
                    );
                    ui.add_space(6.0);
                    for (name, desc, url) in [
                        (
                            "mc3yohan-lab / VIctory-RTX-Mc3yohan",
                            "Fork NVIDIA RTX Wave32 + Launcher Quantico Systems",
                            "https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan",
                        ),
                        (
                            "Marvisak / VIctory",
                            "Proyecto base open-source GPL-3.0-or-later",
                            "https://github.com/Marvisak/VIctory",
                        ),
                        (
                            "TaynanGT / Vice-Tay",
                            "Fixes de alias GPU, colas de texturas, reflejos y optimizaciones RTX 4090",
                            "https://github.com/TaynanGT/Vice-Tay",
                        ),
                        (
                            "ZigZagBoi1 / ScarlettPort",
                            "Planos de video en cinemáticas (teléfono/TV), BCPack, formatos y bloqueo compartido",
                            "https://github.com/ZigZagBoi1/ScarlettPort",
                        ),
                        (
                            "avatarmanz / VIctory-7900XTX",
                            "Renderer Ports 1–7, Live Textures #390 y mejoras Wave32",
                            "https://github.com/avatarmanz/VIctory-7900XTX",
                        ),
                        (
                            "Comunidad (fxceless, Peppacodes, VIctory-DXR, Yaekai, Belobug, RAM16)",
                            "Mejoras de FPS, memoria baja 16–24 GB, controles PC y estabilidad",
                            "https://github.com/fxceless/victory-fork",
                        ),
                    ] {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(name).strong().size(12.5));
                            ui.label(RichText::new(format!("— {desc}")).color(theme::TEXT_MUTED).size(11.8));
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
        let snap = self.runner.snapshot();

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
            // Refrescar estado local de Git tras actualizar.
            if let Some(root) = self.engine.repo_root.as_deref() {
                *self.git.lock().unwrap_or_else(|e| e.into_inner()) =
                    GitInfo::probe_local(Some(root));
            }
        }

        if snap.running.is_some() || *self.git_checking.lock().unwrap_or_else(|e| e.into_inner()) {
            ctx.request_repaint_after(Duration::from_millis(120));
        } else {
            ctx.request_repaint_after(Duration::from_millis(800));
        }

        // Cabecera superior Quantico.
        egui::TopBottomPanel::top("quantico_header")
            .frame(Frame::NONE)
            .show(ctx, |ui| {
                self.ui_top_header(ui, &snap);
            });

        // Toast inferior discreto (desaparece solo a los 6s).
        if let Some((msg, col, at)) = self.toast.clone() {
            if at.elapsed() > Duration::from_secs(6) {
                self.toast = None;
            } else {
                egui::TopBottomPanel::bottom("quantico_toast")
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
            .frame(Frame::new().fill(theme::BG_CANVAS).inner_margin(Margin::same(16)))
            .show(ctx, |ui| match self.tab {
                Tab::Inicio => self.ui_inicio(ui, ctx, &snap),
                Tab::Configuracion => self.ui_configuracion(ui),
                Tab::Logs => self.ui_logs(ui, ctx, &snap),
                Tab::Actualizar => self.ui_actualizar(ui, &snap),
            });
    }
}

fn main() -> eframe::Result<()> {
    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_title("VIctory-RTX · Quantico Systems")
            .with_inner_size([1120.0, 740.0])
            .with_min_inner_size([920.0, 620.0])
            .with_icon(art::app_icon()),
        ..Default::default()
    };

    eframe::run_native(
        "VIctory-RTX · Quantico Systems",
        options,
        Box::new(|cc| Ok(Box::new(VictoryLauncherApp::new(cc)))),
    )
}
