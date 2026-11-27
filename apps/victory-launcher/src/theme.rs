//! Identidad visual oficial de Quantico Systems ("Señal Viva" en modo oscuro):
//! - Negro carbón profundo (#07090D / #0D1119) y superficies limpias (#111622)
//! - Azul de señal Quantico (#007BFF), fuerte (#0062CC) y luminoso (#4DA3FF)
//! - Firma geométrica: emblema de anillo blanco + barra diagonal azul a 53.8°
//!   (pendiente 328/240 del manual de marca Quantico) y cuadrícula técnica de 48 px.

use eframe::egui::{
    self, Color32, CornerRadius, Frame, Margin, Pos2, Rect, Shape, Stroke, Vec2, Visuals, pos2,
    vec2,
};

pub const BG_CANVAS: Color32 = Color32::from_rgb(7, 9, 13); // #07090D
pub const BG_HEADER: Color32 = Color32::from_rgb(11, 15, 23); // #0B0F17
pub const BG_CARD: Color32 = Color32::from_rgb(17, 22, 34); // #111622
pub const BG_CARD_ALT: Color32 = Color32::from_rgb(22, 29, 44); // #161D2C
pub const BG_INPUT: Color32 = Color32::from_rgb(13, 17, 26); // #0D111A
pub const BORDER: Color32 = Color32::from_rgb(33, 43, 61); // #212B3D
pub const BORDER_STRONG: Color32 = Color32::from_rgb(47, 62, 88); // #2F3E58

pub const QUANTICO_BLUE: Color32 = Color32::from_rgb(0, 123, 255); // #007BFF
pub const QUANTICO_BLUE_STRONG: Color32 = Color32::from_rgb(0, 98, 204); // #0062CC
pub const QUANTICO_BLUE_LIGHT: Color32 = Color32::from_rgb(77, 163, 255); // #4DA3FF
pub const QUANTICO_BLUE_SOFT: Color32 = Color32::from_rgba_premultiplied(0, 56, 120, 95);

pub const STATUS_OK: Color32 = Color32::from_rgb(16, 185, 129); // #10B981
pub const STATUS_WARN: Color32 = Color32::from_rgb(245, 158, 11); // #F59E0B
pub const STATUS_ERR: Color32 = Color32::from_rgb(229, 72, 77); // #E5484D
pub const STATUS_ERR_DARK: Color32 = Color32::from_rgb(190, 42, 50);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(246, 248, 252); // #F6F8FC
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(169, 175, 182); // #A9AFB6 (Niebla Quantico)
pub const TEXT_MUTED: Color32 = Color32::from_rgb(112, 126, 146); // #707E92

pub fn apply_quantico_theme(ctx: &egui::Context) {
    let mut visuals = Visuals::dark();
    visuals.override_text_color = Some(TEXT_PRIMARY);
    visuals.panel_fill = BG_CANVAS;
    visuals.window_fill = BG_CARD;
    visuals.extreme_bg_color = BG_INPUT;
    visuals.faint_bg_color = BG_CARD_ALT;
    visuals.code_bg_color = BG_INPUT;
    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(0, 123, 255, 76);
    visuals.selection.stroke = Stroke::new(1.0_f32, QUANTICO_BLUE_LIGHT);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(10);

    visuals.widgets.inactive.bg_fill = BG_CARD_ALT;
    visuals.widgets.inactive.weak_bg_fill = BG_CARD_ALT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(8);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(28, 38, 58);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(28, 38, 58);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, QUANTICO_BLUE_LIGHT);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(8);

    visuals.widgets.active.bg_fill = QUANTICO_BLUE_STRONG;
    visuals.widgets.active.weak_bg_fill = QUANTICO_BLUE_STRONG;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, QUANTICO_BLUE_LIGHT);
    visuals.widgets.active.fg_stroke = Stroke::new(1.5_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(8);

    ctx.set_visuals(visuals);

    let mut style = (*ctx.style()).clone();
    style.spacing.item_spacing = vec2(10.0, 10.0);
    style.spacing.button_padding = vec2(14.0, 8.0);
    style.spacing.window_margin = Margin::same(14);
    style.interaction.tooltip_delay = 0.25;
    ctx.set_style(style);
}

pub fn bento_card() -> Frame {
    Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(12))
        .inner_margin(Margin::symmetric(16, 14))
}

