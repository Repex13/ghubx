//! Runtime colour palette: five presets, a custom accent and the global the
//! UI reads every frame. `ghub_dark` holds the original G HUB tokens verbatim.

use crate::profile::{Appearance, CustomTheme};
use egui::Color32;
use std::sync::RwLock;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Palette {
    pub dark: bool,
    pub bg: Color32,
    pub surface: Color32,
    pub surface_hi: Color32,
    pub button: Color32,
    pub track: Color32,
    pub text: Color32,
    pub text_high: Color32,
    pub text_medium: Color32,
    pub label: Color32,
    pub muted: Color32,
    pub disabled: Color32,
    pub nav: Color32,
    pub outline: Color32,
    pub outline_hi: Color32,
    pub divider: Color32,
    pub row_line: Color32,
    pub leader: Color32,
    pub primary: Color32,
    pub primary_dim: Color32,
    pub primary_hover: Color32,
    pub primary_tint: Color32,
    pub primary_tint_lo: Color32,
    pub primary_disabled: Color32,
    pub on_primary: Color32,
    pub amber: Color32,
    pub amber_tint: Color32,
    pub amber_line: Color32,
    pub gshift: Color32,
    pub error: Color32,
    pub invert: Color32,
    pub on_invert: Color32,
}

/// (id, display name) in picker order.
pub const THEMES: [(&str, &str); 5] = [
    ("ghub-dark", "G HUB Dark"),
    ("ghub-light", "G HUB Light"),
    ("razer-green", "Razer Green"),
    ("midnight", "Midnight"),
    ("ember", "Ember"),
];

const NEUTRAL_ANCHORS: [u8; 7] = [0x00, 0x21, 0x39, 0x5a, 0x88, 0xaa, 0xff];

static PALETTE: RwLock<Palette> = RwLock::new(Palette::ghub_dark());

/// Current palette (a copy; readable from any thread).
pub fn pal() -> Palette {
    *PALETTE.read().unwrap_or_else(|e| e.into_inner())
}

pub fn set_palette(p: Palette) {
    *PALETTE.write().unwrap_or_else(|e| e.into_inner()) = p;
}

const fn hex(h: u32) -> Color32 {
    Color32::from_rgb((h >> 16) as u8, (h >> 8) as u8, h as u8)
}

fn mix(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgb(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()))
}

fn luminance(c: Color32) -> f32 {
    let lin = |v: u8| {
        let v = v as f32 / 255.0;
        if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) }
    };
    0.2126 * lin(c.r()) + 0.7152 * lin(c.g()) + 0.0722 * lin(c.b())
}

/// The explicit colours of a preset; everything else is derived.
struct Seed {
    dark: bool,
    bg: u32,
    surface: u32,
    surface_hi: u32,
    button: u32,
    track: u32,
    disabled: u32,
    muted: u32,
    label: u32,
    text: u32,
    nav: u32,
    divider: u32,
    row_line: u32,
    leader: u32,
    primary: u32,
    amber: u32,
    gshift: u32,
    error: u32,
}

impl Palette {
    pub const fn ghub_dark() -> Self {
        Self {
            dark: true,
            bg: hex(0x000000),
            surface: hex(0x212225),
            surface_hi: hex(0x2a2b30),
            button: hex(0x393939),
            track: hex(0x5a5b5e),
            text: hex(0xffffff),
            text_high: Color32::from_rgba_premultiplied(230, 230, 230, 230),
            text_medium: Color32::from_rgba_premultiplied(153, 153, 153, 153),
            label: hex(0xaaaaaa),
            muted: hex(0x888888),
            disabled: hex(0x6a6a6a),
            nav: Color32::from_rgb(206, 211, 222),
            outline: Color32::from_rgba_premultiplied(26, 26, 26, 26),
            outline_hi: Color32::from_rgba_premultiplied(77, 77, 77, 77),
            divider: hex(0x2b303b),
            row_line: hex(0x404142),
            leader: hex(0x3d3d3d),
            primary: hex(0x1196ff),
            primary_dim: hex(0x1268cb),
            primary_hover: hex(0x3aa8ff),
            primary_tint: hex(0x06263d),
            primary_tint_lo: hex(0x0c1a28),
            primary_disabled: hex(0x0f5a7a),
            on_primary: hex(0xffffff),
            amber: hex(0xffbf00),
            amber_tint: hex(0x2b220e),
            amber_line: hex(0x5c4510),
            gshift: hex(0xff003e),
            error: hex(0xf54155),
            invert: hex(0xffffff),
            on_invert: hex(0x222425),
        }
    }

