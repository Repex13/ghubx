//! Data model mirroring G HUB: applications own profiles and macros; a
//! profile holds the G502 settings (assignments per layer, DPI table,
//! report rate, per-zone lighting). Also the onboard sector layout + CRC.

use crate::device::{LED_BREATHING, LED_CYCLE, LED_FIXED, LED_OFF, SLOTS};
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- input model

/// Modifier bits for [`KeyCombo::mods`].
pub const MOD_CTRL: u8 = 1;
pub const MOD_SHIFT: u8 = 2;
pub const MOD_ALT: u8 = 4;
pub const MOD_WIN: u8 = 8;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct KeyCombo {
    pub vk: u16,
    pub mods: u8,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum MouseFn {
    Primary,
    Secondary,
    Middle,
    Back,
    Forward,
    DoubleClick,
    WheelUp,
    WheelDown,
    WheelLeft,
    WheelRight,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum SystemFn {
    DpiUp,
    DpiDown,
    DpiCycle,
    DpiShift,
    DpiDefault,
    GShift,
    ProfileCycle,
    OnboardProfileCycle,
    ReportRateCycle,
}

/// What a button slot does. `Default` is the slot's native function.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub enum Assignment {
    #[default]
    Default,
    Disabled,
    Mouse(MouseFn),
    Key(KeyCombo),
    /// Named keystroke shortcut from the COMMANDS catalog.
    Shortcut { name: String, combo: KeyCombo },
    System(SystemFn),
    /// Macro by id within the owning application.
    Macro(String),
    Launch { name: String, path: String },
    /// Basilisk V4 "Perfect 180" (calibrated in [`Extras`]).
    Perfect180 { left: bool },
    ToggleScrollAccel,
}

/// The G502 HERO's factory function for each slot (G1..G11).
pub fn native(slot: usize) -> Assignment {
    use Assignment::{Mouse, System};
    match slot {
        0 => Mouse(MouseFn::Primary),
        1 => Mouse(MouseFn::Secondary),
        2 => Mouse(MouseFn::Middle),
        3 => Mouse(MouseFn::Back),
        4 => Mouse(MouseFn::Forward),
        5 => System(SystemFn::DpiShift),
        6 => System(SystemFn::DpiDown),
        7 => System(SystemFn::DpiUp),
        8 => System(SystemFn::OnboardProfileCycle),
        9 => Mouse(MouseFn::WheelRight),
        10 => Mouse(MouseFn::WheelLeft),
        _ => Assignment::Disabled,
    }
}

/// HID button number a slot sends natively when it keeps its factory
/// function and that function is an ordinary click (0x8110 map value).
pub fn native_hid_button(slot: usize) -> u8 {
    if slot < 5 { slot as u8 + 1 } else { 0 }
}

// ---------------------------------------------------------------- macros

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
pub enum MacroKind {
    #[default]
    NoRepeat,
    RepeatWhileHolding,
    Toggle,
}

/// One macro event. `vk` 0x01/0x02/0x04/0x05/0x06 are mouse buttons
/// (VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct MacroStep {
    pub vk: u16,
    pub down: bool,
    pub delay_ms: u32,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: MacroKind,
    #[serde(default)]
    pub steps: Vec<MacroStep>,
}

// ---------------------------------------------------------------- sensitivity

/// G HUB's MOUSE_SETTINGS card: levels plus active/default/shift values.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct DpiTable {
    pub levels: Vec<u16>,
    pub active: u16,
    pub default: u16,
    pub shift: u16,
}

impl Default for DpiTable {
    fn default() -> Self {
        Self { levels: vec![400, 800, 1600, 3200, 6400], active: 1600, default: 1600, shift: 400 }
    }
}

impl DpiTable {
    pub const MAX_LEVELS: usize = 5;

    pub fn active_index(&self) -> usize {
        self.levels.iter().position(|&d| d == self.active).unwrap_or(0)
    }

