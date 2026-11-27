//! Carga de arte de portada en tiempo de ejecución desde la carpeta local del
//! juego (cero imágenes con copyright en Git) + ícono procedural "M" azul RTX.

use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, IconData, Rect, TextureHandle, TextureOptions, pos2};

use crate::theme::{
    BRAND_M_LEFT_BLADE, BRAND_M_LEFT_LEG, BRAND_M_RIGHT_BLADE, BRAND_M_RIGHT_LEG,
};

/// Calcula UVs en modo `object-fit: cover` (recorte centrado sin deformar ni
/// estirar jamás la portada dentro del banner hero).
pub fn cover_uv(tex_size: [usize; 2], target_rect: Rect) -> Rect {
    let tw = (tex_size[0].max(1)) as f32;
    let th = (tex_size[1].max(1)) as f32;
    let rw = target_rect.width().max(1.0);
    let rh = target_rect.height().max(1.0);
    let tex_aspect = tw / th;
    let rect_aspect = rw / rh;
    if tex_aspect > rect_aspect {
        let keep = (rect_aspect / tex_aspect).clamp(0.1, 1.0);
        let pad = (1.0 - keep) * 0.5;
        Rect::from_min_max(pos2(pad, 0.0), pos2(1.0 - pad, 1.0))
    } else {
        let keep = (tex_aspect / rect_aspect).clamp(0.1, 1.0);
        let top = ((1.0 - keep) * 0.35).clamp(0.0, 1.0 - keep);
        Rect::from_min_max(pos2(0.0, top), pos2(1.0, top + keep))
    }
}

/// Carga `xbs_splash.png` o el mayor `xbs_logo_*.png` desde la carpeta local del
/// juego (sin incluir jamás arte propietario en el repositorio).
pub fn load_game_banner_texture(
    ctx: &egui::Context,
    game_dir: &Path,
) -> Option<TextureHandle> {
    let candidate = find_best_banner_image(game_dir)?;
    let img = image::open(&candidate).ok()?.to_rgba8();
    let (w, h) = img.dimensions();
    if w == 0 || h == 0 {
        return None;
    }
    let (w, h, raw) = if w > 1600 {
        let nh = ((h as f64) * (1600.0 / w as f64)).round().max(1.0) as u32;
        let resized =
            image::imageops::resize(&img, 1600, nh, image::imageops::FilterType::Triangle);
        let (rw, rh) = resized.dimensions();
        (rw as usize, rh as usize, resized.into_raw())
    } else {
        (w as usize, h as usize, img.into_raw())
    };
    let color = ColorImage::from_rgba_unmultiplied([w, h], &raw);
    Some(ctx.load_texture("victory_banner_art", color, TextureOptions::LINEAR))
}

fn find_best_banner_image(game_dir: &Path) -> Option<PathBuf> {
    let splash = game_dir.join("xbs_splash.png");
    if splash.is_file() {
        return Some(splash);
    }
    let mut best: Option<(u64, PathBuf)> = None;
    let Ok(entries) = std::fs::read_dir(game_dir) else {
        return None;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if !path.is_file() {
            continue;
        }
        let name = path
            .file_name()
            .and_then(|s| s.to_str())
            .unwrap_or_default()
            .to_ascii_lowercase();
        if (name.starts_with("xbs_logo") || name.contains("splash") || name.contains("banner"))
            && name.ends_with(".png")
            && let Ok(meta) = entry.metadata()
            && best.as_ref().is_none_or(|(b, _)| meta.len() > *b)
        {
            best = Some((meta.len(), path));
        }
    }
    best.map(|(_, p)| p)
}

/// Prueba exacta punto-en-polígono convexo (vértices en sentido horario con +Y hacia abajo).
fn point_in_convex_cw(px: f32, py: f32, poly: &[(f32, f32)]) -> bool {
    let n = poly.len();
    for i in 0..n {
        let (x0, y0) = poly[i];
        let (x1, y1) = poly[(i + 1) % n];
        let cross = (x1 - x0) * (py - y0) - (y1 - y0) * (px - x0);
        if cross < 0.0 {
            return false;
        }
    }
    true
}

