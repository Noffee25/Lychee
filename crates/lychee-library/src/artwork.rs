//! Album artwork: cache files and accent colours.
//!
//! Each distinct image (identified by the MD5 of its bytes) is stored twice in the cache
//! directory: a 320 px thumbnail for grids and lists, and a 720 px version for large views.

use std::fs;
use std::io::BufWriter;
use std::path::{Path, PathBuf};

use image::imageops::FilterType;
use image::{DynamicImage, RgbImage};
use md5::{Digest, Md5};

pub const THUMB: u32 = 320;
pub const LARGE: u32 = 720;

#[derive(Clone, Debug)]
pub struct ArtworkCache {
    dir: PathBuf,
}

impl ArtworkCache {
    pub fn new(dir: PathBuf) -> std::io::Result<Self> {
        fs::create_dir_all(&dir)?;
        Ok(ArtworkCache { dir })
    }

    pub fn key_for(bytes: &[u8]) -> String {
        let digest = Md5::digest(bytes);
        digest.iter().map(|b| format!("{b:02x}")).collect()
    }

    pub fn thumb_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}.jpg"))
    }

    pub fn large_path(&self, key: &str) -> PathBuf {
        self.dir.join(format!("{key}@{LARGE}.jpg"))
    }

    pub fn has(&self, key: &str) -> bool {
        self.thumb_path(key).exists() && self.large_path(key).exists()
    }

    /// Decode `bytes`, write both cache sizes and return the (accent, tint) colours.
    pub fn store(&self, key: &str, bytes: &[u8]) -> Result<Colors, image::ImageError> {
        let img = image::load_from_memory(bytes)?;
        let square = center_square(img);
        let large = square.resize_exact(LARGE.min(square.width()), LARGE.min(square.height()), FilterType::Lanczos3);
        save_jpeg(&large.to_rgb8(), &self.large_path(key))?;
        let thumb = large.resize_exact(THUMB, THUMB, FilterType::Lanczos3).to_rgb8();
        save_jpeg(&thumb, &self.thumb_path(key))?;
        Ok(colors(&thumb))
    }
}

fn center_square(img: DynamicImage) -> DynamicImage {
    let (w, h) = (img.width(), img.height());
    // Only crop clearly non-square art; scans that are a few pixels off stay untouched.
    if w.abs_diff(h) * 50 <= w.max(h) {
        return img;
    }
    let side = w.min(h);
    img.crop_imm((w - side) / 2, (h - side) / 2, side, side)
}

fn save_jpeg(img: &RgbImage, path: &Path) -> Result<(), image::ImageError> {
    let tmp = path.with_extension("tmp");
    {
        let file = fs::File::create(&tmp)?;
        let mut encoder = image::codecs::jpeg::JpegEncoder::new_with_quality(BufWriter::new(file), 90);
        encoder.encode_image(img)?;
    }
    fs::rename(&tmp, path)?;
    Ok(())
}

/// Look for a cover image next to the music: `cover.jpg`, `folder.png`, `front.webp`…
/// Falls back to the only image in the folder, if there is exactly one.
pub fn folder_image(dir: &Path) -> Option<PathBuf> {
    const NAMES: &[&str] = &["cover", "folder", "front", "album", "albumart", "artwork"];
    const EXTS: &[&str] = &["jpg", "jpeg", "png", "webp"];
    let mut images = Vec::new();
    for entry in fs::read_dir(dir).ok()?.flatten() {
        let path = entry.path();
        let Some(ext) = path.extension().and_then(|e| e.to_str()) else { continue };
        if !EXTS.iter().any(|e| e.eq_ignore_ascii_case(ext)) {
            continue;
        }
        images.push(path);
    }
    images.sort();
    for name in NAMES {
        if let Some(found) = images.iter().find(|p| {
            p.file_stem().and_then(|s| s.to_str()).is_some_and(|s| s.eq_ignore_ascii_case(name))
        }) {
            return Some(found.clone());
        }
    }
    if images.len() == 1 { images.pop() } else { None }
}

/// Colours derived from a cover: `accent` is its most vivid colour (buttons, highlights) and
/// `tint` its dominant colour (header backgrounds). Both are `None` for greyscale art.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Colors {
    pub accent: Option<u32>,
    pub tint: Option<u32>,
}

