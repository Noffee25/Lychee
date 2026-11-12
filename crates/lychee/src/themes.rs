//! The colour themes in Settings → Appearance. Each sets the neutral colours (`Theme.colors`) and a
//! default accent, used when colours from album art are off or nothing is playing.
//!
//! Catppuccin, Tokyo Night, Gruvbox and Rosé Pine are adapted from those projects' published
//! palettes (open-source licences, credited in Settings → About); the rest are Lychee's own.

use slint::Color;

use crate::ThemeColors;

pub struct Def {
    pub id: &'static str,
    pub name: &'static str,
    pub blurb: &'static str,
    pub dark: bool,
    /// RGB accent colour.
    pub accent: u32,
    /// RGB colour the album-header tint is derived from when there's no artwork.
    pub tint: u32,
    /// bg, side, surface, surface-2, hover, pressed, border, border-2, text, text-2, text-3 (RGB).
    pub colors: [u32; 11],
}

pub const DEFAULT_DARK: &str = "flow";
pub const DEFAULT_LIGHT: &str = "flow-light";

pub const THEMES: &[Def] = &[
    Def {
        id: "flow",
        name: "Flow",
        blurb: "The original: neutral graphite with a coral accent.",
        dark: true,
        accent: 0xFF6B7D,
        tint: 0x5A6B9A,
        colors: [
            0x0B0B0D, 0x0E0E11, 0x131317, 0x19191E, 0x1E1E24, 0x26262D,
            0x222228, 0x2C2C34, 0xF4F4F5, 0xA1A1AA, 0x6B6B76,
        ],
    },
    Def {
        id: "blackout",
        name: "Blackout",
        blurb: "True black for OLED screens and dark rooms.",
        dark: true,
        accent: 0xFF6B7D,
        tint: 0x5A6B9A,
        colors: [
            0x000000, 0x000000, 0x0A0A0B, 0x111113, 0x18181B, 0x202024,
            0x1A1A1D, 0x27272C, 0xF5F5F5, 0xA3A3A8, 0x6E6E75,
        ],
    },
    Def {
        id: "catppuccin-mocha",
        name: "Catppuccin Mocha",
        blurb: "Soothing pastels on a soft blue-grey, from the Catppuccin palette.",
        dark: true,
        accent: 0xCBA6F7,
        tint: 0x6A5A9A,
        colors: [
            0x1E1E2E, 0x181825, 0x232334, 0x2A2B3C, 0x313244, 0x45475A,
            0x313244, 0x45475A, 0xCDD6F4, 0xA6ADC8, 0x7F849C,
        ],
    },
    Def {
        id: "tokyo-night",
        name: "Tokyo Night",
        blurb: "Deep navy and neon blue, from the Tokyo Night palette.",
        dark: true,
        accent: 0x7AA2F7,
        tint: 0x3A4F8A,
        colors: [
            0x1A1B26, 0x16161E, 0x1F2030, 0x24283B, 0x292E42, 0x343A55,
            0x232535, 0x2F3549, 0xC0CAF5, 0xA9B1D6, 0x737AA2,
        ],
    },
    Def {
        id: "gruvbox",
        name: "Gruvbox",
        blurb: "Retro warm browns, cream text and orange, from the Gruvbox palette.",
        dark: true,
        accent: 0xFE8019,
        tint: 0x8A5A3A,
        colors: [
            0x282828, 0x1D2021, 0x2F2E2C, 0x32302F, 0x3C3836, 0x504945,
            0x3C3836, 0x504945, 0xEBDBB2, 0xBDAE93, 0x928374,
        ],
    },
    Def {
        id: "rose-pine",
        name: "Rosé Pine",
        blurb: "Dusky purple and dusty rose, from the Rosé Pine palette.",
        dark: true,
        accent: 0xEBBCBA,
        tint: 0x6A3A6E,
        colors: [
            0x191724, 0x1F1D2E, 0x1F1D2E, 0x26233A, 0x2C2943, 0x403D52,
            0x26233A, 0x403D52, 0xE0DEF4, 0x908CAA, 0x6E6A86,
        ],
    },
    Def {
        id: "moss",
        name: "Moss",
        blurb: "Forest-floor greens with a sage accent.",
        dark: true,
        accent: 0x7DD3A0,
        tint: 0x3E6B52,
        colors: [
            0x0B110E, 0x0D1411, 0x111A15, 0x17221C, 0x1C2922, 0x23332A,
            0x1C2720, 0x27342C, 0xE9F0EA, 0xA2B5A8, 0x6C8073,
        ],
    },
    Def {
        id: "afterglow",
        name: "Afterglow",
        blurb: "Synthwave indigo with an electric cyan accent.",
        dark: true,
        accent: 0x45E3E8,
        tint: 0x4A3A9A,
        colors: [
            0x0D0A1C, 0x100C22, 0x15102B, 0x1C1637, 0x241C44, 0x2C2352,
            0x241C40, 0x312754, 0xF2EEFF, 0xACA3D3, 0x756D9C,
        ],
    },
    Def {
        id: "flow-light",
        name: "Flow Light",
        blurb: "The original, in daylight.",
        dark: false,
        accent: 0xFF6B7D,
        tint: 0x5A6B9A,
        colors: [
            0xFFFFFF, 0xF7F7F8, 0xFFFFFF, 0xF3F3F5, 0xEDEDF0, 0xE4E4E8,
            0xE6E6EA, 0xD9D9DE, 0x0B0B0D, 0x55555F, 0x8B8B95,
        ],
    },
    Def {
        id: "liner-notes",
        name: "Liner Notes",
        blurb: "Warm paper and vermilion ink, like a gatefold sleeve.",
        dark: false,
        accent: 0xD9482B,
        tint: 0x8A5A3A,
        colors: [
            0xFBF8F2, 0xF4EFE5, 0xFFFDF8, 0xF0EADF, 0xEAE3D5, 0xE1D8C8,
            0xE6DECF, 0xD8CDBB, 0x1E1A16, 0x5C5349, 0x8A8073,
        ],
    },
    Def {
        id: "lychee",
        name: "Lychee",
        blurb: "Blush pink and lychee red, the fruit itself.",
        dark: false,
        accent: 0xE0284F,
        tint: 0x8A3A4A,
        colors: [
            0xFFF8F8, 0xFCEFF0, 0xFFFFFF, 0xFBEEEF, 0xF7E4E6, 0xF0D6D9,
            0xF2DFE1, 0xE8CDD1, 0x2A1418, 0x6E4F55, 0x9A7D83,
        ],
    },
];