    /// Step the active level up/down (clamped), returning the new dpi.
    pub fn step(&mut self, up: bool) -> u16 {
        let i = self.active_index();
        let j = if up { (i + 1).min(self.levels.len().saturating_sub(1)) } else { i.saturating_sub(1) };
        self.active = self.levels.get(j).copied().unwrap_or(self.active);
        self.active
    }

    pub fn cycle(&mut self) -> u16 {
        let i = (self.active_index() + 1) % self.levels.len().max(1);
        self.active = self.levels.get(i).copied().unwrap_or(self.active);
        self.active
    }

    pub fn insert(&mut self, dpi: u16) -> bool {
        if self.levels.len() >= Self::MAX_LEVELS || self.levels.contains(&dpi) {
            return false;
        }
        self.levels.push(dpi);
        self.levels.sort_unstable();
        true
    }

    /// Remove a level, keeping active/default/shift pointing at real levels.
    pub fn remove(&mut self, dpi: u16) {
        if self.levels.len() <= 1 {
            return;
        }
        self.levels.retain(|&d| d != dpi);
        let first = self.levels[0];
        for v in [&mut self.active, &mut self.default] {
            if !self.levels.contains(v) {
                *v = first;
            }
        }
    }

    /// Move a level to a new value, carrying the active/default/shift marks.
    pub fn retarget(&mut self, from: u16, to: u16) {
        if from == to || self.levels.contains(&to) {
            return;
        }
        for d in &mut self.levels {
            if *d == from {
                *d = to;
            }
        }
        self.levels.sort_unstable();
        for v in [&mut self.active, &mut self.default, &mut self.shift] {
            if *v == from {
                *v = to;
            }
        }
    }
}