pub fn colors(img: &RgbImage) -> Colors {
    let small = image::imageops::resize(img, 48, 48, FilterType::Triangle);
    const BUCKETS: usize = 36;
    let mut vivid = [0f32; BUCKETS];
    let mut area = [0f32; BUCKETS];
    let mut vivid_sum = [[0f32; 3]; BUCKETS];
    let mut area_sum = [[0f32; 3]; BUCKETS];
    for px in small.pixels() {
        let [r, g, b] = px.0.map(|c| f32::from(c) / 255.0);
        let (h, s, l) = rgb_to_hsl(r, g, b);
        if s < 0.2 || !(0.08..=0.94).contains(&l) {
            continue;
        }
        let i = ((h / 360.0) * BUCKETS as f32) as usize % BUCKETS;
        // Vivid: strongly favour saturated, mid-to-light colours, the way an accent reads.
        let v = s * s * s * (1.0 - (l - 0.62).abs() * 2.2).max(0.01);
        vivid[i] += v;
        vivid_sum[i] = [vivid_sum[i][0] + r * v, vivid_sum[i][1] + g * v, vivid_sum[i][2] + b * v];
        // Dominant: how much of the cover a hue covers.
        let a = s.sqrt();
        area[i] += a;
        area_sum[i] = [area_sum[i][0] + r * a, area_sum[i][1] + g * a, area_sum[i][2] + b * a];
    }
    let pick = |weights: &[f32; BUCKETS], min: f32| {
        (0..BUCKETS)
            .max_by(|&x, &y| {
                let score = |i: usize| weights[i] + 0.5 * (weights[(i + 1) % BUCKETS] + weights[(i + BUCKETS - 1) % BUCKETS]);
                score(x).total_cmp(&score(y))
            })
            .filter(|&i| weights[i] > min)
    };
    let to_rgb = |(r, g, b): (f32, f32, f32)| ((r * 255.0) as u32) << 16 | ((g * 255.0) as u32) << 8 | (b * 255.0) as u32;
    let accent = pick(&vivid, 0.8).map(|i| {
        let w = vivid[i];
        let (h, s, l) = rgb_to_hsl(vivid_sum[i][0] / w, vivid_sum[i][1] / w, vivid_sum[i][2] / w);
        // Keep it vivid but readable on both dark and light surfaces.
        to_rgb(hsl_to_rgb(h, s.clamp(0.55, 0.9), l.clamp(0.56, 0.68)))
    });
    let tint = pick(&area, 4.0).map(|i| {
        let w = area[i];
        to_rgb((area_sum[i][0] / w, area_sum[i][1] / w, area_sum[i][2] / w))
    });
    Colors { accent, tint }
}

/// The accent alone, for callers that don't need the tint.
#[cfg(test)]
fn accent_color(img: &RgbImage) -> Option<u32> {
    colors(img).accent
}

fn rgb_to_hsl(r: f32, g: f32, b: f32) -> (f32, f32, f32) {
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let l = (max + min) / 2.0;
    if (max - min).abs() < f32::EPSILON {
        return (0.0, 0.0, l);
    }
    let d = max - min;
    let s = if l > 0.5 { d / (2.0 - max - min) } else { d / (max + min) };
    let h = if max == r {
        (g - b) / d + if g < b { 6.0 } else { 0.0 }
    } else if max == g {
        (b - r) / d + 2.0
    } else {
        (r - g) / d + 4.0
    };
    (h * 60.0, s, l)
}

fn hsl_to_rgb(h: f32, s: f32, l: f32) -> (f32, f32, f32) {
    let c = (1.0 - (2.0 * l - 1.0).abs()) * s;
    let hp = h / 60.0;
    let x = c * (1.0 - (hp % 2.0 - 1.0).abs());
    let (r, g, b) = match hp as u32 {
        0 => (c, x, 0.0),
        1 => (x, c, 0.0),
        2 => (0.0, c, x),
        3 => (0.0, x, c),
        4 => (x, 0.0, c),
        _ => (c, 0.0, x),
    };
    let m = l - c / 2.0;
    (r + m, g + m, b + m)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accent_of_a_mostly_red_image_is_red() {
        let mut img = RgbImage::new(64, 64);
        for (x, _, px) in img.enumerate_pixels_mut() {
            *px = if x < 48 { image::Rgb([200, 40, 50]) } else { image::Rgb([20, 20, 20]) };
        }
        let accent = accent_color(&img).expect("colourful");
        let (r, g, b) = (accent >> 16 & 0xff, accent >> 8 & 0xff, accent & 0xff);
        assert!(r > g + 60 && r > b + 60, "{r} {g} {b}");
    }

    #[test]
    fn accent_prefers_vivid_colours_and_tint_the_dominant_one() {
        // A big muted blue field with a small saturated coral "sun", like a sunset cover.
        let img = RgbImage::from_fn(64, 64, |x, y| {
            let dx = x as i32 - 32;
            let dy = y as i32 - 24;
            if dx * dx + dy * dy < 120 { image::Rgb([255, 111, 97]) } else { image::Rgb([30, 58, 85]) }
        });
        let c = colors(&img);
        let (r, _, b) = (c.accent.unwrap() >> 16 & 0xff, 0, c.accent.unwrap() & 0xff);
        assert!(r > b, "accent should be the coral, got {:06x}", c.accent.unwrap());
        let t = c.tint.unwrap();
        assert!((t & 0xff) > (t >> 16 & 0xff), "tint should be the blue, got {t:06x}");
    }

    #[test]
    fn greyscale_images_have_no_accent() {
        let img = RgbImage::from_pixel(64, 64, image::Rgb([120, 120, 120]));
        assert_eq!(accent_color(&img), None);
    }
}
