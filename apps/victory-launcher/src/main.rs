//! VIctory-RTX-Mc3yohan — Native Windows Desktop Launcher (eframe / egui).
//!
//! Zero PowerShell, zero manual env-var setup: pick your own game dump folder,
//! build Wave32 shaders / warm pipelines with live progress, choose a preset,
//! monitor live logs/FPS/crashes, and press PLAY.

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod art;
mod config;
mod hardware;
mod runner;
mod theme;

use std::path::Path;
use std::sync::Arc;
use std::time::Duration;

use eframe::egui::{
    self, Align, Button, Color32, CornerRadius, Layout, Margin, ProgressBar, RichText, ScrollArea,
    Stroke, TextureHandle, TextureOptions, Vec2,
};

use config::{
    ConsoleModel, FsrMode, GameFilesStatus, LauncherConfig, OutputRes, Preset, TriState, cache_dir,
    data_dir,
};
use hardware::{HardwareInfo, detected_controllers};
use runner::{CacheStatus, EnginePaths, LogFileEntry, ProcessRunner, TaskKind};
use theme::{NavIcon, RowIcon};

#[derive(Clone, Copy, PartialEq, Eq)]
enum Tab {
    Play,
    Shaders,
    Logs,
    Settings,
    Links,
}

struct LauncherApp {
    tab: Tab,
    engine: EnginePaths,
    hw: HardwareInfo,
    cfg: LauncherConfig,
    game_files: GameFilesStatus,
    cache: CacheStatus,
    runner: ProcessRunner,
    last_finished_seq: u64,
    // Artwork texture loaded from user's local game folder (or procedural fallback).
    hero_tex: Option<TextureHandle>,
    hero_loaded_for: String,
    // UI state
    prep_jobs: usize,
    log_filter: String,
    auto_scroll: bool,
    log_entries: Vec<LogFileEntry>,
    selected_crash_preview: String,
    toast: Option<String>,
}

impl LauncherApp {
    fn new(cc: &eframe::CreationContext<'_>) -> Self {
        theme::apply(&cc.egui_ctx);
        let engine = EnginePaths::discover();
        let hw = HardwareInfo::probe();
        let cfg = LauncherConfig::load(engine.repo_root.as_deref());
        let game_files = GameFilesStatus::inspect(Path::new(&cfg.game_dir));
        let cache = CacheStatus::scan();
        let prep_jobs = hw.recommended_shader_jobs();
        let log_entries = runner::scan_data_logs();

        let mut app = Self {
            tab: Tab::Play,
            engine,
            hw,
            cfg,
            game_files,
            cache,
            runner: ProcessRunner::new(),
            last_finished_seq: 0,
            hero_tex: None,
            hero_loaded_for: String::new(),
            prep_jobs,
            log_filter: String::new(),
            auto_scroll: true,
            log_entries,
            selected_crash_preview: String::new(),
            toast: None,
        };
        app.reload_hero(&cc.egui_ctx);
        app
    }

    fn reload_hero(&mut self, ctx: &egui::Context) {
        let img = if !self.cfg.game_dir.is_empty() {
            art::from_game(Path::new(&self.cfg.game_dir)).unwrap_or_else(art::fallback_banner)
        } else {
            art::fallback_banner()
        };
        self.hero_tex = Some(ctx.load_texture("hero_banner", img, TextureOptions::LINEAR));
        self.hero_loaded_for = self.cfg.game_dir.clone();
    }

    fn pick_game_folder(&mut self, ctx: &egui::Context) {
        let mut dialog = rfd::FileDialog::new().set_title("Select Xbox Series X|S Game Dump Folder");
        if !self.cfg.game_dir.is_empty() {
            let p = Path::new(&self.cfg.game_dir);
            if p.is_dir() {
                dialog = dialog.set_directory(p);
            }
        }
        if let Some(folder) = dialog.pick_folder() {
            self.cfg.game_dir = folder.to_string_lossy().into_owned();
            self.cfg.save(self.engine.repo_root.as_deref());
            self.game_files = GameFilesStatus::inspect(&folder);
            self.reload_hero(ctx);
            self.toast = if self.game_files.ready() {
                Some("Game folder verified and saved to victory.toml.".into())
            } else {
                Some("Folder selected, but no .exe was found inside.".into())
            };
        }
    }

    fn refresh_status(&mut self) {
        self.game_files = GameFilesStatus::inspect(Path::new(&self.cfg.game_dir));
        self.cache = CacheStatus::scan();
        self.log_entries = runner::scan_data_logs();
    }

    fn save_cfg(&self) {
        self.cfg.save(self.engine.repo_root.as_deref());
    }

    fn trigger_play(&mut self) {
        if self.runner.running().is_some() {
            self.runner.stop();
            return;
        }
        let Some(exe) = self.game_files.exe_path.clone() else {
            self.toast = Some("Select your game folder first (Change Game Folder).".into());
            return;
        };
        self.save_cfg();
        match self.runner.start_play(&self.engine, &self.cfg, &self.hw, &exe) {
            Ok(()) => self.toast = Some("Launching VIctory-RTX-Mc3yohan...".into()),
            Err(e) => self.toast = Some(e),
        }
    }

    fn trigger_prep(&mut self, kind: TaskKind) {
        if self.runner.running().is_some() {
            self.runner.stop();
            return;
        }
        self.save_cfg();
        match self.runner.start_prep(kind, &self.engine, &self.cfg, self.prep_jobs) {
            Ok(()) => {
                self.toast = Some(format!("{} started...", kind.label()));
            }
            Err(e) => self.toast = Some(e),
        }
    }
}

