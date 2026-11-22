//! Runtime assets taken from the local G HUB install (nothing is bundled):
//! Brown Pro fonts out of `resources/app.asar` and the G502 HERO renders +
//! hotspot/zone metadata out of the `g502_hero` depot.

use egui::{Color32, ColorImage, FontData, FontDefinitions, FontFamily, TextureHandle, TextureOptions, Vec2};
use serde::Deserialize;
use std::collections::HashMap;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::sync::Arc;

pub const ASAR: &str = r"C:\Program Files\LGHUB\resources\app.asar";
pub const DEPOTS: &str = r"C:\ProgramData\LGHUB\depots";

// ---------------------------------------------------------------- asar

/// Minimal ASAR reader: header JSON + file offsets.
pub struct Asar {
    file: std::fs::File,
    base: u64,
    entries: HashMap<String, (u64, u64)>,
}

impl Asar {
    pub fn open(path: &str) -> Option<Self> {
        let mut file = std::fs::File::open(path).ok()?;
        let mut head = [0u8; 16];
        file.read_exact(&mut head).ok()?;
        let header_size = u32::from_le_bytes(head[4..8].try_into().ok()?) as u64;
        let json_len = u32::from_le_bytes(head[12..16].try_into().ok()?) as usize;
        let mut json = vec![0u8; json_len];
        file.read_exact(&mut json).ok()?;
        let root: serde_json::Value = serde_json::from_slice(&json).ok()?;
        let mut entries = HashMap::new();
        fn walk(v: &serde_json::Value, prefix: &str, out: &mut HashMap<String, (u64, u64)>) {
            let Some(files) = v.get("files").and_then(|f| f.as_object()) else { return };
            for (name, e) in files {
                let path = if prefix.is_empty() { name.clone() } else { format!("{prefix}/{name}") };
                if e.get("files").is_some() {
                    walk(e, &path, out);
                } else if let (Some(off), Some(size)) = (e.get("offset").and_then(|o| o.as_str()), e.get("size").and_then(|s| s.as_u64()))
                    && let Ok(off) = off.parse::<u64>()
                {
                    out.insert(path, (off, size));
                }
            }
        }
        walk(&root, "", &mut entries);
        Some(Self { file, base: 8 + header_size, entries })
    }

    pub fn read(&mut self, path: &str) -> Option<Vec<u8>> {
        let &(off, size) = self.entries.get(path)?;
        self.file.seek(SeekFrom::Start(self.base + off)).ok()?;
        let mut buf = vec![0u8; size as usize];
        self.file.read_exact(&mut buf).ok()?;
        Some(buf)
    }
}

/// `@font-face{font-family:X;font-weight:W;src:url(assets/....otf)}` -> (family, weight) -> path
fn font_faces(css: &str) -> HashMap<(String, u32), String> {
    let mut out = HashMap::new();
    for block in css.split("@font-face{").skip(1) {
        let block = block.split('}').next().unwrap_or("");
        let field = |k: &str| {
            block.split(';').find_map(|kv| kv.trim().strip_prefix(k).map(|v| v.trim().trim_matches('"').to_string()))
        };
        let (Some(fam), Some(src)) = (field("font-family:"), field("src:")) else { continue };
        let weight = field("font-weight:").and_then(|w| w.parse().ok()).unwrap_or(400);
        if let Some(url) = src.split("url(").nth(1).and_then(|s| s.split(')').next()) {
            out.insert((fam, weight), url.trim_matches('"').to_string());
        }
    }
    out
}

pub struct FontSet {
    pub from_ghub: bool,
    pub defs: FontDefinitions,
}

const FONT_ROLES: [(&str, &str, u32, &str); 5] = [
    ("reg", "BrownPro-Regular", 400, "segoeui.ttf"),
    ("bold", "BrownPro-Bold", 700, "segoeuib.ttf"),
    ("light", "BrownPro-Light", 100, "segoeuisl.ttf"),
    ("pan", "BrownLogitechPan", 400, "segoeui.ttf"),
    ("panb", "BrownLogitechPan", 700, "segoeuib.ttf"),
];

fn windows_fonts_dir() -> PathBuf {
    std::env::var("WINDIR").map(|w| PathBuf::from(w).join("Fonts")).unwrap_or_else(|_| PathBuf::from(r"C:\Windows\Fonts"))
}

fn open_faces() -> (Option<Asar>, HashMap<(String, u32), String>) {
    let mut asar = Asar::open(ASAR);
    let faces = asar
        .as_mut()
        .and_then(|a| a.read("app.min.css"))
        .map(|css| font_faces(&String::from_utf8_lossy(&css)))
        .unwrap_or_default();
    (asar, faces)
}