    fn from_seed(s: Seed) -> Self {
        let (bg, surface, text, amber) = (hex(s.bg), hex(s.surface), hex(s.text), hex(s.amber));
        let mut p = Self {
            dark: s.dark,
            bg,
            surface,
            surface_hi: hex(s.surface_hi),
            button: hex(s.button),
            track: hex(s.track),
            text,
            text_high: text.gamma_multiply(0.9),
            text_medium: text.gamma_multiply(0.6),
            label: hex(s.label),
            muted: hex(s.muted),
            disabled: hex(s.disabled),
            nav: hex(s.nav),
            outline: Color32::TRANSPARENT,
            outline_hi: Color32::TRANSPARENT,
            divider: hex(s.divider),
            row_line: hex(s.row_line),
            leader: hex(s.leader),
            primary: hex(s.primary),
            primary_dim: Color32::TRANSPARENT,
            primary_hover: Color32::TRANSPARENT,
            primary_tint: Color32::TRANSPARENT,
            primary_tint_lo: Color32::TRANSPARENT,
            primary_disabled: Color32::TRANSPARENT,
            on_primary: Color32::WHITE,
            amber,
            amber_tint: mix(bg, amber, 0.17),
            amber_line: mix(bg, amber, 0.36),
            gshift: hex(s.gshift),
            error: hex(s.error),
            invert: text,
            on_invert: surface,
        };
        p.outline = p.ink(26);
        p.outline_hi = p.ink(77);
        p.derive_accent();
        p
    }

    fn derive_accent(&mut self) {
        let p = self.primary;
        self.primary_dim = mix(p, Color32::BLACK, 0.2);
        self.primary_hover = if self.dark { mix(p, Color32::WHITE, 0.17) } else { mix(p, Color32::BLACK, 0.09) };
        self.primary_tint = mix(self.bg, p, 0.24);
        self.primary_tint_lo = mix(self.bg, p, 0.15);
        self.primary_disabled = mix(self.bg, p, 0.45);
        self.on_primary = if luminance(p) > 0.45 { hex(0x0b0b0b) } else { Color32::WHITE };
    }

    pub fn ghub_light() -> Self {
        Self::from_seed(Seed {
            dark: false,
            bg: 0xf0f0f0,
            surface: 0xffffff,
            surface_hi: 0xeceef1,
            button: 0xe1e3e7,
            track: 0xc3c5ca,
            disabled: 0xa4a4a4,
            muted: 0x74767a,
            label: 0x5e5e5e,
            text: 0x18191b,
            nav: 0x3c4048,
            divider: 0xd5d8de,
            row_line: 0xdcdde0,
            leader: 0xb4b6ba,
            primary: 0x1196ff,
            amber: 0xc58a00,
            gshift: 0xe6003a,
            error: 0xe8364c,
        })
    }

    pub fn razer_green() -> Self {
        Self::from_seed(Seed {
            dark: true,
            bg: 0x0a0a0a,
            surface: 0x1a1b1a,
            surface_hi: 0x242624,
            button: 0x333533,
            track: 0x565956,
            disabled: 0x6b6e6b,
            muted: 0x898c89,
            label: 0xadb0ad,
            text: 0xffffff,
            nav: 0xd2d8d2,
            divider: 0x2a2e2a,
            row_line: 0x3e413e,
            leader: 0x3b3d3b,
            primary: 0x44d62c,
            amber: 0xffbf00,
            gshift: 0xff003e,
            error: 0xf54155,
        })
    }

    pub fn midnight() -> Self {
        Self::from_seed(Seed {
            dark: true,
            bg: 0x090b14,
            surface: 0x161929,
            surface_hi: 0x1f2338,
            button: 0x2b3050,
            track: 0x4c5275,
            disabled: 0x636a8c,
            muted: 0x828aab,
            label: 0xa6adc9,
            text: 0xf3f5ff,
            nav: 0xcdd3ee,
            divider: 0x252a45,
            row_line: 0x343a5c,
            leader: 0x343954,
            primary: 0x9e5dff,
            amber: 0xffbf00,
            gshift: 0xff2e63,
            error: 0xf54155,
        })
    }