impl eframe::App for LauncherApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        let running = self.runner.running();
        let seq = self.runner.finished_seq();
        if seq != self.last_finished_seq {
            self.last_finished_seq = seq;
            self.refresh_status();
            if let Some(note) = self.runner.last_exit_note() {
                self.toast = Some(note);
            }
        }
        if running.is_some() {
            ctx.request_repaint_after(Duration::from_millis(120));
        } else {
            // Light 2s heartbeat for controller hot-plug updates.
            ctx.request_repaint_after(Duration::from_secs(2));
        }

        // Left navigation sidebar
        egui::SidePanel::left("nav_rail")
            .exact_width(64.0)
            .resizable(false)
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_SIDEBAR)
                    .stroke(Stroke::new(1.0_f32, theme::BORDER))
                    .inner_margin(Margin::symmetric(10, 14)),
            )
            .show(ctx, |ui| {
                ui.vertical_centered(|ui| {
                    let (badge_rect, _) = ui.allocate_exact_size(Vec2::new(36.0, 36.0), egui::Sense::hover());
                    theme::draw_brand_badge(ui.painter(), badge_rect);
                    ui.add_space(18.0);

                    self.nav_button(ui, Tab::Play, NavIcon::Play, "Play");
                    ui.add_space(6.0);
                    self.nav_button(ui, Tab::Shaders, NavIcon::Shaders, "Shaders & Pipelines");
                    ui.add_space(6.0);
                    self.nav_button(ui, Tab::Logs, NavIcon::Logs, "Logs & Crashes");
                    ui.add_space(6.0);
                    self.nav_button(ui, Tab::Links, NavIcon::Links, "Links & Credits");
                });

                ui.with_layout(Layout::bottom_up(Align::Center), |ui| {
                    ui.label(RichText::new("v0.6.0").color(theme::TEXT_DIM).size(10.5));
                    ui.label(RichText::new("RTX").color(theme::EMERALD).strong().size(10.0));
                    ui.add_space(10.0);
                    self.nav_button(ui, Tab::Settings, NavIcon::Settings, "Settings");
                });
            });

        // Main content area
        egui::CentralPanel::default()
            .frame(
                egui::Frame::new()
                    .fill(theme::BG_CANVAS)
                    .inner_margin(Margin::symmetric(20, 16)),
            )
            .show(ctx, |ui| {
                match self.tab {
                    Tab::Play => self.ui_play(ui, ctx),
                    Tab::Shaders => self.ui_shaders(ui),
                    Tab::Logs => self.ui_logs(ui),
                    Tab::Settings => self.ui_settings(ui, ctx),
                    Tab::Links => self.ui_links(ui),
                }
            });
    }
}

impl LauncherApp {
    fn nav_button(&mut self, ui: &mut egui::Ui, tab: Tab, icon: NavIcon, tooltip: &str) {
        let active = self.tab == tab;
        let (rect, resp) = ui.allocate_exact_size(Vec2::new(42.0, 42.0), egui::Sense::click());
        let bg = if active {
            theme::ACCENT_RED_DIM
        } else if resp.hovered() {
            theme::BG_CARD_ALT
        } else {
            Color32::TRANSPARENT
        };
        let fg = if active {
            theme::TEXT_PRIMARY
        } else if resp.hovered() {
            theme::TEXT_PRIMARY
        } else {
            theme::TEXT_SECONDARY
        };
        ui.painter().rect_filled(rect, CornerRadius::same(9), bg);
        if active {
            let bar = egui::Rect::from_min_size(
                egui::pos2(rect.left() - 10.0, rect.top() + 8.0),
                Vec2::new(3.0, rect.height() - 16.0),
            );
            ui.painter().rect_filled(bar, CornerRadius::same(2), theme::ACCENT_RED);
        }
        theme::draw_nav_icon(ui.painter(), icon, rect.center(), fg);
        if resp.clicked() {
            self.tab = tab;
            if tab == Tab::Logs {
                self.log_entries = runner::scan_data_logs();
            }
        }
        resp.on_hover_text(tooltip);
    }