/// Font bytes for a role: G HUB's Brown Pro from the asar, else the Windows fallback.
/// The bool says whether they came from G HUB.
fn role_bytes(asar: &mut Option<Asar>, faces: &HashMap<(String, u32), String>, win_fonts: &Path, role: (&str, &str, u32, &str)) -> (Option<Vec<u8>>, bool) {
    let (_, fam, weight, fallback) = role;
    match faces.get(&(fam.to_string(), weight)).and_then(|p| asar.as_mut()?.read(p)) {
        Some(b) => (Some(b), true),
        None => (std::fs::read(win_fonts.join(fallback)).or_else(|_| std::fs::read(win_fonts.join("segoeui.ttf"))).ok(), false),
    }
}

/// The bytes `load_fonts` uses for a role key (`reg`, `bold`, ...).
pub fn font_bytes(key: &str) -> Option<Vec<u8>> {
    let role = FONT_ROLES.iter().find(|r| r.0 == key)?;
    let (mut asar, faces) = open_faces();
    role_bytes(&mut asar, &faces, &windows_fonts_dir(), *role).0
}

/// Build egui fonts: Brown Pro from G HUB when present, else Segoe UI.
pub fn load_fonts() -> FontSet {
    let mut defs = FontDefinitions::default();
    let add = |defs: &mut FontDefinitions, key: &str, bytes: Vec<u8>| {
        let mut fd = FontData::from_owned(bytes);
        // CSS px vs egui points: Brown's ascent+descent > 1em; tuned against G HUB captures.
        fd.tweak.scale = 1.0;
        defs.font_data.insert(key.to_string(), Arc::new(fd));
    };
    let mut from_ghub = false;
    let (mut asar, faces) = open_faces();
    let win_fonts = windows_fonts_dir();
    for role in FONT_ROLES {
        let key = role.0;
        let (bytes, ghub) = role_bytes(&mut asar, &faces, &win_fonts, role);
        from_ghub |= ghub;
        let mut chain = Vec::new();
        if let Some(b) = bytes {
            let name = format!("ghubx-{key}");
            add(&mut defs, &name, b);
            chain.push(name);
        }
        chain.extend(defs.families.get(&FontFamily::Proportional).cloned().unwrap_or_default());
        defs.families.insert(FontFamily::Name(key.into()), chain);
    }
    if let Some(reg) = defs.families.get(&FontFamily::Name("reg".into())).cloned() {
        defs.families.insert(FontFamily::Proportional, reg);
    }
    FontSet { from_ghub, defs }
}

// ---------------------------------------------------------------- device art

#[derive(Deserialize, Clone, Debug, Default)]
pub struct Pt {
    pub x: f32,
    pub y: f32,
}

#[derive(Deserialize, Clone, Debug)]
pub struct Hotspot {
    #[serde(rename = "slotId")]
    pub slot_id: String,
    pub marker: Pt,
    pub label: Pt,
}

impl Hotspot {
    /// "g502hero_g7_m1" -> slot 6
    pub fn slot(&self) -> Option<usize> {
        let g = self.slot_id.split('_').find(|p| p.starts_with('g') && p[1..].chars().all(|c| c.is_ascii_digit()) && p.len() > 1)?;
        g[1..].parse::<usize>().ok()?.checked_sub(1)
    }
}

#[derive(Deserialize, Clone, Debug)]
struct ZoneRect {
    x: String,
    y: String,
    width: String,
    height: String,
}

#[derive(Deserialize, Clone, Debug)]
struct Rendering {
    rect: ZoneRect,
}

#[derive(Deserialize, Clone, Debug)]
struct Component {
    renderings: Vec<Rendering>,
}

#[derive(Deserialize, Clone, Debug)]
struct ZoneMeta {
    id: String,
    components: Vec<Component>,
}

#[derive(Deserialize, Clone, Debug)]
struct Origin {
    width: f32,
    height: f32,
}

#[derive(Deserialize, Clone, Debug)]
struct ImageMeta {
    key: String,
    origin: Origin,
    #[serde(default)]
    assignments: Vec<Hotspot>,
    #[serde(default)]
    zones: Vec<ZoneMeta>,
}

#[derive(Deserialize, Clone, Debug)]
struct MetaFile {
    images: Vec<ImageMeta>,
}

#[derive(Clone, Debug)]
pub struct View {
    pub origin: Vec2,
    pub hotspots: Vec<Hotspot>,
    /// (zone index in 0x8070 order, rect in image px): 0 = PRIMARY, 1 = LOGO.
    pub zones: Vec<(usize, egui::Rect)>,
}

#[derive(Clone, Debug)]
pub struct DeviceMeta {
    pub front: View,
    pub side: View,
}