pub fn alert_banner(accent: Color32, fill: Color32) -> Frame {
    Frame::new()
        .fill(fill)
        .stroke(Stroke::new(1.0_f32, accent))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 10))
}

/// Emblema vectorial Quantico Systems: anillo blanco + barra diagonal azul a 53.8°.
pub fn paint_quantico_mark(painter: &egui::Painter, rect: Rect) {
    let center = rect.center();
    let r = rect.width().min(rect.height()) * 0.36;
    painter.rect_filled(rect, CornerRadius::same(9), Color32::from_rgb(12, 18, 30));
    painter.rect_stroke(
        rect,
        CornerRadius::same(9),
        Stroke::new(1.2_f32, Color32::from_rgba_unmultiplied(0, 123, 255, 160)),
        egui::StrokeKind::Inside,
    );
    painter.circle_stroke(center, r, Stroke::new(2.4_f32, Color32::WHITE));
    // Barra diagonal de señal Quantico (pendiente ~53.8°, 328/240).
    let p1 = pos2(center.x + r * 0.08, center.y + r * 0.06);
    let p2 = pos2(center.x + r * 1.05, center.y + r * 1.15);
    painter.line_segment([p1, p2], Stroke::new(3.0_f32, QUANTICO_BLUE));
}

/// Dibuja la textura característica de Quantico sobre el banner hero:
/// cuadrícula técnica de 48 px, resplandor azul sutil y barra diagonal a 53.8°.
pub fn paint_quantico_banner_overlay(painter: &egui::Painter, rect: Rect, has_cover_art: bool) {
    let clipped = painter.with_clip_rect(rect);

    if !has_cover_art {
        // Fondo carbón + cuadrícula técnica de 48 px (firma de quantico-web).
        clipped.rect_filled(rect, CornerRadius::same(14), Color32::from_rgb(9, 13, 21));
        let grid_col = Color32::from_rgba_unmultiplied(255, 255, 255, 9);
        let step = 48.0_f32;
        let mut x = rect.left() + step;
        while x < rect.right() {
            clipped.line_segment(
                [pos2(x, rect.top()), pos2(x, rect.bottom())],
                Stroke::new(1.0_f32, grid_col),
            );
            x += step;
        }
        let mut y = rect.top() + step;
        while y < rect.bottom() {
            clipped.line_segment(
                [pos2(rect.left(), y), pos2(rect.right(), y)],
                Stroke::new(1.0_f32, grid_col),
            );
            y += step;
        }
    }

    // Velo vertical suave en la base (donde reposan título y botón JUGAR) +
    // ligera viñeta izquierda para legibilidad perfecta sin tapar el arte central.
    let v_strips = 20usize;
    let sh = rect.height() / v_strips as f32;
    for i in 0..v_strips {
        let t = i as f32 / (v_strips - 1) as f32; // 0 = arriba, 1 = abajo
        let alpha = if has_cover_art {
            let bottom = ((t - 0.30).max(0.0) / 0.70).powf(1.45) * 238.0;
            let top = ((0.18 - t).max(0.0) / 0.18) * 55.0;
            bottom.max(top).clamp(18.0, 242.0) as u8
        } else {
            (t * 95.0) as u8
        };
        let y0 = rect.top() + i as f32 * sh;
        clipped.rect_filled(
            Rect::from_min_max(pos2(rect.left(), y0), pos2(rect.right(), y0 + sh + 1.0)),
            CornerRadius::ZERO,
            Color32::from_rgba_unmultiplied(7, 10, 16, alpha),
        );
    }

    // Barra diagonal Quantico a 53.8° (pendiente 328/240 -> dx = h * 240/328)
    // en el extremo derecho como firma sobria de marca.
    let h = rect.height();
    let dx = h * (240.0_f32 / 328.0_f32);
    let x_top = rect.right() - 68.0;
    let bar_w = 14.0_f32;
    let poly = vec![
        pos2(x_top, rect.top()),
        pos2(x_top + bar_w, rect.top()),
        pos2(x_top + bar_w - dx, rect.bottom()),
        pos2(x_top - dx, rect.bottom()),
    ];
    clipped.add(Shape::convex_polygon(
        poly,
        Color32::from_rgba_unmultiplied(0, 123, 255, if has_cover_art { 52 } else { 78 }),
        Stroke::NONE,
    ));

    // Segunda línea fina paralela (acento técnico).
    let x2 = x_top + 22.0;
    let poly2 = vec![
        pos2(x2, rect.top()),
        pos2(x2 + 3.0, rect.top()),
        pos2(x2 + 3.0 - dx, rect.bottom()),
        pos2(x2 - dx, rect.bottom()),
    ];
    clipped.add(Shape::convex_polygon(
        poly2,
        Color32::from_rgba_unmultiplied(77, 163, 255, 65),
        Stroke::NONE,
    ));

    // Borde superior luminoso azul Quantico (2 px).
    clipped.line_segment(
        [pos2(rect.left() + 14.0, rect.top() + 1.0), pos2(rect.right() - 14.0, rect.top() + 1.0)],
        Stroke::new(2.0_f32, Color32::from_rgba_unmultiplied(0, 123, 255, 190)),
    );

    // Marco exterior limpio.
    painter.rect_stroke(
        rect,
        CornerRadius::same(14),
        Stroke::new(1.0_f32, BORDER_STRONG),
        egui::StrokeKind::Inside,
    );
}