    fn ui_play(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let running = self.runner.running();
        let controllers = detected_controllers();

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // --- Hero Banner Card ---
            let hero_w = ui.available_width();
            let hero_h = 186.0;
            let (hero_rect, _) = ui.allocate_exact_size(Vec2::new(hero_w, hero_h), egui::Sense::hover());
            let painter = ui.painter();

            if let Some(tex) = &self.hero_tex {
                let tex_size = tex.size_vec2();
                let img_aspect = tex_size.x / tex_size.y.max(1.0);
                let box_aspect = hero_rect.width() / hero_rect.height().max(1.0);
                let uv = if img_aspect > box_aspect {
                    let frac = box_aspect / img_aspect;
                    let pad = (1.0 - frac) * 0.5;
                    egui::Rect::from_min_max(egui::pos2(pad, 0.0), egui::pos2(1.0 - pad, 1.0))
                } else {
                    let frac = img_aspect / box_aspect;
                    let pad = (1.0 - frac) * 0.5;
                    egui::Rect::from_min_max(egui::pos2(0.0, pad), egui::pos2(1.0, 1.0 - pad))
                };
                painter.image(tex.id(), hero_rect, uv, Color32::WHITE);
            } else {
                painter.rect_filled(hero_rect, CornerRadius::same(12), theme::BG_CARD);
            }

            // Dark scrim for guaranteed high-contrast text over any splash image.
            painter.rect_filled(
                hero_rect,
                CornerRadius::same(12),
                Color32::from_rgba_unmultiplied(10, 12, 18, 155),
            );
            let bottom_scrim = egui::Rect::from_min_max(
                egui::pos2(hero_rect.left(), hero_rect.bottom() - 96.0),
                hero_rect.max,
            );
            painter.rect_filled(
                bottom_scrim,
                CornerRadius::same(12),
                Color32::from_rgba_unmultiplied(10, 12, 18, 175),
            );
            painter.rect_stroke(
                hero_rect,
                CornerRadius::same(12),
                Stroke::new(1.0_f32, theme::BORDER),
                egui::StrokeKind::Inside,
            );

            // Overlay widgets inside hero card.
            let inner = hero_rect.shrink2(Vec2::new(20.0, 16.0));
            ui.allocate_new_ui(egui::UiBuilder::new().max_rect(inner), |ui| {
                // Top row: tag left + Change Game Folder button right.
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new("NVIDIA RTX  ·  WAVE32  ·  SERIES X|S")
                            .color(theme::EMERALD)
                            .strong()
                            .size(11.0),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        let btn = Button::new(
                            RichText::new("CHANGE GAME FOLDER")
                                .strong()
                                .size(11.5)
                                .color(theme::TEXT_PRIMARY),
                        )
                        .fill(Color32::from_rgba_unmultiplied(18, 22, 32, 220))
                        .stroke(Stroke::new(1.0_f32, theme::BORDER_ACTIVE))
                        .corner_radius(CornerRadius::same(7))
                        .min_size(Vec2::new(165.0, 32.0));
                        if ui.add(btn).clicked() {
                            self.pick_game_folder(ctx);
                        }
                    });
                });

                // Bottom row: Title + subtitle left, giant PLAY / STOP CTA right.
                ui.with_layout(Layout::bottom_up(Align::Min), |ui| {
                    ui.horizontal(|ui| {
                        ui.vertical(|ui| {
                            ui.label(
                                RichText::new("VIctory-RTX-Mc3yohan")
                                    .color(theme::TEXT_PRIMARY)
                                    .strong()
                                    .size(26.0),
                            );
                            let sub = if let Some(kind) = running {
                                format!(
                                    "{} ({} s)  ·  {}",
                                    kind.label(),
                                    self.runner.elapsed_secs(),
                                    self.hw.summary_line()
                                )
                            } else {
                                self.hw.summary_line()
                            };
                            ui.label(RichText::new(sub).color(theme::TEXT_SECONDARY).size(12.5));
                        });

                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            let (btn_text, btn_color, enabled) = match running {
                                Some(TaskKind::Play) => ("■  STOP", Color32::from_rgb(185, 28, 28), true),
                                Some(_) => ("■  CANCEL", theme::AMBER, true),
                                None => {
                                    let ready = self.game_files.ready()
                                        && self.engine.active_exe(self.cfg.diag_build || self.cfg.gpu_timing).is_some();
                                    ("▶  PLAY", theme::ACCENT_RED, ready)
                                }
                            };
                            let play_btn = Button::new(
                                RichText::new(btn_text)
                                    .color(Color32::WHITE)
                                    .strong()
                                    .size(15.0),
                            )
                            .fill(if enabled { btn_color } else { theme::BG_CARD_ALT })
                            .corner_radius(CornerRadius::same(8))
                            .min_size(Vec2::new(152.0, 44.0));
                            if ui.add_enabled(enabled, play_btn).clicked() {
                                self.trigger_play();
                            }
                        });
                    });
                });
            });

            if let Some(toast) = self.toast.clone() {
                ui.add_space(6.0);
                egui::Frame::new()
                    .fill(Color32::from_rgba_unmultiplied(16, 185, 129, 26))
                    .stroke(Stroke::new(1.0_f32, Color32::from_rgba_unmultiplied(16, 185, 129, 90)))
                    .corner_radius(CornerRadius::same(8))
                    .inner_margin(Margin::symmetric(12, 7))
                    .show(ui, |ui| {
                        ui.horizontal(|ui| {
                            ui.label(RichText::new(&toast).color(theme::TEXT_PRIMARY).size(12.5));
                            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                if ui.small_button("Dismiss").clicked() {
                                    self.toast = None;
                                }
                            });
                        });
                    });
            }

            if let Some(kind) = running
                && kind != TaskKind::Play
            {
                let prog = self.runner.snapshot_progress();
                ui.add_space(6.0);
                theme::card_frame().show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label(RichText::new(&prog.step_label).strong().color(theme::CYAN));
                        ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                            ui.label(
                                RichText::new(format!("{} s elapsed", self.runner.elapsed_secs()))
                                    .color(theme::TEXT_SECONDARY)
                                    .size(12.0),
                            );
                        });
                    });
                    ui.add_space(4.0);
                    ui.add(
                        ProgressBar::new(prog.fraction())
                            .desired_height(8.0)
                            .show_percentage(),
                    );
                });
            }

            ui.add_space(10.0);

            // --- Row 1: Game files ---
            self.status_row(
                ui,
                RowIcon::Folder,
                "Game files",
                &self.game_files.detail_line(&self.cfg.game_dir),
                |ui| {
                    if self.game_files.ready() {
                        theme::status_pill(ui, "Ready", theme::EMERALD);
                    } else {
                        theme::status_pill(ui, "Select folder", theme::AMBER);
                    }
                },
            );
            ui.add_space(6.0);

            // --- Row 2: Engine & GPU Auto-Tier ---
            let want_diag = self.cfg.diag_build || self.cfg.gpu_timing;
            let engine_ok = self.engine.active_exe(want_diag).is_some();
            let ram16_note = match self.cfg.ram16_profile {
                TriState::On => " · RAM16 ON",
                TriState::Off => " · RAM16 OFF",
                TriState::Auto if self.hw.auto_ram16() => " · RAM16 Auto ON",
                TriState::Auto => "",
            };
            let engine_detail = format!(
                "{} build  ·  Tier {} (ReBAR {} MiB, MipDrop {})  ·  Thin={} {}",
                if want_diag { "Diagnostics" } else { "Release ThinLTO + mimalloc" },
                self.hw.vram_tier.label(),
                self.cfg.effective_windows_bar_mb(&self.hw),
                self.cfg.effective_mip_drop(&self.hw),
                if self.cfg.effective_thin(&self.hw) { "ON" } else { "OFF" },
                ram16_note,
            );
            self.status_row(ui, RowIcon::CpuGpu, "Engine & GPU Auto-Tier", &engine_detail, |ui| {
                if engine_ok {
                    theme::status_pill(ui, "Ready", theme::EMERALD);
                } else {
                    theme::status_pill(ui, "Build required", theme::AMBER);
                }
            });
            ui.add_space(6.0);

            // --- Row 3: Shaders ---
            let shader_detail = if self.cache.shaders_ready() {
                format!(
                    "{} Wave32 shaders cached ({} MB) — built for this runtime",
                    self.cache.shader_files, self.cache.shader_mb
                )
            } else {
                "Not built yet — run once (~2-10 min) so gameplay has zero shader compile stutters".to_owned()
            };
            let mut build_shaders_clicked = false;
            self.status_row(ui, RowIcon::Download, "Shaders", &shader_detail, |ui| {
                if self.cache.shaders_ready() {
                    theme::status_pill(ui, "Ready", theme::EMERALD);
                } else {
                    if ui
                        .add_enabled(
                            running.is_none() && self.game_files.ready(),
                            Button::new("Build Shaders"),
                        )
                        .clicked()
                    {
                        build_shaders_clicked = true;
                    }
                    theme::status_pill(ui, "Pending", theme::AMBER);
                }
            });
            if build_shaders_clicked {
                self.tab = Tab::Shaders;
                self.trigger_prep(TaskKind::Shaders);
            }
            ui.add_space(6.0);

            // --- Row 4: World pipelines ---
            let pipe_detail = if self.cache.pipeline_lib_exists {
                format!(
                    "host-pipelines.bin cached ({} MB) — instant PSO loads",
                    self.cache.pipeline_lib_mb
                )
            } else {
                "Optional: warm pipelines after your first run to remove driver hitching".to_owned()
            };
            let mut warm_clicked = false;
            self.status_row(ui, RowIcon::Layers, "World pipelines", &pipe_detail, |ui| {
                if self.cache.pipeline_lib_exists {
                    theme::status_pill(ui, "Ready", theme::EMERALD);
                } else {
                    if ui
                        .add_enabled(running.is_none() && self.cache.shaders_ready(), Button::new("Warm"))
                        .clicked()
                    {
                        warm_clicked = true;
                    }
                    theme::status_pill(ui, "Optional", theme::TEXT_SECONDARY);
                }
            });
            if warm_clicked {
                self.tab = Tab::Shaders;
                self.trigger_prep(TaskKind::Warm);
            }
            ui.add_space(6.0);

            // --- Row 5: Controller & Input ---
            let (ctrl_detail, ctrl_badge, ctrl_ok) = controllers.describe(self.cfg.kbm_gta);
            let mut toggle_kbm = false;
            self.status_row(ui, RowIcon::Gamepad, "Controller & Input", &ctrl_detail, |ui| {
                let kbm_btn = Button::new(
                    RichText::new(if self.cfg.kbm_gta { "KBM: ON" } else { "KBM: OFF" }).size(12.0),
                )
                .fill(if self.cfg.kbm_gta {
                    Color32::from_rgba_unmultiplied(16, 185, 129, 55)
                } else {
                    theme::BG_CARD_ALT
                });
                if ui.add(kbm_btn).on_hover_text("Toggle GTA PC Keyboard + Mouse-Look (F2 / ~)").clicked() {
                    toggle_kbm = true;
                }
                if ctrl_ok {
                    theme::status_pill(ui, ctrl_badge, theme::EMERALD);
                } else {
                    ui.label(RichText::new(ctrl_badge).color(theme::TEXT_DIM).size(13.0));
                }
            });
            if toggle_kbm {
                self.cfg.kbm_gta = !self.cfg.kbm_gta;
                self.save_cfg();
            }
            ui.add_space(6.0);

            // --- Row 6: Preset selector ---
            let mut picked_preset = None;
            let preset_desc = self.cfg.preset.summary();
            self.status_row(ui, RowIcon::Sliders, "Preset", preset_desc, |ui| {
                for preset in Preset::ALL {
                    let active = self.cfg.preset == preset;
                    let btn = Button::new(
                        RichText::new(preset.label())
                            .color(if active { Color32::WHITE } else { theme::TEXT_SECONDARY })
                            .strong()
                            .size(12.0),
                    )
                    .fill(if active { theme::ACCENT_RED } else { theme::BG_CARD_ALT })
                    .corner_radius(CornerRadius::same(6));
                    if ui.add(btn).clicked() {
                        picked_preset = Some(preset);
                    }
                }
            });
            if let Some(p) = picked_preset {
                self.cfg.apply_preset(p);
                self.save_cfg();
                if p == Preset::Custom {
                    self.tab = Tab::Settings;
                }
            }
        });
    }

    fn status_row(
        &self,
        ui: &mut egui::Ui,
        icon: RowIcon,
        title: &str,
        detail: &str,
        right: impl FnOnce(&mut egui::Ui),
    ) {
        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                theme::draw_row_icon(ui, icon);
                ui.add_space(6.0);
                ui.vertical(|ui| {
                    ui.label(RichText::new(title).color(theme::TEXT_PRIMARY).strong().size(14.0));
                    ui.label(RichText::new(detail).color(theme::TEXT_SECONDARY).size(12.0));
                });
                ui.with_layout(Layout::right_to_left(Align::Center), right);
            });
        });
    }

    fn ui_shaders(&mut self, ui: &mut egui::Ui) {
        let running = self.runner.running();
        let prog = self.runner.snapshot_progress();

        ui.label(RichText::new("Shaders & Pipeline Library").strong().size(22.0));
        ui.label(
            RichText::new(
                "Pre-translate all ~26,084 console shaders to NVIDIA Wave32 DXIL once, then warm your D3D12 pipeline library so gameplay stays smooth.",
            )
            .color(theme::TEXT_SECONDARY)
            .size(12.5),
        );
        ui.add_space(12.0);

        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(RichText::new("Worker threads:").strong());
                ui.add(egui::Slider::new(&mut self.prep_jobs, 1..=self.hw.logical_cpus.max(2)));
                ui.label(
                    RichText::new("(leaves 2 cores free for desktop by default)")
                        .color(theme::TEXT_DIM)
                        .size(11.5),
                );
            });
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                let can_run = running.is_none() && self.game_files.ready();
                let primary = Button::new(RichText::new("Prepare All (Shaders + Warm)").strong().color(Color32::WHITE))
                    .fill(if can_run { theme::ACCENT_RED } else { theme::BG_CARD_ALT })
                    .min_size(Vec2::new(210.0, 36.0));
                if ui.add_enabled(can_run, primary).clicked() {
                    self.trigger_prep(TaskKind::PrepareAll);
                }
                if ui
                    .add_enabled(can_run, Button::new("Build Shaders Only").min_size(Vec2::new(150.0, 36.0)))
                    .clicked()
                {
                    self.trigger_prep(TaskKind::Shaders);
                }
                if ui
                    .add_enabled(
                        running.is_none() && self.cache.shaders_ready(),
                        Button::new("Warm Pipelines").min_size(Vec2::new(140.0, 36.0)),
                    )
                    .clicked()
                {
                    self.trigger_prep(TaskKind::Warm);
                }
                if running.is_some() && ui.button("Stop").clicked() {
                    self.runner.stop();
                }
            });

            if running.is_some() || prog.done > 0 {
                ui.add_space(12.0);
                ui.horizontal(|ui| {
                    ui.label(RichText::new(&prog.step_label).strong().color(theme::CYAN));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.label(
                            RichText::new(format!("{} s", self.runner.elapsed_secs()))
                                .color(theme::TEXT_SECONDARY),
                        );
                    });
                });
                ui.add(ProgressBar::new(prog.fraction()).desired_height(10.0).show_percentage());
                if !prog.last_summary.is_empty() {
                    ui.label(RichText::new(&prog.last_summary).color(theme::TEXT_SECONDARY).size(12.0));
                }
            }
        });

        ui.add_space(10.0);
        theme::card_frame().show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.vertical(|ui| {
                    ui.label(RichText::new("Cache Folder").strong().size(14.0));
                    ui.label(
                        RichText::new(format!(
                            "{}  ·  {} shaders ({} MB)  ·  host-pipelines.bin: {} MB",
                            cache_dir().display(),
                            self.cache.shader_files,
                            self.cache.shader_mb,
                            self.cache.pipeline_lib_mb
                        ))
                        .color(theme::TEXT_SECONDARY)
                        .size(12.0),
                    );
                });
                ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                    if ui.button("Open Cache Folder").clicked() {
                        runner::open_folder(&cache_dir());
                    }
                    if ui.button("Refresh").clicked() {
                        self.refresh_status();
                    }
                });
            });
        });

        ui.add_space(10.0);
        ui.label(RichText::new("Recent output").strong().size(13.0));
        let lines = self.runner.logs_filtered("");
        theme::card_frame().show(ui, |ui| {
            ScrollArea::vertical()
                .stick_to_bottom(true)
                .max_height(ui.available_height().max(180.0))
                .show(ui, |ui| {
                    let start = lines.len().saturating_sub(250);
                    for line in &lines[start..] {
                        ui.label(RichText::new(line).monospace().size(11.5).color(theme::TEXT_SECONDARY));
                    }
                });
        });
    }

    fn ui_logs(&mut self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Logs and crashes").strong().size(22.0));
        ui.add_space(8.0);

        let avail_w = ui.available_width();
        let right_w = 300.0f32.min(avail_w * 0.34);
        let left_w = (avail_w - right_w - 14.0).max(320.0);

        ui.horizontal_top(|ui| {
            // Left: Live Log stream
            ui.vertical(|ui| {
                ui.set_width(left_w);
                ui.horizontal(|ui| {
                    ui.label(
                        RichText::new(format!("LIVE LOG  ·  {} LINES", self.runner.total_lines()))
                            .color(theme::TEXT_SECONDARY)
                            .strong()
                            .size(11.5),
                    );
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut self.log_filter)
                                .hint_text("Filter...")
                                .desired_width(140.0),
                        );
                        if ui.small_button("Copy").clicked() {
                            ui.ctx().copy_text(self.runner.all_logs_text());
                            self.toast = Some("Live log copied to clipboard.".into());
                        }
                        if ui.small_button("Logs folder").clicked() {
                            runner::open_folder(&data_dir());
                        }
                        if ui.small_button("Clear").clicked() {
                            self.runner.clear_logs();
                        }
                    });
                });
                ui.add_space(4.0);
                let lines = self.runner.logs_filtered(&self.log_filter);
                theme::card_frame().show(ui, |ui| {
                    ui.set_min_height((ui.available_height() - 12.0).max(380.0));
                    ScrollArea::vertical()
                        .stick_to_bottom(self.auto_scroll)
                        .auto_shrink([false, false])
                        .show(ui, |ui| {
                            if lines.is_empty() {
                                ui.label(
                                    RichText::new("No log lines yet. Start a run or build shaders to see live output here.")
                                        .color(theme::TEXT_DIM)
                                        .monospace()
                                        .size(12.0),
                                );
                            } else {
                                for line in &lines {
                                    let col = if line.contains("ERROR") || line.contains("panic") || line.contains("failed") {
                                        theme::ACCENT_RED
                                    } else if line.contains("WARN") {
                                        theme::AMBER
                                    } else if line.contains(" fps, ") {
                                        theme::EMERALD
                                    } else {
                                        theme::TEXT_SECONDARY
                                    };
                                    ui.label(RichText::new(line).monospace().size(11.5).color(col));
                                }
                            }
                        });
                });
            });

            ui.add_space(12.0);

            // Right: Crashes + Copy Report + Last Run Readout
            ui.vertical(|ui| {
                ui.set_width(right_w);
                ui.horizontal(|ui| {
                    ui.label(RichText::new("CRASHES & RUN LOGS").color(theme::TEXT_SECONDARY).strong().size(11.5));
                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                        if ui.small_button("Refresh").clicked() {
                            self.log_entries = runner::scan_data_logs();
                        }
                    });
                });
                ui.add_space(4.0);

                theme::card_frame().show(ui, |ui| {
                    ScrollArea::vertical().max_height(210.0).show(ui, |ui| {
                        if self.log_entries.is_empty() {
                            ui.label(
                                RichText::new("No log or crash files found yet in %LOCALAPPDATA%\\victory\\data.")
                                    .color(theme::TEXT_DIM)
                                    .size(12.0),
                            );
                        } else {
                            let entries = self.log_entries.clone();
                            for entry in entries {
                                ui.horizontal(|ui| {
                                    let col = if entry.is_crash { theme::ACCENT_RED } else { theme::TEXT_PRIMARY };
                                    if ui
                                        .selectable_label(false, RichText::new(&entry.name).color(col).size(12.0))
                                        .clicked()
                                        && let Ok(text) = std::fs::read_to_string(&entry.path)
                                    {
                                        let tail: Vec<&str> = text.lines().rev().take(25).collect();
                                        self.selected_crash_preview =
                                            tail.into_iter().rev().collect::<Vec<_>>().join("\n");
                                    }
                                    ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                                        ui.label(
                                            RichText::new(format!("{} KB", entry.size_kb))
                                                .color(theme::TEXT_DIM)
                                                .size(11.0),
                                        );
                                    });
                                });
                            }
                        }
                    });
                });

                if !self.selected_crash_preview.is_empty() {
                    ui.add_space(6.0);
                    theme::card_frame().show(ui, |ui| {
                        ScrollArea::vertical().max_height(110.0).show(ui, |ui| {
                            ui.label(
                                RichText::new(&self.selected_crash_preview)
                                    .monospace()
                                    .size(10.5)
                                    .color(theme::TEXT_SECONDARY),
                            );
                        });
                    });
                }

                ui.add_space(8.0);
                ui.horizontal(|ui| {
                    let report_btn = Button::new(RichText::new("Copy report").strong().color(Color32::WHITE))
                        .fill(theme::ACCENT_RED)
                        .min_size(Vec2::new(138.0, 34.0));
                    if ui.add(report_btn).clicked() {
                        let metrics = self.runner.snapshot_metrics();
                        let logs = self.runner.logs_filtered("");
                        let rep = runner::build_diagnostic_report(&self.hw, &self.cfg, &self.cache, &metrics, &logs);
                        ui.ctx().copy_text(rep);
                        self.toast = Some("Sanitized diagnostic report copied to clipboard.".into());
                    }
                    if ui.add(Button::new("Logs folder").min_size(Vec2::new(138.0, 34.0))).clicked() {
                        runner::open_folder(&data_dir());
                    }
                });

                ui.add_space(8.0);
                let m = self.runner.snapshot_metrics();
                theme::card_frame().show(ui, |ui| {
                    ui.label(RichText::new("Last run").strong().size(13.5));
                    ui.add_space(4.0);
                    if let Some(med) = m.median_fps() {
                        ui.label(
                            RichText::new(format!(
                                "Median: {:.1} fps  (range {:.1} – {:.1} fps)",
                                med, m.worst_fps, m.best_fps
                            ))
                            .color(theme::EMERALD)
                            .strong()
                            .size(12.5),
                        );
                        if let Some(ms) = m.last_ms {
                            let draws_s = m
                                .last_draws
                                .map(|d| format!(" · {d} draws"))
                                .unwrap_or_default();
                            ui.label(
                                RichText::new(format!("Last sample: {:.1} ms/frame{draws_s}", ms))
                                    .color(theme::TEXT_SECONDARY)
                                    .size(12.0),
                            );
                        }
                        if !m.last_vram_line.is_empty() {
                            ui.label(
                                RichText::new(&m.last_vram_line)
                                    .color(theme::TEXT_DIM)
                                    .size(11.0),
                            );
                        }
                        if let Some(log_path) = &m.last_log_path
                            && let Some(name) = log_path.file_name().and_then(|s| s.to_str())
                        {
                            ui.label(
                                RichText::new(format!("Log: {name}"))
                                    .color(theme::TEXT_DIM)
                                    .size(11.0),
                            );
                        }
                    } else {
                        ui.label(
                            RichText::new("Play once for a readout: median FPS, min/max frame rate, draws, and VRAM.")
                                .color(theme::TEXT_DIM)
                                .size(12.0),
                        );
                    }
                });
            });
        });
    }

    fn ui_settings(&mut self, ui: &mut egui::Ui, ctx: &egui::Context) {
        let mut changed = false;

        ui.horizontal(|ui| {
            ui.label(RichText::new("Settings").strong().size(22.0));
            ui.with_layout(Layout::right_to_left(Align::Center), |ui| {
                if ui.button("Reset to Recommended").clicked() {
                    let saved_dir = self.cfg.game_dir.clone();
                    self.cfg = LauncherConfig::default();
                    self.cfg.game_dir = saved_dir;
                    changed = true;
                }
            });
        });
        ui.add_space(8.0);

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            // --- Display & Upscaling ---
            theme::card_frame().show(ui, |ui| {
                ui.label(RichText::new("Display & Upscaling").strong().size(15.0));
                ui.add_space(6.0);

                ui.horizontal(|ui| {
                    ui.label("Console profile:");
                    egui::ComboBox::from_id_salt("console_combo")
                        .selected_text(self.cfg.console.label())
                        .show_ui(ui, |ui| {
                            for opt in [ConsoleModel::Auto, ConsoleModel::SeriesX, ConsoleModel::SeriesS] {
                                if ui.selectable_value(&mut self.cfg.console, opt, opt.label()).changed() {
                                    self.cfg.preset = Preset::Custom;
                                    changed = true;
                                }
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("Output resolution:");
                    egui::ComboBox::from_id_salt("res_combo")
                        .selected_text(self.cfg.output.label())
                        .show_ui(ui, |ui| {
                            for opt in [OutputRes::P720, OutputRes::P1080, OutputRes::P1440, OutputRes::P4K] {
                                if ui.selectable_value(&mut self.cfg.output, opt, opt.label()).changed() {
                                    self.cfg.preset = Preset::Custom;
                                    changed = true;
                                }
                            }
                        });
                });

                ui.horizontal(|ui| {
                    ui.label("Internal render scale (0 = title default 0.667):");
                    if ui
                        .add(egui::Slider::new(&mut self.cfg.render_scale, 0.0..=1.0).fixed_decimals(2))
                        .changed()
                    {
                        self.cfg.preset = Preset::Custom;
                        changed = true;
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("FPS cap (0 = uncapped):");
                    if ui.add(egui::Slider::new(&mut self.cfg.fps_cap, 0..=120)).changed() {
                        changed = true;
                    }
                });

                ui.horizontal(|ui| {
                    if ui
                        .checkbox(&mut self.cfg.fsr_4k, "Native PC FidelityFX 4K (FSR 3.1.6 / FSR 4)")
                        .changed()
                    {
                        self.cfg.preset = Preset::Custom;
                        changed = true;
                    }
                    if self.cfg.fsr_4k {
                        egui::ComboBox::from_id_salt("fsr_combo")
                            .selected_text(self.cfg.fsr_mode.label())
                            .show_ui(ui, |ui| {
                                for m in [FsrMode::Performance, FsrMode::Balanced, FsrMode::Quality] {
                                    if ui.selectable_value(&mut self.cfg.fsr_mode, m, m.label()).changed() {
                                        changed = true;
                                    }
                                }
                            });
                    }
                });

                if ui
                    .checkbox(
                        &mut self.cfg.smooth_motion,
                        "NVIDIA Smooth Motion copy (launches victory_sm.exe for 2x visual FPS in NVIDIA App)",
                    )
                    .changed()
                {
                    changed = true;
                }
            });

            ui.add_space(8.0);

            // --- NVIDIA Performance & VRAM / RAM ---
            theme::card_frame().show(ui, |ui| {
                ui.label(RichText::new("NVIDIA Performance, ReBAR & Memory").strong().size(15.0));
                ui.add_space(6.0);

                if ui
                    .checkbox(
                        &mut self.cfg.skip_rt_resolve,
                        "Skip CS_RaytracingResolve when RT is off (saves ~18 ms GPU/frame, keeps shadows & ambient intact)",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut self.cfg.ray_tracing, "Enable real Wave32 Ray Tracing (heavy GPU load)")
                    .changed()
                {
                    self.cfg.preset = Preset::Custom;
                    changed = true;
                }
                if ui
                    .checkbox(&mut self.cfg.send_bar, "ReBAR submission upload buffers (VICTORY_SEND_BAR=1)")
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(
                        &mut self.cfg.wc_no_back,
                        "Skip readback on large (>=16 MiB) write-combined buffers (VICTORY_WC_NO_BACK=1)",
                    )
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut self.cfg.tex_store, "Disk mip cache (VICTORY_TEXTURE_STORE) for faster revisits")
                    .changed()
                {
                    changed = true;
                }
                if ui
                    .checkbox(&mut self.cfg.wait_textures, "Wait for texture remaps (VICTORY_TEXTURE_NO_WAIT=0)")
                    .changed()
                {
                    changed = true;
                }

                ui.horizontal(|ui| {
                    ui.label("ReBAR window cap MiB (-1 = auto by GPU tier):");
                    if ui.add(egui::DragValue::new(&mut self.cfg.windows_bar_mb).range(-1..=16384).speed(256)).changed() {
                        changed = true;
                    }
                    ui.label(
                        RichText::new(format!("(effective: {} MiB)", self.cfg.effective_windows_bar_mb(&self.hw)))
                            .color(theme::TEXT_DIM)
                            .size(11.5),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Texture mip drop (-1 = auto by VRAM tier, 0..3):");
                    if ui.add(egui::DragValue::new(&mut self.cfg.mip_drop).range(-1..=3)).changed() {
                        changed = true;
                    }
                    ui.label(
                        RichText::new(format!("(effective: {})", self.cfg.effective_mip_drop(&self.hw)))
                            .color(theme::TEXT_DIM)
                            .size(11.5),
                    );
                });

                ui.horizontal(|ui| {
                    ui.label("Deferred thin lists (auto OFF on RTX 50 Blackwell):");
                    for opt in [TriState::Auto, TriState::On, TriState::Off] {
                        if ui.selectable_value(&mut self.cfg.thin_lists, opt, opt.label()).changed() {
                            changed = true;
                        }
                    }
                });

                ui.horizontal(|ui| {
                    ui.label("16 GiB RAM retention profile (leave Auto on 32/64 GB RAM):");
                    for opt in [TriState::Auto, TriState::On, TriState::Off] {
                        if ui.selectable_value(&mut self.cfg.ram16_profile, opt, opt.label()).changed() {
                            changed = true;
                        }
                    }
                });
                if self.cfg.ram16_profile == TriState::On && self.hw.ram_total_gb >= 24.0 {
                    ui.label(
                        RichText::new(format!(
                            "Warning: RAM16 is forced ON on your {:.0} GB RAM PC — set Auto/Off for best FPS.",
                            self.hw.ram_total_gb
                        ))
                        .color(theme::AMBER)
                        .size(12.0),
                    );
                }
            });

            ui.add_space(8.0);

            // --- Input ---
            theme::card_frame().show(ui, |ui| {
                ui.label(RichText::new("Keyboard & Mouse Input").strong().size(15.0));
                ui.add_space(6.0);
                if ui
                    .checkbox(
                        &mut self.cfg.kbm_gta,
                        "GTA V PC Keyboard + Mouse-Look (press F2 or ~ in-game to lock/hide cursor)",
                    )
                    .changed()
                {
                    changed = true;
                }
                if self.cfg.kbm_gta {
                    ui.horizontal(|ui| {
                        ui.label("Mouse sensitivity:");
                        if ui
                            .add(egui::Slider::new(&mut self.cfg.mouse_sens, 0.25..=3.0).fixed_decimals(2))
                            .changed()
                        {
                            changed = true;
                        }
                        if ui.checkbox(&mut self.cfg.mouse_invert_y, "Invert Y").changed() {
                            changed = true;
                        }
                    });
                }
            });

            ui.add_space(8.0);

            // --- Renderer Ports & Diagnostics ---
            theme::card_frame().show(ui, |ui| {
                ui.label(RichText::new("Renderer Ports & Diagnostics").strong().size(15.0));
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    changed |= ui.checkbox(&mut self.cfg.port_constants, "Port 1 (Constants)").changed();
                    changed |= ui.checkbox(&mut self.cfg.port_cbv, "Port 5 (NVIDIA CBV)").changed();
                    changed |= ui.checkbox(&mut self.cfg.port_guard, "Port 6 (Guard)").changed();
                    changed |= ui.checkbox(&mut self.cfg.port_journal, "Port 7 (Journal)").changed();
                    changed |= ui.checkbox(&mut self.cfg.port_rt, "Port RT (Fill Forward)").changed();
                    changed |= ui.checkbox(&mut self.cfg.port_descriptors, "Port 2 (Opt-in Descriptors)").changed();
                });
                ui.add_space(6.0);
                ui.horizontal_wrapped(|ui| {
                    changed |= ui.checkbox(&mut self.cfg.panel_hud, "Show F3 HUD on start").changed();
                    changed |= ui.checkbox(&mut self.cfg.diag_build, "Use diag build").changed();
                    if ui.checkbox(&mut self.cfg.gpu_timing, "15s GPU timing profile (uses diag build)").changed() {
                        if self.cfg.gpu_timing {
                            self.cfg.diag_build = true;
                        }
                        changed = true;
                    }
                });
                ui.add_space(6.0);
                if ui.button("Change Game Folder...").clicked() {
                    self.pick_game_folder(ctx);
                }
            });
        });

        if changed {
            self.save_cfg();
        }
    }

    fn ui_links(&self, ui: &mut egui::Ui) {
        ui.label(RichText::new("Links & Open-Source Credits").strong().size(22.0));
        ui.add_space(10.0);

        ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
            ui.label(RichText::new("PROJECT").color(theme::TEXT_DIM).strong().size(11.0));
            ui.add_space(4.0);

            self.link_card(
                ui,
                "VIctory-RTX-Mc3yohan Repository",
                "mc3yohan-lab/VIctory-RTX-Mc3yohan — NVIDIA RTX / Wave32 fork (GPL-3.0-or-later)",
                "https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan",
            );
            ui.add_space(6.0);
            self.link_card(
                ui,
                "Maintainer Profile (Mc3yohan)",
                "github.com/Mc3yohan",
                "https://github.com/Mc3yohan",
            );

            ui.add_space(14.0);
            ui.label(RichText::new("UPSTREAM & COMMUNITY CREDITS (GPL-3.0)").color(theme::TEXT_DIM).strong().size(11.0));
            ui.add_space(4.0);

            for (title, desc, url) in [
                ("Marvisak/VIctory", "Original PE/XEX loader, HLE runtime, D3D12 backend & GFX10-to-DXIL recompiler", "https://github.com/Marvisak/VIctory"),
                ("avatarmanz/VIctory-7900XTX", "Renderer Ports 1–7 & RT Part 1, VICTORY_OUTPUT, thin lists, async submit & relay", "https://github.com/avatarmanz/VIctory-7900XTX"),
                ("codec-d/cool_fox", "Wave32 guarded joins, LDS atomic ordering, WaitOnAddress, texback & fence guards", "https://github.com/codec-d/cool_fox"),
                ("trustno1-coder/VIctory-NVIDIA", "D3D12 538/646 fixes, mimalloc, Blackwell thin guard, texback async", "https://github.com/trustno1-coder/VIctory-NVIDIA"),
                ("fxceless/victory-fork", "Face animation, target variants, FEED_ONE_CALL, Shadow::differing fix", "https://github.com/fxceless/victory-fork"),
                ("Belobug/VIctory-Belobug-AMD", "ThinLTO, PreludeUpload, F3 HUD, GTA KBM mouse-look, FidelityFX DX12 bridge", "https://github.com/Belobug/VIctory-Belobug-AMD"),
                ("Peppacodes / TaynanGT / Geckomacabre / TheUnrealZaka / polackangry / ScarlettPort", "WRITE_JOBS, Vice-Tay #27/#45/#116, ambient Role::Reflect + bilinear/NaN scrub, readback pool, batched sweep, MMCSS Pro Audio, levels-on-demand fix", "https://github.com/ZigZagBoi1/ScarlettPort"),
            ] {
                self.link_card(ui, title, desc, url);
                ui.add_space(6.0);
            }
        });
    }

    fn link_card(&self, ui: &mut egui::Ui, title: &str, subtitle: &str, url: &str) {
        self.status_row(ui, RowIcon::Link, title, subtitle, |ui| {
            if ui.button("Open").clicked() {
                runner::open_url(url);
            }
        });
    }
}

fn main() -> eframe::Result<()> {
    let viewport = egui::ViewportBuilder::default()
        .with_title("VIctory-RTX-Mc3yohan")
        .with_inner_size([1080.0, 710.0])
        .with_min_inner_size([920.0, 600.0])
        .with_icon(Arc::new(art::app_icon()));
    let options = eframe::NativeOptions {
        viewport,
        ..Default::default()
    };
    eframe::run_native(
        "VIctory-RTX-Mc3yohan",
        options,
        Box::new(|cc| Ok(Box::new(LauncherApp::new(cc)))),
    )
}
