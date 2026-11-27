//! Carga de arte de portada en tiempo de ejecución desde la carpeta local del
//! juego (cero imágenes con copyright en Git) + ícono procedural Quantico.

use std::path::{Path, PathBuf};

use eframe::egui::{self, ColorImage, IconData, TextureHandle, TextureOptions};

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
        let resized = image::imageops::resize(&img, 1600, nh, image::imageops::FilterType::Triangle);
        let (rw, rh) = resized.dimensions();
        (rw as usize, rh as usize, resized.into_raw())
    } else {
        (w as usize, h as usize, img.into_raw())
    };
    let color = ColorImage::from_rgba_unmultiplied([w, h], &raw);
    Some(ctx.load_texture("quantico_banner_art", color, TextureOptions::LINEAR))
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

/// Ícono procedural de ventana 64×64: anillo blanco Quantico + barra diagonal azul #007BFF.
pub fn app_icon() -> IconData {
    let size = 64usize;
    let mut rgba = vec![0u8; size * size * 4];
    let cx = 31.5_f32;
    let cy = 31.5_f32;
    for y in 0..size {
        for x in 0..size {
            let idx = (y * size + x) * 4;
            let fx = x as f32;
            let fy = y as f32;
            let dx = fx - cx;
            let dy = fy - cy;
            let r = (dx * dx + dy * dy).sqrt();

            // Fondo carbón Quantico redondeado.
            let corner_x = (dx.abs() - 18.0).max(0.0);
            let corner_y = (dy.abs() - 18.0).max(0.0);
            if corner_x * corner_x + corner_y * corner_y > 14.0 * 14.0 {
                continue;
            }
            let mut col = [10u8, 14, 22, 255];

            // Borde sutil azul Quantico.
            if dx.abs() > 29.0 || dy.abs() > 29.0 {
                col = [0, 123, 255, 220];
            }

            // Anillo blanco de la Q.
            if (13.0..=19.2).contains(&r) {
                col = [248, 250, 252, 255];
            }

            // Barra diagonal azul Quantico (#007BFF) hacia abajo-derecha (53.8°).
            let proj = (dx - 3.0) * 0.68 + (dy - 3.0) * 0.73;
            let perp = ((dx - 3.0) * 0.73 - (dy - 3.0) * 0.68).abs();
            if proj >= 0.0 && proj <= 24.5 && perp <= 3.6 {
                col = [0, 123, 255, 255];
            }

            rgba[idx..idx + 4].copy_from_slice(&col);
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
}
