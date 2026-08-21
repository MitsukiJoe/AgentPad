use eframe::egui::{Color32, ColorImage};
use qrcode::QrCode;

pub const QR_PT: f32 = 260.0;

pub fn raster_px(scale: f32) -> u32 {
    (QR_PT * scale).round().max(1.0) as u32
}

pub fn color_image(payload: &str, px: u32, dark: bool) -> Result<ColorImage, String> {
    let code = QrCode::new(payload.as_bytes()).map_err(|e| e.to_string())?;
    let w = px as usize;
    let n = code.width();
    if n == 0 {
        return Err("empty qr".into());
    }
    let quiet = 4usize;
    let dim = n + quiet * 2;
    let mark = if dark { Color32::WHITE } else { Color32::BLACK };
    let mut pixels = vec![Color32::TRANSPARENT; w * w];
    for y in 0..w {
        for x in 0..w {
            let mx = x * dim / w;
            let my = y * dim / w;
            if mx >= quiet && my >= quiet && mx < quiet + n && my < quiet + n {
                if code[(mx - quiet, my - quiet)] == qrcode::Color::Dark {
                    pixels[y * w + x] = mark;
                }
            }
        }
    }
    Ok(ColorImage::new([w, w], pixels))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_follows_scale_not_display_size() {
        assert_eq!(raster_px(1.0), 260);
        assert_eq!(raster_px(2.0), 520);
        assert_eq!(raster_px(1.5), 390);
        assert_eq!(QR_PT, 260.0);
    }

    #[test]
    fn renders_at_retina_pixels() {
        let img = color_image("{\"v\":1}", 64, false).unwrap();
        assert_eq!(img.size, [64, 64]);
        assert!(img.pixels.iter().any(|p| *p == Color32::BLACK));
        assert_eq!(img.pixels[0], Color32::TRANSPARENT);
        assert_eq!(img.pixels[63], Color32::TRANSPARENT);
    }

    #[test]
    fn dark_inverts_modules() {
        let img = color_image("{\"v\":1}", 64, true).unwrap();
        assert!(img.pixels.iter().any(|p| *p == Color32::WHITE));
        assert!(!img.pixels.iter().any(|p| *p == Color32::BLACK));
        assert_eq!(img.pixels[0], Color32::TRANSPARENT);
    }
}