    pub fn ember() -> Self {
        Self::from_seed(Seed {
            dark: true,
            bg: 0x0c0a09,
            surface: 0x1f1a18,
            surface_hi: 0x2a2320,
            button: 0x3a302c,
            track: 0x5f524c,
            disabled: 0x74665f,
            muted: 0x94867f,
            label: 0xb5a79f,
            text: 0xfff7f2,
            nav: 0xe6d8cf,
            divider: 0x2e2622,
            row_line: 0x433832,
            leader: 0x40362f,
            primary: 0xff5e01,
            amber: 0xffd24a,
            gshift: 0xff003e,
            error: 0xf54155,
        })
    }

    /// Neutral ramp: `level` is the grey value the colour has in the default
    /// dark theme, mapped onto this palette's bg..text scale.
    pub fn n(&self, level: u8) -> Color32 {
        let anchors = [self.bg, self.surface, self.button, self.track, self.muted, self.label, self.text];
        let i = NEUTRAL_ANCHORS.iter().rposition(|&a| a <= level).unwrap_or(0).min(NEUTRAL_ANCHORS.len() - 2);
        let (lo, hi) = (NEUTRAL_ANCHORS[i] as f32, NEUTRAL_ANCHORS[i + 1] as f32);
        mix(anchors[i], anchors[i + 1], (level as f32 - lo) / (hi - lo))
    }

    /// Translucent ink over the background: white on dark themes, black on light.
    pub fn ink(&self, alpha: u8) -> Color32 {
        if self.dark { Color32::from_white_alpha(alpha) } else { Color32::from_black_alpha(alpha) }
    }

    pub fn with_accent(mut self, rgb: [u8; 3]) -> Self {
        self.primary = Color32::from_rgb(rgb[0], rgb[1], rgb[2]);
        self.derive_accent();
        self
    }
}

pub fn palette_for(id: &str) -> Palette {
    match id {
        "ghub-light" => Palette::ghub_light(),
        "razer-green" => Palette::razer_green(),
        "midnight" => Palette::midnight(),
        "ember" => Palette::ember(),
        _ => Palette::ghub_dark(),
    }
}