/// Ícono procedural de ventana 64×64 con supersampling 4×4 MSAA:
/// "M" geométrica inclinada con cortes biselados estilo NVIDIA en azul eléctrico.
pub fn app_icon() -> IconData {
    let size = 64usize;
    let mut rgba = vec![0u8; size * size * 4];
    let inv = 1.0_f32 / size as f32;
    let offsets = [0.125_f32, 0.375, 0.625, 0.875];

    for y in 0..size {
        for x in 0..size {
            let mut acc = [0.0_f32; 4];
            for &oy in &offsets {
                for &ox in &offsets {
                    let u = (x as f32 + ox) * inv;
                    let v = (y as f32 + oy) * inv;
                    let dx = (u - 0.5).abs();
                    let dy = (v - 0.5).abs();

                    // Squircle redondeado (radio 0.22 en esquinas).
                    let cx = (dx - 0.28).max(0.0);
                    let cy = (dy - 0.28).max(0.0);
                    let dist = (cx * cx + cy * cy).sqrt();
                    if dist > 0.22 {
                        continue;
                    }

                    let mut col = if dist > 0.195 || dx > 0.475 || dy > 0.475 {
                        // Borde azul eléctrico.
                        [0.0, 123.0, 255.0, 235.0]
                    } else if v < 0.48 {
                        // Fondo carbón con sutil resplandor superior.
                        [12.0, 22.0, 39.0, 255.0]
                    } else {
                        [10.0, 15.0, 26.0, 255.0]
                    };

                    // Capas de la "M" en el mismo orden que `theme::paint_brand_mark`.
                    if point_in_convex_cw(u, v, BRAND_M_LEFT_LEG)
                        || point_in_convex_cw(u, v, BRAND_M_RIGHT_LEG)
                    {
                        col = [0.0, 123.0, 255.0, 255.0];
                    }
                    if point_in_convex_cw(u, v, BRAND_M_LEFT_BLADE) {
                        col = [29.0, 140.0, 255.0, 255.0];
                    }
                    if point_in_convex_cw(u, v, BRAND_M_RIGHT_BLADE) {
                        col = [77.0, 163.0, 255.0, 255.0];
                    }

                    // Filo especular hielo-azul en la arista superior del brazo ascendente.
                    let ax = 0.51_f32;
                    let ay = 0.65_f32;
                    let bx = 0.76_f32;
                    let by = 0.22_f32;
                    let abx = bx - ax;
                    let aby = by - ay;
                    let ab_len2 = abx * abx + aby * aby;
                    let t = (((u - ax) * abx + (v - ay) * aby) / ab_len2).clamp(0.0, 1.0);
                    let px = ax + t * abx;
                    let py = ay + t * aby;
                    if (u - px).hypot(v - py) <= 0.015 {
                        col = [140.0, 206.0, 255.0, 255.0];
                    }

                    acc[0] += col[0];
                    acc[1] += col[1];
                    acc[2] += col[2];
                    acc[3] += col[3];
                }
            }

            let idx = (y * size + x) * 4;
            let samples = 16.0_f32;
            rgba[idx] = (acc[0] / samples).round().clamp(0.0, 255.0) as u8;
            rgba[idx + 1] = (acc[1] / samples).round().clamp(0.0, 255.0) as u8;
            rgba[idx + 2] = (acc[2] / samples).round().clamp(0.0, 255.0) as u8;
            rgba[idx + 3] = (acc[3] / samples).round().clamp(0.0, 255.0) as u8;
        }
    }

    IconData {
        rgba,
        width: size as u32,
        height: size as u32,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picks_splash_then_largest_logo() {
        let dir = std::env::temp_dir().join(format!("victory-art-test-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("xbs_logo_small.png"), vec![0u8; 32]).unwrap();
        std::fs::write(dir.join("xbs_logo_big.png"), vec![0u8; 512]).unwrap();
        assert_eq!(
            find_best_banner_image(&dir).unwrap().file_name().unwrap(),
            "xbs_logo_big.png"
        );
        std::fs::write(dir.join("xbs_splash.png"), vec![0u8; 64]).unwrap();
        assert_eq!(
            find_best_banner_image(&dir).unwrap().file_name().unwrap(),
            "xbs_splash.png"
        );
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn brand_m_icon_renders_non_empty_electric_blue_pixels() {
        let icon = app_icon();
        assert_eq!(icon.width, 64);
        assert_eq!(icon.height, 64);
        let blue_pixels = icon
            .rgba
            .chunks_exact(4)
            .filter(|px| px[3] > 200 && px[2] > 200 && px[2] > px[0] + 80)
            .count();
        assert!(blue_pixels > 400, "expected blue M emblem pixels, got {blue_pixels}");
    }
}