/// Pequeña píldora de estado con punto de color.
pub fn status_pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    let bg = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 26);
    let border = Color32::from_rgba_unmultiplied(color.r(), color.g(), color.b(), 95);
    Frame::new()
        .fill(bg)
        .stroke(Stroke::new(1.0_f32, border))
        .corner_radius(CornerRadius::same(99))
        .inner_margin(Margin::symmetric(10, 3))
        .show(ui, |ui| {
            ui.horizontal(|ui| {
                ui.spacing_mut().item_spacing.x = 6.0;
                let (dot_rect, _) = ui.allocate_exact_size(vec2(7.0, 7.0), egui::Sense::hover());
                ui.painter().circle_filled(dot_rect.center(), 3.2, color);
                ui.label(egui::RichText::new(label).color(color).strong().size(11.5));
            });
        });
}

/// Dibuja un ícono vectorial limpio para las tarjetas Bento.
pub fn paint_badge_icon(ui: &mut egui::Ui, kind: BadgeKind, accent: Color32) {
    let (rect, _) = ui.allocate_exact_size(Vec2::splat(30.0), egui::Sense::hover());
    let bg = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 24);
    let border = Color32::from_rgba_unmultiplied(accent.r(), accent.g(), accent.b(), 72);
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(8), bg);
    p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0_f32, border), egui::StrokeKind::Inside);
    let c: Pos2 = rect.center();
    let s = Stroke::new(1.6_f32, accent);

    match kind {
        BadgeKind::Folder => {
            p.rect_stroke(
                Rect::from_center_size(c + vec2(0.0, 1.5), vec2(14.0, 9.5)),
                CornerRadius::same(2),
                s,
                egui::StrokeKind::Middle,
            );
            p.line_segment([c + vec2(-7.0, -3.5), c + vec2(-1.5, -3.5)], s);
        }
        BadgeKind::Bolt => {
            p.line_segment([c + vec2(1.5, -7.0), c + vec2(-3.5, 0.5)], s);
            p.line_segment([c + vec2(-3.5, 0.5), c + vec2(2.5, 0.5)], s);
            p.line_segment([c + vec2(2.5, 0.5), c + vec2(-1.5, 7.0)], s);
        }
        BadgeKind::Sliders => {
            for (dy, knob_x) in [(-4.5_f32, -2.5_f32), (0.0, 3.0), (4.5, -1.0)] {
                p.line_segment([c + vec2(-6.5, dy), c + vec2(6.5, dy)], s);
                p.circle_filled(c + vec2(knob_x, dy), 2.1, accent);
            }
        }
        BadgeKind::Pad => {
            p.rect_stroke(
                Rect::from_center_size(c, vec2(15.0, 9.5)),
                CornerRadius::same(4),
                s,
                egui::StrokeKind::Middle,
            );
            p.line_segment([c + vec2(-4.5, 0.0), c + vec2(-1.5, 0.0)], s);
            p.line_segment([c + vec2(-3.0, -1.5), c + vec2(-3.0, 1.5)], s);
            p.circle_filled(c + vec2(3.5, -1.2), 1.2, accent);
            p.circle_filled(c + vec2(5.2, 1.2), 1.2, accent);
        }
    }
}

#[derive(Clone, Copy)]
pub enum BadgeKind {
    Folder,
    Bolt,
    Sliders,
    Pad,
}