impl Default for DeviceMeta {
    /// Values from G HUB's g502_hero/metadata.json (used when the depot is missing).
    fn default() -> Self {
        let hs = |slot: u8, mx: f32, my: f32, lx: f32, ly: f32| Hotspot {
            slot_id: format!("g502hero_g{slot}_m1"),
            marker: Pt { x: mx, y: my },
            label: Pt { x: lx, y: ly },
        };
        Self {
            front: View {
                origin: Vec2::new(1391.0, 2700.0),
                hotspots: vec![
                    hs(1, 538.0, 614.0, -1400.0, 164.0),
                    hs(2, 1110.0, 614.0, 2840.0, 164.0),
                    hs(3, 800.0, 869.0, 800.0, -300.0),
                    hs(7, 315.0, 989.0, -1400.0, 1240.0),
                    hs(8, 320.0, 760.0, -1400.0, 510.0),
                    hs(9, 835.0, 1387.0, 2840.0, 1387.0),
                    hs(10, 900.0, 869.0, 2840.0, 869.0),
                    hs(11, 700.0, 869.0, -1400.0, 869.0),
                ],
                zones: vec![
                    (1, egui::Rect::from_min_size(egui::pos2(405.0, 1760.0), Vec2::new(300.0, 300.0))),
                    (0, egui::Rect::from_min_size(egui::pos2(210.0, 1339.0), Vec2::new(140.0, 300.0))),
                ],
            },
            side: View {
                origin: Vec2::new(936.0, 2700.0),
                hotspots: vec![
                    hs(4, 580.0, 1800.0, -1200.0, 1800.0),
                    hs(5, 500.0, 1400.0, -1200.0, 1400.0),
                    hs(6, 270.0, 1250.0, -1200.0, 988.0),
                ],
                zones: vec![],
            },
        }
    }
}

fn view_from(m: &ImageMeta) -> View {
    let num = |s: &str| s.parse::<f32>().unwrap_or(0.0);
    View {
        origin: Vec2::new(m.origin.width, m.origin.height),
        hotspots: m.assignments.clone(),
        zones: m
            .zones
            .iter()
            .filter_map(|z| {
                let r = &z.components.first()?.renderings.first()?.rect;
                let idx = match z.id.as_str() {
                    "ZONE_PRIMARY" => 0,
                    "ZONE_BRANDING" | "ZONE_LOGO" => 1,
                    _ => return None,
                };
                Some((idx, egui::Rect::from_min_size(egui::pos2(num(&r.x), num(&r.y)), Vec2::new(num(&r.width), num(&r.height)))))
            })
            .collect(),
    }
}

pub fn find_depot() -> Option<PathBuf> {
    std::fs::read_dir(DEPOTS).ok()?.flatten().map(|e| e.path().join("g502_hero")).find(|p| p.join("metadata.json").exists())
}

pub fn load_meta(depot: Option<&Path>) -> DeviceMeta {
    let parsed = depot
        .and_then(|d| std::fs::read(d.join("metadata.json")).ok())
        .and_then(|b| serde_json::from_slice::<MetaFile>(&b).ok());
    let mut meta = DeviceMeta::default();
    if let Some(m) = parsed {
        for im in &m.images {
            match im.key.as_str() {
                "device_image" => meta.front = view_from(im),
                "device_side" => meta.side = view_from(im),
                _ => {}
            }
        }
    }
    meta
}

/// Decoded, downscaled renders (done off the UI thread).
pub struct Art {
    pub front: Option<ColorImage>,
    pub side: Option<ColorImage>,
}

pub fn decode_art(depot: Option<PathBuf>) -> Art {
    let load = |name: &str| -> Option<ColorImage> {
        let path = depot.as_ref()?.join("images").join("device").join(name);
        let img = image::open(path).ok()?.to_rgba8();
        let h = 1100u32.min(img.height());
        let w = (img.width() as f32 * h as f32 / img.height() as f32).round() as u32;
        let small = image::imageops::resize(&img, w, h, image::imageops::FilterType::Triangle);
        Some(ColorImage::from_rgba_unmultiplied([w as usize, h as usize], small.as_raw()))
    };
    Art { front: load("front_hero.png"), side: load("side_hero.png") }
}

pub const TEX_OPTS: TextureOptions = TextureOptions {
    magnification: egui::TextureFilter::Linear,
    minification: egui::TextureFilter::Linear,
    wrap_mode: egui::TextureWrapMode::ClampToEdge,
    mipmap_mode: Some(egui::TextureFilter::Linear),
};

/// Soft radial glow used to preview LED zones on the render.
pub fn glow_image() -> ColorImage {
    let n = 128usize;
    let mut px = Vec::with_capacity(n * n);
    for y in 0..n {
        for x in 0..n {
            let dx = (x as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let dy = (y as f32 + 0.5) / n as f32 * 2.0 - 1.0;
            let d = (dx * dx + dy * dy).sqrt();
            let a = (1.0 - d).clamp(0.0, 1.0).powf(1.8);
            px.push(Color32::from_white_alpha((a * 255.0) as u8));
        }
    }
    ColorImage::new([n, n], px)
}

pub struct Textures {
    pub front: Option<TextureHandle>,
    pub side: Option<TextureHandle>,
    pub glow: TextureHandle,
}