/// The theme with this id, or the default dark or light one.
pub fn get(id: &str, dark: bool) -> &'static Def {
    THEMES
        .iter()
        .find(|t| t.id == id && t.dark == dark)
        .or_else(|| THEMES.iter().find(|t| t.id == if dark { DEFAULT_DARK } else { DEFAULT_LIGHT }))
        .unwrap_or(&THEMES[0])
}

pub fn rgb(rgb: u32) -> Color {
    Color::from_rgb_u8((rgb >> 16) as u8, (rgb >> 8) as u8, rgb as u8)
}

impl Def {
    pub fn slint_colors(&self) -> ThemeColors {
        let c = self.colors.map(rgb);
        ThemeColors {
            dark: self.dark,
            bg: c[0],
            side: c[1],
            surface: c[2],
            surface_2: c[3],
            hover: c[4],
            pressed: c[5],
            border: c[6],
            border_2: c[7],
            text: c[8],
            text_2: c[9],
            text_3: c[10],
        }
    }
}

/// Relative luminance (WCAG).
fn luminance(rgb: u32) -> f32 {
    let channel = |shift: u32| {
        let c = ((rgb >> shift) & 0xff) as f32 / 255.0;
        if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * channel(16) + 0.7152 * channel(8) + 0.0722 * channel(0)
}

fn contrast(a: u32, b: u32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Text colour for an accent fill: near-black on light accents, white on dark ones.
pub fn on_accent(accent: u32) -> Color {
    if luminance(accent) > 0.18 { rgb(0x16100F) } else { rgb(0xFFFFFF) }
}

/// The accent, darkened on light themes until it reads as text on the background (3:1). Album art
/// gives light, vivid accents that vanish on white ("Low Tide" in pale amber).
pub fn readable_accent(accent: u32, theme: &Def) -> u32 {
    if theme.dark {
        return accent;
    }
    let bg = theme.colors[0];
    let mut c = accent;
    for _ in 0..20 {
        if contrast(c, bg) >= 3.0 {
            break;
        }
        let darker = |shift: u32| (((c >> shift) & 0xff) as f32 * 0.92) as u32;
        c = darker(16) << 16 | darker(8) << 8 | darker(0);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ids_are_unique_and_defaults_exist() {
        for (i, t) in THEMES.iter().enumerate() {
            assert!(THEMES[i + 1..].iter().all(|o| o.id != t.id), "{} twice", t.id);
        }
        assert_eq!(get("nope", true).id, DEFAULT_DARK);
        assert_eq!(get("nope", false).id, DEFAULT_LIGHT);
        assert_eq!(get("flow", false).id, DEFAULT_LIGHT, "a dark theme isn't used for light mode");
    }

    #[test]
    fn accents_get_readable_text() {
        assert_eq!(on_accent(0xFFD60A), rgb(0x16100F));
        assert_eq!(on_accent(0xE0284F), rgb(0xFFFFFF));
    }

    #[test]
    fn light_themes_darken_pale_accents() {
        let light = get(DEFAULT_LIGHT, false);
        let amber = readable_accent(0xF2B24C, light);
        assert!(contrast(amber, light.colors[0]) >= 3.0);
        assert_eq!(readable_accent(0x2F6BFF, light), 0x2F6BFF, "dark enough already");
        assert_eq!(readable_accent(0xF2B24C, get(DEFAULT_DARK, true)), 0xF2B24C);
    }
}
