//! Dark carbon & NVIDIA emerald/crimson theme + crisp vector-drawn icons.

use eframe::egui::{self, Color32, CornerRadius, Margin, Painter, Pos2, Rect, Stroke, Vec2, pos2};

pub const BG_CANVAS: Color32 = Color32::from_rgb(12, 14, 19);
pub const BG_SIDEBAR: Color32 = Color32::from_rgb(16, 18, 25);
pub const BG_CARD: Color32 = Color32::from_rgb(22, 25, 34);
pub const BG_CARD_ALT: Color32 = Color32::from_rgb(27, 31, 43);
pub const BG_INPUT: Color32 = Color32::from_rgb(17, 20, 28);
pub const BORDER: Color32 = Color32::from_rgb(38, 43, 58);
pub const BORDER_ACTIVE: Color32 = Color32::from_rgb(64, 73, 98);

pub const TEXT_PRIMARY: Color32 = Color32::from_rgb(243, 245, 250);
pub const TEXT_SECONDARY: Color32 = Color32::from_rgb(148, 157, 178);
pub const TEXT_DIM: Color32 = Color32::from_rgb(104, 113, 134);

pub const ACCENT_RED: Color32 = Color32::from_rgb(230, 57, 70);
pub const ACCENT_RED_DIM: Color32 = Color32::from_rgb(76, 26, 34);
pub const EMERALD: Color32 = Color32::from_rgb(16, 185, 129);
pub const AMBER: Color32 = Color32::from_rgb(245, 158, 11);
pub const CYAN: Color32 = Color32::from_rgb(56, 189, 248);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NavIcon {
    Play,
    Shaders,
    Logs,
    Settings,
    Links,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum RowIcon {
    Folder,
    CpuGpu,
    Download,
    Layers,
    Gamepad,
    Sliders,
    Link,
}

pub fn apply(ctx: &egui::Context) {
    let mut visuals = egui::Visuals::dark();
    visuals.window_fill = BG_CANVAS;
    visuals.panel_fill = BG_CANVAS;
    visuals.faint_bg_color = BG_CARD;
    visuals.extreme_bg_color = BG_INPUT;
    visuals.override_text_color = Some(TEXT_PRIMARY);

    visuals.widgets.noninteractive.bg_fill = BG_CARD;
    visuals.widgets.noninteractive.weak_bg_fill = BG_CARD;
    visuals.widgets.noninteractive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.noninteractive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.noninteractive.corner_radius = CornerRadius::same(8);

    visuals.widgets.inactive.bg_fill = BG_CARD_ALT;
    visuals.widgets.inactive.weak_bg_fill = BG_CARD_ALT;
    visuals.widgets.inactive.bg_stroke = Stroke::new(1.0_f32, BORDER);
    visuals.widgets.inactive.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.inactive.corner_radius = CornerRadius::same(7);

    visuals.widgets.hovered.bg_fill = Color32::from_rgb(36, 42, 58);
    visuals.widgets.hovered.weak_bg_fill = Color32::from_rgb(36, 42, 58);
    visuals.widgets.hovered.bg_stroke = Stroke::new(1.0_f32, BORDER_ACTIVE);
    visuals.widgets.hovered.fg_stroke = Stroke::new(1.0_f32, TEXT_PRIMARY);
    visuals.widgets.hovered.corner_radius = CornerRadius::same(7);

    visuals.widgets.active.bg_fill = ACCENT_RED;
    visuals.widgets.active.weak_bg_fill = ACCENT_RED;
    visuals.widgets.active.bg_stroke = Stroke::new(1.0_f32, ACCENT_RED);
    visuals.widgets.active.fg_stroke = Stroke::new(1.0_f32, Color32::WHITE);
    visuals.widgets.active.corner_radius = CornerRadius::same(7);

    visuals.selection.bg_fill = Color32::from_rgba_unmultiplied(16, 185, 129, 70);
    visuals.selection.stroke = Stroke::new(1.0_f32, EMERALD);

    let mut style = (*ctx.style()).clone();
    style.visuals = visuals;
    style.spacing.item_spacing = Vec2::new(10.0, 8.0);
    style.spacing.button_padding = Vec2::new(12.0, 6.0);
    style.spacing.scroll.bar_width = 7.0;
    ctx.set_style(style);
}

pub fn card_frame() -> egui::Frame {
    egui::Frame::new()
        .fill(BG_CARD)
        .stroke(Stroke::new(1.0_f32, BORDER))
        .corner_radius(CornerRadius::same(10))
        .inner_margin(Margin::symmetric(14, 11))
}

pub fn draw_brand_badge(painter: &Painter, rect: Rect) {
    painter.rect_filled(rect, CornerRadius::same(9), Color32::from_rgb(20, 26, 36));
    painter.rect_stroke(
        rect,
        CornerRadius::same(9),
        Stroke::new(1.5_f32, EMERALD),
        egui::StrokeKind::Inside,
    );
    let c = rect.center();
    let s = rect.width() * 0.25;
    let left = pos2(c.x - s, c.y - s * 0.85);
    let mid = pos2(c.x, c.y + s * 0.95);
    let right = pos2(c.x + s, c.y - s * 0.85);
    painter.line_segment([left, mid], Stroke::new(2.8_f32, TEXT_PRIMARY));
    painter.line_segment([mid, right], Stroke::new(2.8_f32, EMERALD));
}

pub fn draw_nav_icon(painter: &Painter, icon: NavIcon, center: Pos2, color: Color32) {
    let st = Stroke::new(1.8_f32, color);
    match icon {
        NavIcon::Play => {
            let pts = vec![
                pos2(center.x - 4.5, center.y - 6.5),
                pos2(center.x + 6.5, center.y),
                pos2(center.x - 4.5, center.y + 6.5),
            ];
            painter.add(egui::Shape::convex_polygon(pts, color, Stroke::NONE));
        }
        NavIcon::Shaders => {
            // Down arrow into tray (prepare/build shaders)
            painter.line_segment([pos2(center.x, center.y - 7.0), pos2(center.x, center.y + 2.5)], st);
            painter.line_segment([pos2(center.x - 4.0, center.y - 1.5), pos2(center.x, center.y + 2.5)], st);
            painter.line_segment([pos2(center.x + 4.0, center.y - 1.5), pos2(center.x, center.y + 2.5)], st);
            painter.line_segment([pos2(center.x - 6.5, center.y + 6.0), pos2(center.x + 6.5, center.y + 6.0)], st);
        }
        NavIcon::Logs => {
            // Terminal / console lines
            let r = Rect::from_center_size(center, Vec2::new(14.0, 12.0));
            painter.rect_stroke(r, CornerRadius::same(2), st, egui::StrokeKind::Inside);
            painter.line_segment([pos2(center.x - 4.0, center.y - 2.0), pos2(center.x - 1.5, center.y)], st);
            painter.line_segment([pos2(center.x - 4.0, center.y + 2.0), pos2(center.x - 1.5, center.y)], st);
            painter.line_segment([pos2(center.x + 0.5, center.y + 2.0), pos2(center.x + 4.0, center.y + 2.0)], st);
        }
        NavIcon::Settings => {
            painter.circle_stroke(center, 3.3, st);
            painter.circle_stroke(center, 6.8, Stroke::new(1.4_f32, color));
        }
        NavIcon::Links => {
            painter.circle_stroke(pos2(center.x - 3.2, center.y - 2.0), 3.0, st);
            painter.circle_stroke(pos2(center.x + 3.8, center.y - 1.2), 2.3, st);
            painter.line_segment([pos2(center.x - 7.0, center.y + 5.5), pos2(center.x + 1.0, center.y + 5.5)], st);
            painter.line_segment([pos2(center.x + 2.0, center.y + 5.5), pos2(center.x + 7.0, center.y + 5.5)], st);
        }
    }
}

pub fn draw_row_icon(ui: &mut egui::Ui, icon: RowIcon) {
    let (rect, _) = ui.allocate_exact_size(Vec2::new(34.0, 34.0), egui::Sense::hover());
    let p = ui.painter();
    p.rect_filled(rect, CornerRadius::same(8), BG_CARD_ALT);
    p.rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0_f32, BORDER), egui::StrokeKind::Inside);
    let c = rect.center();
    let st = Stroke::new(1.6_f32, TEXT_SECONDARY);
    match icon {
        RowIcon::Folder => {
            let body = Rect::from_center_size(pos2(c.x, c.y + 1.5), Vec2::new(14.0, 9.5));
            p.rect_stroke(body, CornerRadius::same(2), st, egui::StrokeKind::Inside);
            p.line_segment([pos2(c.x - 7.0, c.y - 3.5), pos2(c.x - 2.0, c.y - 3.5)], st);
        }
        RowIcon::CpuGpu => {
            let chip = Rect::from_center_size(c, Vec2::new(11.0, 11.0));
            p.rect_stroke(chip, CornerRadius::same(2), st, egui::StrokeKind::Inside);
            p.rect_filled(Rect::from_center_size(c, Vec2::new(4.0, 4.0)), CornerRadius::same(1), EMERALD);
        }
        RowIcon::Download => draw_nav_icon(p, NavIcon::Shaders, c, TEXT_SECONDARY),
        RowIcon::Layers => {
            p.rect_stroke(Rect::from_center_size(pos2(c.x, c.y - 2.5), Vec2::new(13.0, 5.0)), CornerRadius::same(2), st, egui::StrokeKind::Inside);
            p.rect_stroke(Rect::from_center_size(pos2(c.x, c.y + 3.0), Vec2::new(13.0, 5.0)), CornerRadius::same(2), st, egui::StrokeKind::Inside);
        }
        RowIcon::Gamepad => {
            let r = Rect::from_center_size(c, Vec2::new(14.0, 9.0));
            p.rect_stroke(r, CornerRadius::same(4), st, egui::StrokeKind::Inside);
            p.circle_filled(pos2(c.x - 3.5, c.y), 1.3, TEXT_SECONDARY);
            p.circle_filled(pos2(c.x + 3.5, c.y - 1.0), 1.1, TEXT_SECONDARY);
            p.circle_filled(pos2(c.x + 3.5, c.y + 1.5), 1.1, TEXT_SECONDARY);
        }
        RowIcon::Sliders => draw_nav_icon(p, NavIcon::Settings, c, TEXT_SECONDARY),
        RowIcon::Link => {
            p.circle_stroke(pos2(c.x - 2.5, c.y), 3.6, st);
            p.circle_stroke(pos2(c.x + 2.5, c.y), 3.6, st);
        }
    }
}

pub fn status_pill(ui: &mut egui::Ui, label: &str, color: Color32) {
    ui.horizontal(|ui| {
        ui.spacing_mut().item_spacing.x = 6.0;
        let (dot_rect, _) = ui.allocate_exact_size(Vec2::new(8.0, 8.0), egui::Sense::hover());
        ui.painter().circle_filled(dot_rect.center(), 3.6, color);
        ui.label(egui::RichText::new(label).color(color).strong().size(13.0));
    });
}