// ---------------------------------------------------------------- lighting

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Effect {
    Off,
    Fixed,
    Breathing,
    Cycle,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lighting {
    pub effect: Effect,
    pub color: [u8; 3],
    pub period_ms: u16,
    /// 1..=100 %
    pub brightness: u8,
}

impl Default for Lighting {
    fn default() -> Self {
        Self { effect: Effect::Cycle, color: [0x00, 0xB8, 0xFC], period_ms: 8000, brightness: 100 }
    }
}

impl Lighting {
    /// 11-byte `internal_led` payload for 0x8070 setZoneEffect (libratbag layout).
    pub fn to_led(&self) -> [u8; 11] {
        let mut l = [0u8; 11];
        let intensity = if self.brightness >= 100 { 0 } else { self.brightness.max(1) };
        match self.effect {
            Effect::Off => l[0] = LED_OFF,
            Effect::Fixed => {
                l[0] = LED_FIXED;
                let s = self.brightness.clamp(1, 100) as u16;
                for i in 0..3 {
                    l[1 + i] = (self.color[i] as u16 * s / 100) as u8;
                }
            }
            Effect::Breathing => {
                l[0] = LED_BREATHING;
                l[1..4].copy_from_slice(&self.color);
                l[4..6].copy_from_slice(&self.period_ms.to_be_bytes());
                l[7] = intensity;
            }
            Effect::Cycle => {
                l[0] = LED_CYCLE;
                l[6..8].copy_from_slice(&self.period_ms.to_be_bytes());
                l[8] = intensity;
            }
        }
        l
    }

    pub fn from_led(l: &[u8; 11]) -> Option<Self> {
        let pct = |i: u8| if i == 0 || i > 100 { 100 } else { i };
        Some(match l[0] {
            LED_OFF => Self { effect: Effect::Off, ..Default::default() },
            LED_FIXED => Self { effect: Effect::Fixed, color: [l[1], l[2], l[3]], ..Default::default() },
            LED_BREATHING => Self {
                effect: Effect::Breathing,
                color: [l[1], l[2], l[3]],
                period_ms: u16::from_be_bytes([l[4], l[5]]),
                brightness: pct(l[7]),
            },
            LED_CYCLE => Self {
                effect: Effect::Cycle,
                period_ms: u16::from_be_bytes([l[6], l[7]]),
                brightness: pct(l[8]),
                ..Default::default()
            },
            _ => return None,
        })
    }
}

/// LED zones in 0x8070 order: zone 0 = PRIMARY (DPI indicator), zone 1 = LOGO.
pub const ZONE_NAMES: [&str; 2] = ["PRIMARY", "LOGO"];

// ---------------------------------------------------------------- profiles

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Profile {
    pub name: String,
    #[serde(default)]
    pub dpi: DpiTable,
    #[serde(default = "default_rate_ms")]
    pub rate_ms: u8,
    #[serde(default = "default_slots")]
    pub base: Vec<Assignment>,
    #[serde(default = "default_slots")]
    pub gshift: Vec<Assignment>,
    #[serde(default)]
    pub lighting: [Lighting; 2],
}

fn default_rate_ms() -> u8 {
    1
}

fn default_slots() -> Vec<Assignment> {
    vec![Assignment::Default; SLOTS]
}

impl Profile {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.into(),
            dpi: DpiTable::default(),
            rate_ms: 1,
            base: default_slots(),
            gshift: default_slots(),
            lighting: Default::default(),
        }
    }

    pub fn layer(&self, gshift: bool) -> &Vec<Assignment> {
        if gshift { &self.gshift } else { &self.base }
    }

    pub fn layer_mut(&mut self, gshift: bool) -> &mut Vec<Assignment> {
        let l = if gshift { &mut self.gshift } else { &mut self.base };
        l.resize(SLOTS, Assignment::Default);
        l
    }

    /// Resolved function for a slot in a layer (`Default` -> native).
    pub fn effective(&self, slot: usize, gshift: bool) -> Assignment {
        match self.layer(gshift).get(slot) {
            None | Some(Assignment::Default) => native(slot),
            Some(a) => a.clone(),
        }
    }

    pub fn uses_gshift(&self) -> bool {
        (0..SLOTS).any(|s| self.effective(s, false) == Assignment::System(SystemFn::GShift))
    }

    /// 0x8110 map for host mode: a slot keeps its native HID button only if
    /// it performs that same click in every layer it can be pressed in.
    pub fn spy_map(&self) -> [u8; 16] {
        let mut map = [0u8; 16];
        let gs = self.uses_gshift();
        for (slot, m) in map.iter_mut().enumerate().take(SLOTS) {
            let hid = native_hid_button(slot);
            let keeps = |layer| self.effective(slot, layer) == native(slot);
            if hid != 0 && keeps(false) && (!gs || keeps(true)) {
                *m = hid;
            }
        }
        map
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AppEntry {
    pub name: String,
    /// Executable file name for automatic switching; `None` for Desktop.
    #[serde(default)]
    pub exe: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub active_profile: usize,
    #[serde(default)]
    pub macros: Vec<Macro>,
}

fn yes() -> bool {
    true
}

impl AppEntry {
    pub fn desktop() -> Self {
        Self {
            name: "Desktop".into(),
            exe: None,
            enabled: true,
            profiles: vec![Profile::new("Default")],
            active_profile: 0,
            macros: vec![],
        }
    }

    pub fn profile(&self) -> &Profile {
        &self.profiles[self.active_profile.min(self.profiles.len() - 1)]
    }
}

// ---------------------------------------------------------------- settings

/// Perfect 180 calculator presets: (label, degrees per count at sensitivity 1.0).
/// Custom = enter the count directly.
pub const P180_GAMES: [(&str, f32); 4] = [("CS2/APEX", 0.022), ("VALORANT", 0.07), ("OW2/COD", 0.0066), ("CUSTOM", 0.0)];

/// Mouse counts for a 180° turn: 180 / (yaw × in-game sensitivity).
pub fn p180_counts_for(yaw: f32, sens: f32) -> Option<i32> {
    (yaw > 0.0 && sens > 0.0).then(|| (180.0 / (yaw * sens)).round().clamp(1.0, 1_000_000.0) as i32)
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Extras {
    /// Mouse counts injected for a 180° turn ("total count" in Synapse terms).
    pub p180_counts: i32,
    pub p180_ms: u16,
    pub p180_steps: u8,
    /// Index into [`P180_GAMES`] used by the calculator.
    pub p180_game: usize,
    /// In-game sensitivity used by the calculator.
    pub p180_sens: f32,
    pub scroll_accel: bool,
    /// Extra wheel ticks per tick at full speed.
    pub scroll_boost: f32,
}

impl Default for Extras {
    fn default() -> Self {
        Self { p180_counts: 8182, p180_ms: 60, p180_steps: 12, p180_game: 0, p180_sens: 1.0, scroll_accel: false, scroll_boost: 3.0 }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct General {
    pub launch_on_login: bool,
    /// Take host control of the mouse whenever G HUB isn't running.
    pub take_control: bool,
    /// Turn lighting off after this many idle minutes (0 = never).
    pub lighting_sleep_min: u32,
    /// Closing the window keeps GHUB-X running in the tray.
    pub close_to_tray: bool,
    /// Autostart with `--minimized` so it starts in the tray.
    pub start_minimized: bool,
}

impl Default for General {
    fn default() -> Self {
        Self { launch_on_login: false, take_control: true, lighting_sleep_min: 0, close_to_tray: true, start_minimized: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Preset id (see `ui::palette::THEMES`).
    pub theme: String,
    /// Custom accent colour; `None` keeps the preset's own.
    pub accent: Option<[u8; 3]>,
    /// Use the logo zone's lighting colour as the accent.
    pub follow_lighting: bool,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: "ghub-dark".into(), accent: None, follow_lighting: false }
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Config {
    pub apps: Vec<AppEntry>,
    /// Application whose profile the selector shows (0 = Desktop).
    #[serde(default)]
    pub current_app: usize,
    #[serde(default)]
    pub profile_switching: bool,
    /// (app, profile) forced for every application.
    #[serde(default)]
    pub persistent: Option<(usize, usize)>,
    #[serde(default)]
    pub general: General,
    #[serde(default)]
    pub extras: Extras,
    #[serde(default)]
    pub appearance: Appearance,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            apps: vec![AppEntry::desktop()],
            current_app: 0,
            profile_switching: false,
            persistent: None,
            general: General::default(),
            extras: Extras::default(),
            appearance: Appearance::default(),
        }
    }
}

impl Config {
    /// Repair indices after edits/deserialization.
    pub fn sanitize(&mut self) {
        if self.apps.is_empty() {
            self.apps.push(AppEntry::desktop());
        }
        for a in &mut self.apps {
            if a.profiles.is_empty() {
                a.profiles.push(Profile::new("Default"));
            }
            a.active_profile = a.active_profile.min(a.profiles.len() - 1);
            for p in &mut a.profiles {
                p.base.resize(SLOTS, Assignment::Default);
                p.gshift.resize(SLOTS, Assignment::Default);
                if p.dpi.levels.is_empty() {
                    p.dpi = DpiTable::default();
                }
            }
        }
        self.current_app = self.current_app.min(self.apps.len() - 1);
        if let Some((a, p)) = self.persistent
            && (a >= self.apps.len() || p >= self.apps[a].profiles.len())
        {
            self.persistent = None;
        }
    }

    pub fn current(&self) -> (&AppEntry, &Profile) {
        let a = &self.apps[self.current_app];
        (a, a.profile())
    }

    pub fn current_profile_mut(&mut self) -> &mut Profile {
        let a = &mut self.apps[self.current_app];
        let i = a.active_profile.min(a.profiles.len() - 1);
        &mut a.profiles[i]
    }

    /// The (app, profile) that should drive the mouse given the foreground exe.
    pub fn resolve(&self, foreground_exe: Option<&str>) -> (usize, usize) {
        if let Some(p) = self.persistent {
            return p;
        }
        if self.profile_switching
            && let Some(exe) = foreground_exe
            && let Some(i) = self.apps.iter().position(|a| {
                a.enabled && a.exe.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(exe))
            })
        {
            return (i, self.apps[i].active_profile);
        }
        (self.current_app, self.apps[self.current_app].active_profile)
    }

    pub fn profile_at(&self, (a, p): (usize, usize)) -> Option<&Profile> {
        self.apps.get(a)?.profiles.get(p)
    }

    pub fn macro_by_id(&self, app: usize, id: &str) -> Option<&Macro> {
        self.apps.get(app)?.macros.iter().find(|m| m.id == id)
    }

    pub fn path() -> std::path::PathBuf {
        let base = std::env::var("GHUBX_CONFIG_DIR").or_else(|_| std::env::var("APPDATA")).unwrap_or_else(|_| ".".into());
        std::path::Path::new(&base).join("ghubx").join("config.toml")
    }

    pub fn load() -> Self {
        Self::load_existing().unwrap_or_default()
    }

    /// The saved config, or `None` if there is none (or it's from an older,
    /// incompatible version).
    pub fn load_existing() -> Option<Self> {
        let mut c: Config = toml::from_str(&std::fs::read_to_string(Self::path()).ok()?).ok()?;
        c.sanitize();
        Some(c)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let p = Self::path();
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        let s = toml::to_string_pretty(self).map_err(std::io::Error::other)?;
        let tmp = p.with_extension("toml.tmp");
        std::fs::write(&tmp, s)?;
        std::fs::rename(tmp, p)
    }
}

/// Short unique id for new macros.
pub fn new_id() -> String {
    use std::time::{SystemTime, UNIX_EPOCH};
    let t = SystemTime::now().duration_since(UNIX_EPOCH).map(|d| d.as_nanos()).unwrap_or(0);
    format!("m{:x}", t)
}

// ---------------------------------------------------------------- onboard sector

pub const SECTOR_SIZE: usize = 256;

/// crc16-ccitt (poly 0x1021, init 0xFFFF), stored big-endian in the last 2 bytes.
pub fn crc16(data: &[u8]) -> u16 {
    let mut crc: u16 = 0xFFFF;
    for &b in data {
        crc ^= (b as u16) << 8;
        for _ in 0..8 {
            crc = if crc & 0x8000 != 0 { (crc << 1) ^ 0x1021 } else { crc << 1 };
        }
    }
    crc
}

pub fn sector_crc_ok(s: &[u8; SECTOR_SIZE]) -> bool {
    crc16(&s[..SECTOR_SIZE - 2]) == u16::from_be_bytes([s[SECTOR_SIZE - 2], s[SECTOR_SIZE - 1]])
}

/// Summary of a G303-format onboard profile sector.
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardProfile {
    pub rate_ms: u8,
    pub default_dpi: usize,
    pub dpi: Vec<u16>,
    pub name: String,
    pub leds: [[u8; 11]; 2],
}

impl OnboardProfile {
    pub fn parse(s: &[u8; SECTOR_SIZE]) -> Option<Self> {
        if s[..4] == [0xFF; 4] || !sector_crc_ok(s) {
            return None;
        }
        let dpi = (0..5)
            .map(|i| u16::from_le_bytes([s[3 + i * 2], s[4 + i * 2]]))
            .filter(|&d| d != 0 && d != 0xFFFF)
            .collect();
        let raw_name = &s[0xA0..0xA0 + 48];
        let end = raw_name.iter().position(|&c| c == 0 || c == 0xFF).unwrap_or(48);
        let mut leds = [[0u8; 11]; 2];
        leds[0].copy_from_slice(&s[0xD0..0xDB]);
        leds[1].copy_from_slice(&s[0xDB..0xE6]);
        Some(Self {
            rate_ms: s[0],
            default_dpi: s[1] as usize,
            dpi,
            name: String::from_utf8_lossy(&raw_name[..end]).into_owned(),
            leds,
        })
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn config_without_appearance_loads_defaults() {
        let c: Config = toml::from_str("apps = []").unwrap();
        assert_eq!(c.appearance.theme, "ghub-dark");
        assert_eq!(c.appearance.accent, None);
        assert!(!c.appearance.follow_lighting);
    }

    use super::*;

    /// Sector 1 as read from the user's G502 HERO (probe round 8).
    fn real_sector1() -> [u8; 256] {
        let mut s = [0xFFu8; 256];
        let rows: [(usize, &[u8]); 7] = [
            (0x00, &[0x01, 0x01, 0x00, 0xB0, 0x04, 0x60, 0x09, 0x80, 0x0C, 0x00, 0x19, 0x00, 0x00, 0xFF, 0xFF, 0xFF]),
            (0x10, &[0xFF, 0x00]),
            (0x20, &[0x80, 0x01, 0x00, 0x01, 0x80, 0x01, 0x00, 0x02, 0x80, 0x01, 0x00, 0x04, 0x80, 0x01, 0x00, 0x08]),
            (0x30, &[0x80, 0x01, 0x00, 0x10, 0x90, 0x07, 0xFF, 0xFF, 0x90, 0x04, 0xFF, 0xFF, 0x90, 0x03, 0xFF, 0xFF]),
            (0x40, &[0x90, 0x0A, 0xFF, 0xFF, 0x90, 0x02, 0xFF, 0xFF, 0x90, 0x01, 0xFF, 0xFF]),
            (0xD0, &[0x03, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1F, 0x40, 0x00, 0x00, 0x00, 0x03, 0x00, 0x00, 0x00, 0x00]),
            (0xE0, &[0x00, 0x1F, 0x40, 0x00, 0x00, 0x00]),
        ];
        for (off, b) in rows {
            s[off..off + b.len()].copy_from_slice(b);
        }
        s[0xFE] = 0x32;
        s[0xFF] = 0x9A;
        s
    }

    #[test]
    fn crc_matches_device_sector() {
        let s = real_sector1();
        assert!(sector_crc_ok(&s));
        let p = OnboardProfile::parse(&s).unwrap();
        assert_eq!(p.rate_ms, 1);
        assert_eq!(p.dpi, vec![1200, 2400, 3200, 6400]);
        let led = Lighting::from_led(&p.leds[0]).unwrap();
        assert_eq!((led.effect, led.period_ms, led.brightness), (Effect::Cycle, 8000, 100));
        assert_eq!(led.to_led(), p.leds[0]);
    }

    #[test]
    fn spy_map_mutes_only_remapped_clicks() {
        let mut p = Profile::new("t");
        assert_eq!(&p.spy_map()[..11], &[1, 2, 3, 4, 5, 0, 0, 0, 0, 0, 0]);
        p.base[3] = Assignment::Macro("x".into());
        p.base[5] = Assignment::System(SystemFn::GShift);
        p.gshift[0] = Assignment::Key(KeyCombo { vk: 0x41, mods: 0 });
        assert_eq!(&p.spy_map()[..11], &[0, 2, 3, 0, 5, 0, 0, 0, 0, 0, 0]);
    }

    #[test]
    fn perfect_180_counts_from_sensitivity() {
        assert_eq!(p180_counts_for(0.022, 1.0), Some(8182)); // CS2 @ 1.0
        assert_eq!(p180_counts_for(0.07, 0.3), Some(8571)); // Valorant @ 0.3
        assert_eq!(p180_counts_for(0.0066, 4.5), Some(6061)); // OW2 @ 4.5
        assert_eq!(p180_counts_for(0.0, 1.0), None);
        // older configs without the calculator fields still load
        let e: Extras = toml::from_str("p180_counts = 1800\np180_ms = 60\np180_steps = 12\nscroll_accel = false\nscroll_boost = 3.0").unwrap();
        assert_eq!((e.p180_counts, e.p180_game, e.p180_sens), (1800, 0, 1.0));
    }

    #[test]
    fn dpi_table_edits_keep_marks_valid() {
        let mut t = DpiTable { levels: vec![600, 1000, 1600], active: 600, default: 1600, shift: 600 };
        assert_eq!(t.step(true), 1000);
        assert_eq!(t.cycle(), 1600);
        assert_eq!(t.cycle(), 600);
        t.retarget(600, 800);
        assert_eq!((t.levels[0], t.active, t.shift), (800, 800, 800));
        t.remove(1600);
        assert_eq!(t.default, 800);
        assert!(t.insert(3200) && !t.insert(3200));
    }
}
