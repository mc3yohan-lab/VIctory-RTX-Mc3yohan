//! Runtime hero artwork and window icon.
//!
//! No copyrighted artwork is shipped in the repository: when the user selects
//! their own game dump folder, the launcher reads `xbs_splash.png` (or the
//! largest `xbs_logo_*.png`) directly from that local folder at runtime.
//! Otherwise, it renders a dark obsidian-emerald procedural banner.

use std::path::{Path, PathBuf};

use eframe::egui::{ColorImage, IconData};

/// Pick the best banner artwork from a user's local game dump folder.
pub fn pick(game_dir: &Path) -> Option<PathBuf> {
    let splash = game_dir.join("xbs_splash.png");
    if splash.is_file() {
        return Some(splash);
    }
    std::fs::read_dir(game_dir)
        .ok()?
        .flatten()
        .filter(|entry| {
            let name = entry.file_name().to_string_lossy().to_ascii_lowercase();
            name.starts_with("xbs_logo_") && name.ends_with(".png")
        })
        .filter_map(|entry| Some((entry.metadata().ok()?.len(), entry.path())))
        .max_by_key(|(len, _)| *len)
        .map(|(_, path)| path)
}

/// Load and downscale the user's local splash/logo image to at most 1600 px wide.
pub fn from_game(game_dir: &Path) -> Option<ColorImage> {
    let img = image::open(pick(game_dir)?).ok()?;
    let img = if img.width() > 1600 {
        let new_h = (1600 * img.height() / img.width().max(1)).max(1);
        img.resize(1600, new_h, image::imageops::FilterType::Triangle)
    } else {
        img
    };
    let rgba = img.to_rgba8();
    Some(ColorImage::from_rgba_unmultiplied(
        [rgba.width() as usize, rgba.height() as usize],
        rgba.as_raw(),
    ))
}

/// Clean procedural obsidian-emerald-crimson gradient banner used before a
/// game folder is configured (or when the folder contains no PNG banner).
pub fn fallback_banner() -> ColorImage {
    let (w, h) = (320usize, 128usize);
    let mut pixels = Vec::with_capacity(w * h * 4);
    for y in 0..h {
        let ny = y as f32 / (h - 1) as f32;
        for x in 0..w {
            let nx = x as f32 / (w - 1) as f32;
            // Subtle NVIDIA emerald glow top-left fading into deep carbon & warm crimson hint right.
            let g_glow = ((1.0 - nx).powf(2.2) * (1.0 - ny).powf(1.4)).clamp(0.0, 1.0);
            let r_glow = (nx.powf(2.4) * (1.0 - ny * 0.6)).clamp(0.0, 1.0);
            let r = (14.0 + r_glow * 58.0 + g_glow * 8.0).min(255.0) as u8;
            let g = (17.0 + g_glow * 62.0 + r_glow * 14.0).min(255.0) as u8;
            let b = (24.0 + g_glow * 34.0 + r_glow * 24.0).min(255.0) as u8;
            pixels.extend_from_slice(&[r, g, b, 255]);
        }
    }
    ColorImage::from_rgba_unmultiplied([w, h], &pixels)
}

/// Procedural 64x64 RGBA window icon (obsidian rounded badge + crisp emerald-white "V").
pub fn app_icon() -> IconData {
    let size = 64usize;
    let mut rgba = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let fx = x as f32 + 0.5;
            let fy = y as f32 + 0.5;
            // Rounded square SDF (radius 14).
            let dx = (fx - 32.0).abs() - 18.0;
            let dy = (fy - 32.0).abs() - 18.0;
            let out = (dx.max(0.0).hypot(dy.max(0.0)) + dx.max(dy).min(0.0)) - 14.0;
            if out > 0.5 {
                continue;
            }
            let alpha = (0.5 - out).clamp(0.0, 1.0);
            let mut r = 20.0f32;
            let mut g = 24.0f32;
            let mut b = 32.0f32;

            // Emerald accent border.
            if out > -2.6 {
                r = 16.0;
                g = 185.0;
                b = 129.0;
            }

            // Crisp bold "V" strokes from (17,17)->(32,48) and (47,17)->(32,48).
            let dist_seg = |px: f32, py: f32, ax: f32, ay: f32, bx: f32, by: f32| -> f32 {
                let vx = bx - ax;
                let vy = by - ay;
                let t = (((px - ax) * vx + (py - ay) * vy) / (vx * vx + vy * vy)).clamp(0.0, 1.0);
                (px - (ax + vx * t)).hypot(py - (ay + vy * t))
            };
            let d_left = dist_seg(fx, fy, 18.0, 17.0, 32.0, 47.0);
            let d_right = dist_seg(fx, fy, 46.0, 17.0, 32.0, 47.0);
            let d_v = d_left.min(d_right);
            if d_v < 4.8 {
                let blend = (4.8 - d_v).clamp(0.0, 1.0);
                let (tr, tg, tb) = if d_right < d_left {
                    (16.0, 215.0, 148.0)
                } else {
                    (244.0, 247.0, 252.0)
                };
                r = r + (tr - r) * blend;
                g = g + (tg - g) * blend;
                b = b + (tb - b) * blend;
            }

            let idx = (y * size + x) * 4;
            rgba[idx] = r as u8;
            rgba[idx + 1] = g as u8;
            rgba[idx + 2] = b as u8;
            rgba[idx + 3] = (alpha * 255.0) as u8;
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
        let dir = std::env::temp_dir().join(format!("vrtx-art-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(pick(&dir), None);
        std::fs::write(dir.join("xbs_logo_small.png"), [0u8; 16]).unwrap();
        std::fs::write(dir.join("xbs_logo_large.png"), [0u8; 64]).unwrap();
        assert_eq!(pick(&dir), Some(dir.join("xbs_logo_large.png")));
        std::fs::write(dir.join("xbs_splash.png"), [0u8; 8]).unwrap();
        assert_eq!(pick(&dir), Some(dir.join("xbs_splash.png")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