fn to_hex(c: Color32) -> u32 {
    ((c.r() as u32) << 16) | ((c.g() as u32) << 8) | c.b() as u32
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

/// WCAG contrast ratio between two colours (1.0 to 21.0).
pub fn contrast(a: Color32, b: Color32) -> f32 {
    let (la, lb) = (luminance(a), luminance(b));
    (la.max(lb) + 0.05) / (la.min(lb) + 0.05)
}

/// Palette of a user-made theme: four chosen colours, everything else derived.
pub fn custom_palette(t: &CustomTheme) -> Palette {
    let (bg, surface, text) = (rgb(t.bg), rgb(t.surface), rgb(t.text));
    let dark = luminance(text) >= luminance(bg);
    let m = |a: Color32, b: Color32, f: f32| to_hex(mix(a, b, f));
    let (amber, gshift, error) = if dark { (0xffbf00, 0xff003e, 0xf54155) } else { (0xc58a00, 0xe6003a, 0xe8364c) };
    Palette::from_seed(Seed {
        dark,
        bg: to_hex(bg),
        surface: to_hex(surface),
        surface_hi: m(surface, text, 0.045),
        button: m(surface, text, 0.11),
        track: m(surface, text, 0.26),
        disabled: m(bg, text, 0.42),
        muted: m(bg, text, 0.53),
        label: m(bg, text, 0.67),
        text: to_hex(text),
        nav: m(bg, text, 0.82),
        divider: m(bg, text, 0.17),
        row_line: m(bg, text, 0.25),
        leader: m(bg, text, 0.24),
        primary: to_hex(rgb(t.accent)),
        amber,
        gshift,
        error,
    })
}

/// The theme's palette (preset or custom) with the accent precedence: lighting
/// colour (when following and available), then the custom accent, then the theme's own.
pub fn resolve(a: &Appearance, lighting_accent: Option<[u8; 3]>) -> Palette {
    let p = match a.custom.iter().find(|c| c.id == a.theme) {
        Some(t) => custom_palette(t),
        None => palette_for(&a.theme),
    };
    match (a.follow_lighting.then_some(lighting_accent).flatten(), a.accent) {
        (Some(c), _) | (None, Some(c)) => p.with_accent(c),
        (None, None) => p,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn all() -> Vec<Palette> {
        THEMES.iter().map(|(id, _)| palette_for(id)).collect()
    }

    #[test]
    fn default_palette_matches_legacy_tokens() {
        let p = Palette::ghub_dark();
        assert!(p.dark);
        assert_eq!(p.bg, Color32::from_rgb(0x00, 0x00, 0x00));
        assert_eq!(p.surface, Color32::from_rgb(0x21, 0x22, 0x25));
        assert_eq!(p.surface_hi, Color32::from_rgb(0x2a, 0x2b, 0x30));
        assert_eq!(p.button, Color32::from_rgb(0x39, 0x39, 0x39));
        assert_eq!(p.track, Color32::from_rgb(0x5a, 0x5b, 0x5e));
        assert_eq!(p.text, Color32::WHITE);
        assert_eq!(p.text_high, Color32::from_rgba_premultiplied(230, 230, 230, 230));
        assert_eq!(p.text_medium, Color32::from_rgba_premultiplied(153, 153, 153, 153));
        assert_eq!(p.label, Color32::from_rgb(0xaa, 0xaa, 0xaa));
        assert_eq!(p.muted, Color32::from_rgb(0x88, 0x88, 0x88));
        assert_eq!(p.disabled, Color32::from_rgb(0x6a, 0x6a, 0x6a));
        assert_eq!(p.nav, Color32::from_rgb(206, 211, 222));
        assert_eq!(p.outline, Color32::from_rgba_premultiplied(26, 26, 26, 26));
        assert_eq!(p.outline_hi, Color32::from_rgba_premultiplied(77, 77, 77, 77));
        assert_eq!(p.divider, Color32::from_rgb(0x2b, 0x30, 0x3b));
        assert_eq!(p.row_line, Color32::from_rgb(0x40, 0x41, 0x42));
        assert_eq!(p.leader, Color32::from_rgb(0x3d, 0x3d, 0x3d));
        assert_eq!(p.primary, Color32::from_rgb(0x11, 0x96, 0xff));
        assert_eq!(p.primary_dim, Color32::from_rgb(0x12, 0x68, 0xcb));
        assert_eq!(p.primary_hover, Color32::from_rgb(0x3a, 0xa8, 0xff));
        assert_eq!(p.primary_tint, Color32::from_rgb(0x06, 0x26, 0x3d));
        assert_eq!(p.primary_tint_lo, Color32::from_rgb(0x0c, 0x1a, 0x28));
        assert_eq!(p.primary_disabled, Color32::from_rgb(0x0f, 0x5a, 0x7a));
        assert_eq!(p.on_primary, Color32::WHITE);
        assert_eq!(p.amber, Color32::from_rgb(0xff, 0xbf, 0x00));
        assert_eq!(p.amber_tint, Color32::from_rgb(0x2b, 0x22, 0x0e));
        assert_eq!(p.amber_line, Color32::from_rgb(0x5c, 0x45, 0x10));
        assert_eq!(p.gshift, Color32::from_rgb(0xff, 0x00, 0x3e));
        assert_eq!(p.error, Color32::from_rgb(0xf5, 0x41, 0x55));
        assert_eq!(p.invert, Color32::WHITE);
        assert_eq!(p.on_invert, Color32::from_rgb(0x22, 0x24, 0x25));
    }

    #[test]
    fn neutral_ramp_hits_anchors() {
        for p in all() {
            assert_eq!(p.n(0x00), p.bg);
            assert_eq!(p.n(0x21), p.surface);
            assert_eq!(p.n(0x39), p.button);
            assert_eq!(p.n(0x5a), p.track);
            assert_eq!(p.n(0x88), p.muted);
            assert_eq!(p.n(0xaa), p.label);
            assert_eq!(p.n(0xff), p.text);
        }
        let c = Palette::ghub_dark().n(0x2b);
        for (got, want) in [(c.r(), 0x2b), (c.g(), 0x2b), (c.b(), 0x2d)] {
            assert!((got as i32 - want).abs() <= 2, "{c:?}");
        }
    }

    #[test]
    fn ink_follows_theme() {
        assert_eq!(Palette::ghub_dark().ink(26), Color32::from_white_alpha(26));
        assert_eq!(Palette::ghub_light().ink(26), Color32::from_black_alpha(26));
    }

    #[test]
    fn accent_override_picks_readable_on_colour() {
        let green = Palette::ghub_dark().with_accent([0x44, 0xd6, 0x2c]);
        assert_eq!(green.primary, Color32::from_rgb(0x44, 0xd6, 0x2c));
        assert_eq!(green.on_primary, Color32::from_rgb(0x0b, 0x0b, 0x0b));
        assert_eq!(Palette::ghub_dark().with_accent([0x11, 0x96, 0xff]).on_primary, Color32::WHITE);
    }

    fn sunset() -> CustomTheme {
        CustomTheme { id: "custom-sunset".into(), name: "Sunset".into(), bg: [0x1A, 0x10, 0x23], surface: [0x2A, 0x1B, 0x3D], text: [0xF6, 0xEE, 0xFC], accent: [0xFF, 0x7A, 0x59] }
    }

    fn paper() -> CustomTheme {
        CustomTheme { id: "custom-paper".into(), name: "Paper".into(), bg: [0xF4, 0xF1, 0xEA], surface: [0xFF, 0xFF, 0xFF], text: [0x1D, 0x1C, 0x1A], accent: [0xC2, 0x41, 0x0C] }
    }

    #[test]
    fn custom_palette_keeps_the_chosen_colors() {
        for t in [sunset(), paper()] {
            let p = custom_palette(&t);
            assert_eq!((p.bg, p.surface, p.text, p.primary), (rgb(t.bg), rgb(t.surface), rgb(t.text), rgb(t.accent)));
        }
        assert!(custom_palette(&sunset()).dark);
        let light = custom_palette(&paper());
        assert!(!light.dark);
        assert_eq!(light.ink(26), Color32::from_black_alpha(26));
    }

    #[test]
    fn custom_neutral_ramp_moves_toward_the_text_color() {
        for t in [sunset(), paper()] {
            let p = custom_palette(&t);
            let lt = luminance(p.text);
            let d: Vec<f32> = [p.button, p.track, p.muted, p.label, p.text].iter().map(|c| (luminance(*c) - lt).abs()).collect();
            assert!(d.windows(2).all(|w| w[1] < w[0]), "{} {d:?}", t.name);
        }
    }

    #[test]
    fn contrast_ratio() {
        assert!((contrast(Color32::BLACK, Color32::WHITE) - 21.0).abs() < 0.01);
        let c = Color32::from_rgb(10, 120, 200);
        assert_eq!(contrast(c, c), 1.0);
        let d = Color32::from_rgb(240, 30, 60);
        assert_eq!(contrast(c, d), contrast(d, c));
    }

    #[test]
    fn resolve_custom_themes() {
        let t = sunset();
        let mut a = Appearance { theme: t.id.clone(), custom: vec![t.clone()], ..Appearance::default() };
        assert_eq!(resolve(&a, None), custom_palette(&t));
        a.accent = Some([1, 2, 3]);
        let p = resolve(&a, None);
        assert_eq!((p.primary, p.bg), (Color32::from_rgb(1, 2, 3), rgb(t.bg)));
        let missing = Appearance { theme: "custom-gone".into(), ..Appearance::default() };
        assert_eq!(resolve(&missing, None), Palette::ghub_dark());
    }

    #[test]
    fn unknown_theme_falls_back() {
        assert_eq!(palette_for("nope"), Palette::ghub_dark());
    }

    #[test]
    fn resolve_precedence() {
        let lighting = [0x10, 0x20, 0x30];
        let custom = [0xaa, 0xbb, 0xcc];
        let mut a = Appearance { theme: "ember".into(), accent: Some(custom), follow_lighting: true, ..Appearance::default() };
        assert_eq!(resolve(&a, Some(lighting)).primary, Color32::from_rgb(0x10, 0x20, 0x30));
        assert_eq!(resolve(&a, None).primary, Color32::from_rgb(0xaa, 0xbb, 0xcc));
        a.follow_lighting = false;
        assert_eq!(resolve(&a, Some(lighting)).primary, Color32::from_rgb(0xaa, 0xbb, 0xcc));
        a.accent = None;
        assert_eq!(resolve(&a, Some(lighting)), Palette::ember());
    }
}
