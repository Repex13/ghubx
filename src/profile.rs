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
    /// OBS Studio action (sent over its WebSocket server).
    Obs(ObsAction),
}

/// OBS Studio actions, named as in G HUB's OBS integration.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ObsAction {
    StartStream,
    StopStream,
    ToggleStream,
    StartRecord,
    StopRecord,
    ToggleRecord,
    StartBuffer,
    StopBuffer,
    ToggleBuffer,
    SaveBuffer,
    MuteDesktop,
    UnmuteDesktop,
    ToggleDesktopMute,
    MuteMic,
    UnmuteMic,
    ToggleMicMute,
    /// Activate Scene.
    Scene(String),
    /// Activate Collection.
    Collection(String),
    /// Activate Source, in the current scene.
    SourceOn(String),
    /// Deactivate A Source.
    SourceOff(String),
    SourceToggle(String),
    /// Mute Mixer (an audio input by name).
    MixerMute(String),
    MixerUnmute(String),
    MixerToggle(String),
}

impl ObsAction {
    /// The 16 actions without a target, in G HUB's order.
    pub const SIMPLE: [ObsAction; 16] = [
        ObsAction::StartStream,
        ObsAction::StopStream,
        ObsAction::ToggleStream,
        ObsAction::StartRecord,
        ObsAction::StopRecord,
        ObsAction::ToggleRecord,
        ObsAction::StartBuffer,
        ObsAction::StopBuffer,
        ObsAction::ToggleBuffer,
        ObsAction::SaveBuffer,
        ObsAction::MuteDesktop,
        ObsAction::UnmuteDesktop,
        ObsAction::ToggleDesktopMute,
        ObsAction::MuteMic,
        ObsAction::UnmuteMic,
        ObsAction::ToggleMicMute,
    ];
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
    /// ON PRESS (`steps`), WHILE HOLDING (`hold`), ON RELEASE (`release`).
    Sequence,
}

/// What a step does when it is not a key event.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum StepAct {
    /// Type this text.
    Text(String),
    /// Run an assignment once (press and release): SYSTEM commands and LAUNCH APPLICATION.
    Do(Assignment),
}

/// One macro event. `vk` 0x01/0x02/0x04/0x05/0x06 are mouse buttons
/// (VK_LBUTTON, VK_RBUTTON, VK_MBUTTON, VK_XBUTTON1, VK_XBUTTON2).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize, Default)]
pub struct MacroStep {
    pub vk: u16,
    pub down: bool,
    /// Wait this long before the step.
    pub delay_ms: u32,
    /// When set: `vk == 0` and `down == false`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub act: Option<StepAct>,
}

impl MacroStep {
    pub fn key(vk: u16, down: bool, delay_ms: u32) -> Self {
        Self { vk, down, delay_ms, act: None }
    }

    pub fn pause(delay_ms: u32) -> Self {
        Self::key(0, false, delay_ms)
    }

    pub fn text(text: impl Into<String>, delay_ms: u32) -> Self {
        Self { vk: 0, down: false, delay_ms, act: Some(StepAct::Text(text.into())) }
    }

    pub fn run(a: Assignment, delay_ms: u32) -> Self {
        Self { vk: 0, down: false, delay_ms, act: Some(StepAct::Do(a)) }
    }

    pub fn is_pause(&self) -> bool {
        self.vk == 0 && self.act.is_none()
    }
}

/// Assignments a macro step may run: everything except Default, Disabled, Macro(_),
/// System(DpiShift) and System(GShift) (those need a held button).
pub fn step_allowed(a: &Assignment) -> bool {
    !matches!(a, Assignment::Default | Assignment::Disabled | Assignment::Macro(_) | Assignment::System(SystemFn::DpiShift | SystemFn::GShift))
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Macro {
    pub id: String,
    pub name: String,
    #[serde(default)]
    pub kind: MacroKind,
    /// The macro (NoRepeat / Repeat / Toggle) or ON PRESS (Sequence).
    #[serde(default)]
    pub steps: Vec<MacroStep>,
    /// Sequence: WHILE HOLDING.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub hold: Vec<MacroStep>,
    /// Sequence: ON RELEASE.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub release: Vec<MacroStep>,
    /// USE STANDARD DELAYS: `Some(ms)` = every gap is this long, `None` = each step keeps its own delay.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub std_delay: Option<u32>,
    /// SHOW KEY DOWN/KEY UP in the editor.
    #[serde(default = "yes")]
    pub show_up_down: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub color: Option<[u8; 3]>,
}

impl Default for Macro {
    fn default() -> Self {
        Self { id: String::new(), name: String::new(), kind: MacroKind::NoRepeat, steps: vec![], hold: vec![], release: vec![], std_delay: None, show_up_down: true, color: None }
    }
}

impl Macro {
    /// The standard delay in force: `std_delay`, or for macros saved before that field existed
    /// the delay every step already uses.
    pub fn standard_delay(&self) -> Option<u32> {
        if self.std_delay.is_some() {
            return self.std_delay;
        }
        let mut found: Option<u32> = None;
        for list in [&self.steps, &self.hold, &self.release] {
            for (i, s) in list.iter().enumerate() {
                if i == 0 && !s.is_pause() && s.delay_ms == 0 {
                    continue;
                }
                if s.is_pause() && i + 1 != list.len() {
                    return None;
                }
                if *found.get_or_insert(s.delay_ms) != s.delay_ms {
                    return None;
                }
            }
        }
        found.filter(|&d| d > 0)
    }

    /// Rewrite all lists for USE STANDARD DELAYS = `ms` and set `std_delay`.
    pub fn apply_standard_delay(&mut self, ms: u32) {
        let looping = match self.kind {
            MacroKind::RepeatWhileHolding | MacroKind::Toggle => 1,
            MacroKind::Sequence => 2,
            MacroKind::NoRepeat => 0,
        };
        for (n, list) in [&mut self.steps, &mut self.hold, &mut self.release].into_iter().enumerate() {
            list.retain(|s| !s.is_pause());
            for (i, s) in list.iter_mut().enumerate() {
                s.delay_ms = if i == 0 { 0 } else { ms };
            }
            if n + 1 == looping && !list.is_empty() {
                list.push(MacroStep::pause(ms));
            }
        }
        self.std_delay = Some(ms);
    }
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
    /// Host-driven: the PC samples the screen and writes the colour.
    Screen,
    /// Host-driven: the PC follows the audio it plays and writes the colour.
    Audio,
}

impl Effect {
    pub fn is_host(self) -> bool {
        matches!(self, Effect::Screen | Effect::Audio)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
pub enum ScreenArea {
    #[default]
    Full,
    TopLeft,
    Top,
    TopRight,
    Left,
    Center,
    Right,
    BottomLeft,
    Bottom,
    BottomRight,
}

impl ScreenArea {
    pub const GRID: [ScreenArea; 9] = [
        ScreenArea::TopLeft,
        ScreenArea::Top,
        ScreenArea::TopRight,
        ScreenArea::Left,
        ScreenArea::Center,
        ScreenArea::Right,
        ScreenArea::BottomLeft,
        ScreenArea::Bottom,
        ScreenArea::BottomRight,
    ];

    /// (x0, y0, x1, y1) as fractions of the primary screen.
    pub fn rect(self) -> (f32, f32, f32, f32) {
        let Some(i) = Self::GRID.iter().position(|a| *a == self) else {
            return (0.0, 0.0, 1.0, 1.0);
        };
        let (c, r) = ((i % 3) as f32, (i / 3) as f32);
        (c / 3.0, r / 3.0, (c + 1.0) / 3.0, (r + 1.0) / 3.0)
    }

    pub fn label(self) -> &'static str {
        match self {
            ScreenArea::Full => "FULL SCREEN",
            ScreenArea::TopLeft => "TOP LEFT",
            ScreenArea::Top => "TOP CENTER",
            ScreenArea::TopRight => "TOP RIGHT",
            ScreenArea::Left => "MID LEFT",
            ScreenArea::Center => "CENTER",
            ScreenArea::Right => "MID RIGHT",
            ScreenArea::BottomLeft => "BOTTOM LEFT",
            ScreenArea::Bottom => "BOTTOM CENTER",
            ScreenArea::BottomRight => "BOTTOM RIGHT",
        }
    }
}

/// Settings of the two host-driven effects (SCREEN SAMPLER, AUDIO VISUALIZER).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostFx {
    pub area: ScreenArea,
    /// Colour boost 0..=100.
    pub boost: u8,
    /// 0..=100.
    pub smoothing: u8,
    /// Audio visualizer background colour (shown at silence).
    pub color2: [u8; 3],
    pub intensity: u8,
    pub auto_sens: bool,
    /// Manual sensitivity 1..=100.
    pub sensitivity: u8,
    pub bass_only: bool,
}

impl Default for HostFx {
    fn default() -> Self {
        Self { area: ScreenArea::Full, boost: 50, smoothing: 40, color2: [0, 0, 0], intensity: 50, auto_sens: true, sensitivity: 50, bass_only: false }
    }
}

impl HostFx {
    pub fn is_default(&self) -> bool {
        *self == Self::default()
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct Lighting {
    pub effect: Effect,
    pub color: [u8; 3],
    pub period_ms: u16,
    /// 1..=100 %
    pub brightness: u8,
    #[serde(default, skip_serializing_if = "HostFx::is_default")]
    pub host: HostFx,
}

impl Default for Lighting {
    fn default() -> Self {
        Self { effect: Effect::Cycle, color: [0x00, 0xB8, 0xFC], period_ms: 8000, brightness: 100, host: HostFx::default() }
    }
}

impl Lighting {
    /// 11-byte `internal_led` payload for 0x8070 setZoneEffect (libratbag layout).
    pub fn to_led(&self) -> [u8; 11] {
        let mut l = [0u8; 11];
        let intensity = if self.brightness >= 100 { 0 } else { self.brightness.max(1) };
        match self.effect {
            Effect::Off => l[0] = LED_OFF,
            Effect::Screen | Effect::Audio => l[0] = LED_FIXED,
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
                ..Default::default()
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
    /// Perfect 180 calibration for this profile only (falls back to `Extras`).
    #[serde(default)]
    pub turn: Option<TurnCal>,
}

/// Perfect 180 calibration stored with a profile.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct TurnCal {
    pub counts: i32,
    pub game: usize,
    pub sens: f32,
    pub angle: u16,
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
            turn: None,
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

    /// Resolved function for a slot in a layer (`Default` -> native; a `Default`
    /// G-Shift slot keeps the base layer's assignment, as G HUB does).
    pub fn effective(&self, slot: usize, gshift: bool) -> Assignment {
        match self.layer(gshift).get(slot) {
            None | Some(Assignment::Default) if gshift => self.effective(slot, false),
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
    /// Install folder: the app is active when a process from inside it is in front.
    #[serde(default)]
    pub dir: Option<String>,
    /// G HUB game database id (for the game's commands).
    #[serde(default)]
    pub ghub_id: Option<String>,
    #[serde(default = "yes")]
    pub enabled: bool,
    pub profiles: Vec<Profile>,
    #[serde(default)]
    pub active_profile: usize,
    #[serde(default)]
    pub macros: Vec<Macro>,
}

fn path_key(p: &str) -> String {
    p.replace('/', "\\").to_lowercase()
}

fn yes() -> bool {
    true
}

impl AppEntry {
    pub fn desktop() -> Self {
        Self {
            name: "Desktop".into(),
            exe: None,
            dir: None,
            ghub_id: None,
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

/// A game or app with its profiles and macros as a shareable `.ghubx` file (TOML).
pub fn export_app(app: &AppEntry) -> String {
    #[derive(Serialize)]
    struct File<'a> {
        format: u32,
        app: &'a AppEntry,
    }
    toml::to_string_pretty(&File { format: 1, app }).unwrap_or_default()
}

pub fn import_app(text: &str) -> Result<AppEntry, String> {
    #[derive(Deserialize)]
    struct File {
        format: Option<u32>,
        app: Option<AppEntry>,
    }
    let f: File = toml::from_str(text).map_err(|e| format!("Not a GHUB-X profile file: {e}"))?;
    if f.format != Some(1) {
        return Err("Unsupported profile file format".into());
    }
    let mut app = f.app.ok_or("The file has no app")?;
    if app.profiles.is_empty() {
        return Err("The file has no profiles".into());
    }
    Config::sanitize_app(&mut app);
    Ok(app)
}

// ---------------------------------------------------------------- settings

/// Perfect 180 calculator presets: (label, degrees per count at sensitivity 1.0).
/// Custom = enter the count directly.
pub const P180_GAMES: [(&str, f32); 6] = [
    ("CS2/APEX", 0.022),
    ("VALORANT", 0.07),
    ("OW2/COD", 0.0066),
    ("CUSTOM", 0.0),
    ("R6 SIEGE", 0.005_729_578),
    ("FORTNITE", 0.005555),
];
/// Index of the "enter the count directly" preset.
pub const P180_CUSTOM: usize = 3;
/// Preset pills are shown in this order (custom last).
pub const P180_ORDER: [usize; 6] = [0, 1, 2, 4, 5, P180_CUSTOM];

/// Counts for a turn of `angle` degrees given the counts for 180°.
pub fn turn_counts(counts: i32, angle: u16) -> i32 {
    (counts as i64 * angle as i64 + 90).div_euclid(180) as i32
}

/// The calibration in force for a profile: its own, else the global one.
pub fn turn_for(profile: &Profile, extras: &Extras) -> TurnCal {
    profile.turn.clone().unwrap_or(TurnCal { counts: extras.p180_counts, game: extras.p180_game, sens: extras.p180_sens, angle: extras.p180_angle })
}

/// Index of the FORTNITE preset in `P180_GAMES`.
pub const P180_FORTNITE: usize = 5;

/// The sensitivity in the preset's own unit. Fortnite shows a percentage (13.8 %), but players
/// also know it as the decimal (0.138): a Fortnite value below 1.0 is taken as the decimal.
pub fn p180_sens_for(game: usize, typed: f32) -> f32 {
    if game == P180_FORTNITE && typed < 1.0 { typed * 100.0 } else { typed }
}

/// Counts for a 180 degree turn from a preset and the sensitivity as typed (`None` for CUSTOM or bad input).
pub fn p180_preset_counts(game: usize, typed: f32) -> Option<i32> {
    p180_counts_for(P180_GAMES.get(game)?.1, p180_sens_for(game, typed))
}

/// Fortnite settings saved before decimal input existed hold counts computed from the decimal as if it
/// were a percentage; recompute them. Idempotent.
fn fix_fortnite_counts(game: usize, sens: f32, counts: &mut i32) {
    if game == P180_FORTNITE
        && sens < 1.0
        && p180_counts_for(P180_GAMES[P180_FORTNITE].1, sens) == Some(*counts)
        && let Some(c) = p180_preset_counts(game, sens)
    {
        *counts = c;
    }
}

/// Centimetres of mouse travel for a full 360 turn at `dpi` (0 when `dpi` is 0).
pub fn p180_travel_cm(counts: i32, dpi: u16) -> f32 {
    if dpi == 0 { 0.0 } else { 2.0 * counts as f32 / dpi as f32 * 2.54 }
}

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
    /// Turn angle in degrees (180 = a half turn).
    pub p180_angle: u16,
    pub scroll_accel: bool,
    /// Extra wheel ticks per tick at full speed.
    pub scroll_boost: f32,
}

impl Default for Extras {
    fn default() -> Self {
        Self { p180_counts: 8182, p180_ms: 60, p180_steps: 12, p180_game: 0, p180_sens: 1.0, p180_angle: 180, scroll_accel: false, scroll_boost: 3.0 }
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
    /// Pop-up with the new DPI speed after a DPI button.
    pub osd_dpi: bool,
    /// Pop-up with the profile name after a profile change.
    pub osd_profile: bool,
    /// Flash both lighting zones in the speed's color after a DPI button.
    pub dpi_flash: bool,
    /// Keep the DPI LEDs lit (the mouse stores this; GHUB-X also lights the level while it controls).
    pub dpi_led_always_on: bool,
}

impl Default for General {
    fn default() -> Self {
        Self {
            launch_on_login: false,
            take_control: true,
            lighting_sleep_min: 0,
            close_to_tray: true,
            start_minimized: false,
            osd_dpi: true,
            osd_profile: true,
            dpi_flash: false,
            dpi_led_always_on: false,
        }
    }
}

/// Lighting flash color for a DPI level index (wraps after five).
pub fn dpi_flash_color(level: usize) -> [u8; 3] {
    [[0xFF, 0x3B, 0x30], [0x34, 0xC7, 0x59], [0x0A, 0x84, 0xFF], [0xFF, 0xD6, 0x0A], [0xBF, 0x5A, 0xF2]][level % 5]
}

/// A user-made colour theme (Settings > Appearance > MY THEMES).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct CustomTheme {
    /// `custom-<new_id()>`; never shown.
    pub id: String,
    pub name: String,
    pub bg: [u8; 3],
    pub surface: [u8; 3],
    pub text: [u8; 3],
    pub accent: [u8; 3],
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Appearance {
    /// Preset id (see `ui::palette::THEMES`) or the id of a custom theme.
    pub theme: String,
    /// Custom accent colour; `None` keeps the theme's own.
    pub accent: Option<[u8; 3]>,
    /// Use the logo zone's lighting colour as the accent.
    pub follow_lighting: bool,
    pub custom: Vec<CustomTheme>,
}

impl Default for Appearance {
    fn default() -> Self {
        Self { theme: "ghub-dark".into(), accent: None, follow_lighting: false, custom: Vec::new() }
    }
}

impl Appearance {
    pub const MAX_CUSTOM: usize = 20;

    /// Add a custom theme with a trimmed, unique name and a fresh id. `None` at the limit.
    pub fn add_theme(&mut self, mut t: CustomTheme) -> Option<usize> {
        if self.custom.len() >= Self::MAX_CUSTOM {
            return None;
        }
        t.name = self.normalize_name(&t.name, None);
        if t.id.is_empty() || self.custom.iter().any(|c| c.id == t.id) {
            let mut id = format!("custom-{}", new_id());
            let mut n = 2;
            while self.custom.iter().any(|c| c.id == id) {
                id = format!("custom-{}-{n}", new_id());
                n += 1;
            }
            t.id = id;
        }
        self.custom.push(t);
        Some(self.custom.len() - 1)
    }

    /// Trimmed to 24 characters, "My Theme" when empty, then made unique against the other themes.
    pub fn normalize_name(&self, name: &str, skip: Option<usize>) -> String {
        let name: String = name.trim().chars().take(24).collect();
        self.unique_name(if name.is_empty() { "My Theme" } else { &name }, skip)
    }

    /// `name`, or `name (2)`, `name (3)`... when another theme (not `skip`) already uses it.
    pub fn unique_name(&self, name: &str, skip: Option<usize>) -> String {
        let taken = |n: &str| self.custom.iter().enumerate().any(|(i, c)| Some(i) != skip && c.name.eq_ignore_ascii_case(n));
        let mut out = name.to_string();
        let mut n = 2;
        while taken(&out) {
            out = format!("{name} ({n})");
            n += 1;
        }
        out
    }
}

/// A custom theme as a shareable `.ghubxtheme` file (TOML, id not included).
pub fn export_theme(t: &CustomTheme) -> String {
    #[derive(Serialize)]
    struct Body<'a> {
        name: &'a str,
        bg: [u8; 3],
        surface: [u8; 3],
        text: [u8; 3],
        accent: [u8; 3],
    }
    #[derive(Serialize)]
    struct File<'a> {
        format: u32,
        theme: Body<'a>,
    }
    toml::to_string_pretty(&File { format: 1, theme: Body { name: &t.name, bg: t.bg, surface: t.surface, text: t.text, accent: t.accent } }).unwrap_or_default()
}

pub fn import_theme(text: &str) -> Result<CustomTheme, String> {
    #[derive(Deserialize)]
    struct Body {
        name: String,
        bg: [u8; 3],
        surface: [u8; 3],
        text: [u8; 3],
        accent: [u8; 3],
    }
    #[derive(Deserialize)]
    struct File {
        format: Option<u32>,
        theme: Option<Body>,
    }
    let f: File = toml::from_str(text).map_err(|e| format!("Not a GHUB-X theme file: {e}"))?;
    if f.format != Some(1) {
        return Err("Unsupported theme file format".into());
    }
    let b = f.theme.ok_or("The file has no theme")?;
    Ok(CustomTheme { id: String::new(), name: b.name, bg: b.bg, surface: b.surface, text: b.text, accent: b.accent })
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
    /// The user's own OBS actions with a target (scene, source, mixer...).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub obs_actions: Vec<ObsAction>,
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
            obs_actions: Vec::new(),
        }
    }
}

impl Config {
    /// Make a freshly loaded or imported app consistent (same repair a loaded config gets).
    pub fn sanitize_app(a: &mut AppEntry) {
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

    /// Add an imported app with a unique name. The second value is false when the
    /// app's executable or folder clashed with an existing entry and was cleared
    /// (so it will not activate by itself).
    pub fn add_imported(&mut self, mut app: AppEntry) -> (usize, bool) {
        let base = app.name.clone();
        let mut n = 2;
        while self.apps.iter().any(|a| a.name.eq_ignore_ascii_case(&app.name)) {
            app.name = format!("{base} ({n})");
            n += 1;
        }
        let clash = self.apps.iter().any(|a| {
            let same_exe = matches!((&a.exe, &app.exe), (Some(x), Some(y)) if x.eq_ignore_ascii_case(y));
            let same_dir = matches!((&a.dir, &app.dir), (Some(x), Some(y)) if path_key(x).trim_end_matches('\\') == path_key(y).trim_end_matches('\\'));
            same_exe || same_dir
        });
        if clash {
            app.exe = None;
            app.dir = None;
        }
        self.apps.push(app);
        (self.apps.len() - 1, !clash)
    }

    /// Repair indices after edits/deserialization.
    pub fn sanitize(&mut self) {
        if self.apps.is_empty() {
            self.apps.push(AppEntry::desktop());
        }
        for a in &mut self.apps {
            Self::sanitize_app(a);
        }
        self.current_app = self.current_app.min(self.apps.len() - 1);
        fix_fortnite_counts(self.extras.p180_game, self.extras.p180_sens, &mut self.extras.p180_counts);
        for p in self.apps.iter_mut().flat_map(|a| a.profiles.iter_mut()) {
            if let Some(t) = &mut p.turn {
                fix_fortnite_counts(t.game, t.sens, &mut t.counts);
            }
        }
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

    /// The (app, profile) that should drive the mouse given the foreground
    /// process path. An executable-name match beats an install-folder match;
    /// among folders the longest wins.
    pub fn resolve(&self, foreground_path: Option<&str>) -> (usize, usize) {
        if let Some(p) = self.persistent {
            return p;
        }
        if self.profile_switching
            && let Some(path) = foreground_path
            && let Some(i) = self.match_app(path)
        {
            return (i, self.apps[i].active_profile);
        }
        (self.current_app, self.apps[self.current_app].active_profile)
    }

    fn match_app(&self, path: &str) -> Option<usize> {
        let exe = path.rsplit(['\\', '/']).next().unwrap_or(path);
        let by_exe = self.apps.iter().position(|a| a.enabled && a.exe.as_deref().is_some_and(|e| e.eq_ignore_ascii_case(exe)));
        by_exe.or_else(|| {
            let p = path_key(path);
            self.apps
                .iter()
                .enumerate()
                .filter(|(_, a)| a.enabled)
                .filter_map(|(i, a)| {
                    let d = path_key(a.dir.as_deref()?);
                    let d = d.trim_end_matches('\\');
                    (!d.is_empty() && p.len() > d.len() && p.starts_with(d) && p.as_bytes()[d.len()] == b'\\').then_some((d.len(), i))
                })
                .max_by_key(|&(len, _)| len)
                .map(|(_, i)| i)
        })
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
    /// Active DPI level index and DPI shift level index.
    pub active_dpi: usize,
    pub shift_dpi: usize,
    /// Button entries of the base and G-Shift layer (G1..G11).
    pub layers: [[[u8; 4]; SLOTS]; 2],
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
        let units: Vec<u16> = s[0xA0..0xD0].as_chunks::<2>().0.iter().map(|c| u16::from_le_bytes(*c)).take_while(|&u| u != 0 && u != 0xFFFF).collect();
        let mut layers = [[[0u8; 4]; SLOTS]; 2];
        for (li, layer) in layers.iter_mut().enumerate() {
            for (slot, e) in layer.iter_mut().enumerate() {
                let o = 0x20 + li * 0x40 + slot * 4;
                e.copy_from_slice(&s[o..o + 4]);
            }
        }
        let mut leds = [[0u8; 11]; 2];
        leds[0].copy_from_slice(&s[0xD0..0xDB]);
        leds[1].copy_from_slice(&s[0xDB..0xE6]);
        Some(Self {
            rate_ms: s[0],
            default_dpi: s[1] as usize,
            dpi,
            name: String::from_utf16_lossy(&units),
            leds,
            active_dpi: s[1] as usize,
            shift_dpi: s[2] as usize,
            layers,
        })
    }
}


/// A profile as stored in an on-board sector, plus what could not be stored.
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardEncoding {
    pub sector: [u8; SECTOR_SIZE],
    /// (slot, G-Shift layer, label of what the mouse cannot store).
    pub not_stored: Vec<(usize, bool, String)>,
}

fn hid_usage_of(vk: u16) -> Option<u8> {
    (0u32..=0xE7).find(|&u| crate::ghub_import::hid_to_vk(u) == Some(vk)).map(|u| u as u8)
}

fn consumer_usage(vk: u16) -> Option<u16> {
    Some(match vk {
        0xB0 => 0x00B5,
        0xB1 => 0x00B6,
        0xB2 => 0x00B7,
        0xB3 => 0x00CD,
        0xAD => 0x00E2,
        0xAF => 0x00E9,
        0xAE => 0x00EA,
        _ => return None,
    })
}

fn assignment_label(a: &Assignment, macros: &[Macro]) -> String {
    match a {
        Assignment::Macro(id) => macros.iter().find(|m| m.id == *id).map(|m| format!("{} (macro)", m.name)).unwrap_or_else(|| "macro".into()),
        Assignment::Launch { name, .. } => format!("{name} (launch)"),
        Assignment::Perfect180 { .. } => "Perfect 180".into(),
        Assignment::ToggleScrollAccel => "scroll acceleration".into(),
        Assignment::Obs(a) => crate::catalog::obs_name(a),
        Assignment::System(SystemFn::ReportRateCycle) => "report rate cycle".into(),
        Assignment::Mouse(MouseFn::DoubleClick) => "double click".into(),
        Assignment::Key(k) | Assignment::Shortcut { combo: k, .. } => format!("key {:#04X}", k.vk),
        _ => "button".into(),
    }
}

/// 4-byte button entry; `None` when the mouse cannot store the assignment.
fn encode_button(a: &Assignment) -> Option<[u8; 4]> {
    let special = |c: u8| Some([0x90, c, 0, 0]);
    match a {
        Assignment::Default => None,
        Assignment::Disabled => special(0),
        Assignment::Mouse(m) => match m {
            MouseFn::Primary => Some([0x80, 1, 0, 1]),
            MouseFn::Secondary => Some([0x80, 1, 0, 2]),
            MouseFn::Middle => Some([0x80, 1, 0, 4]),
            MouseFn::Back => Some([0x80, 1, 0, 8]),
            MouseFn::Forward => Some([0x80, 1, 0, 16]),
            MouseFn::WheelLeft => special(0x01),
            MouseFn::WheelRight => special(0x02),
            MouseFn::WheelDown => special(0x10),
            MouseFn::WheelUp => special(0x11),
            MouseFn::DoubleClick => None,
        },
        Assignment::Key(k) | Assignment::Shortcut { combo: k, .. } => {
            if let Some(c) = consumer_usage(k.vk) {
                return Some([0x80, 3, (c >> 8) as u8, c as u8]);
            }
            hid_usage_of(k.vk).map(|u| [0x80, 2, k.mods & 0x0F, u])
        }
        Assignment::System(s) => match s {
            SystemFn::DpiUp => special(0x03),
            SystemFn::DpiDown => special(0x04),
            SystemFn::DpiCycle => special(0x05),
            SystemFn::DpiDefault => special(0x06),
            SystemFn::DpiShift => special(0x07),
            SystemFn::OnboardProfileCycle | SystemFn::ProfileCycle => special(0x0A),
            SystemFn::GShift => special(0x0B),
            SystemFn::ReportRateCycle => None,
        },
        _ => None,
    }
}

/// Zone payload as stored on-board: like `Lighting::to_led` but the intensity byte is the brightness.
fn onboard_zone(l: &Lighting) -> [u8; 11] {
    let host = l.effect.is_host();
    let eff = if host { Effect::Fixed } else { l.effect };
    let mut b = Lighting { effect: eff, ..*l }.to_led();
    let br = l.brightness.clamp(1, 100);
    match eff {
        Effect::Breathing => b[7] = br,
        Effect::Cycle => b[8] = br,
        _ => {}
    }
    b
}

/// The sector G HUB writes for this profile (byte-exact).
pub fn encode_onboard(p: &Profile, name: &str, macros: &[Macro]) -> OnboardEncoding {
    let mut s = [0xFFu8; SECTOR_SIZE];
    s[0] = p.rate_ms;
    let idx = |d: u16| p.dpi.levels.iter().position(|&x| x == d).unwrap_or(0) as u8;
    s[1] = idx(p.dpi.active);
    s[2] = idx(p.dpi.shift);
    for i in 0..5 {
        let v = p.dpi.levels.get(i).copied().unwrap_or(0);
        s[3 + i * 2..5 + i * 2].copy_from_slice(&v.to_le_bytes());
    }
    s[0x11] = 0;
    let mut not_stored = Vec::new();
    for (li, gshift) in [(0usize, false), (1, true)] {
        let base = 0x20 + li * 0x40;
        for slot in 0..16 {
            let entry = if slot >= SLOTS {
                [0xFF; 4]
            } else {
                let own = p.layer(gshift).get(slot).cloned().unwrap_or_default();
                let a = if own == Assignment::Default && gshift { p.base.get(slot).cloned().unwrap_or_default() } else { own };
                let a = if a == Assignment::Default { native(slot) } else { a };
                match encode_button(&a) {
                    Some(e) => e,
                    None => {
                        not_stored.push((slot, gshift, assignment_label(&a, macros)));
                        [0x90, 0, 0, 0]
                    }
                }
            };
            s[base + slot * 4..base + slot * 4 + 4].copy_from_slice(&entry);
        }
    }
    s[0xA0..0xD0].fill(0);
    for (i, u) in name.encode_utf16().take(23).enumerate() {
        s[0xA0 + i * 2..0xA2 + i * 2].copy_from_slice(&u.to_le_bytes());
    }
    s[0xD0..0xDB].copy_from_slice(&onboard_zone(&p.lighting[0]));
    s[0xDB..0xE6].copy_from_slice(&onboard_zone(&p.lighting[1]));
    s[0xFC] = 0;
    set_crc(&mut s);
    OnboardEncoding { sector: s, not_stored }
}

fn set_crc(s: &mut [u8; SECTOR_SIZE]) {
    let c = crc16(&s[..SECTOR_SIZE - 2]).to_be_bytes();
    s[SECTOR_SIZE - 2..].copy_from_slice(&c);
}

/// Sector 0 for five slots (sectors 1..=5) with the given enabled flags.
pub fn encode_directory(enabled: [bool; 5]) -> [u8; SECTOR_SIZE] {
    let mut s = [0xFFu8; SECTOR_SIZE];
    for (i, en) in enabled.iter().enumerate() {
        s[i * 4..i * 4 + 4].copy_from_slice(&[0, i as u8 + 1, u8::from(*en), 0]);
    }
    s[20..24].copy_from_slice(&[0xFF, 0xFF, 0, 0]);
    set_crc(&mut s);
    s
}

/// A ROM sector (0x0101..) as the user sector: same bytes, CRC recomputed.
pub fn restore_from_rom(rom: &[u8; SECTOR_SIZE]) -> [u8; SECTOR_SIZE] {
    let mut s = *rom;
    set_crc(&mut s);
    s
}

/// Plain-language text of one 4-byte button entry.
pub fn describe_button(e: [u8; 4]) -> String {
    match e[0] {
        0x80 => match e[1] {
            1 => match e[3] {
                1 => "Left click".into(),
                2 => "Right click".into(),
                4 => "Middle click".into(),
                8 => "Back".into(),
                16 => "Forward".into(),
                b => format!("Mouse button {b:#04X}"),
            },
            2 => {
                let mut t = String::new();
                for (bit, n) in [(MOD_CTRL, "Ctrl+"), (MOD_SHIFT, "Shift+"), (MOD_ALT, "Alt+"), (MOD_WIN, "Win+")] {
                    if e[2] & bit != 0 {
                        t += n;
                    }
                }
                match crate::ghub_import::hid_to_vk(e[3] as u32) {
                    Some(vk @ 0x41..=0x5A) | Some(vk @ 0x30..=0x39) => format!("Key {t}{}", vk as u8 as char),
                    _ => format!("Key {t}#{:02X}", e[3]),
                }
            }
            3 => match u16::from_be_bytes([e[2], e[3]]) {
                0xB5 => "Next track".into(),
                0xB6 => "Previous track".into(),
                0xB7 => "Stop".into(),
                0xCD => "Play / pause".into(),
                0xE2 => "Mute".into(),
                0xE9 => "Volume up".into(),
                0xEA => "Volume down".into(),
                u => format!("Media {u:#06X}"),
            },
            _ => "Unknown".into(),
        },
        0x90 => match e[1] {
            0 => "Nothing".into(),
            1 => "Wheel left".into(),
            2 => "Wheel right".into(),
            3 => "DPI up".into(),
            4 => "DPI down".into(),
            5 => "DPI cycle".into(),
            6 => "DPI default".into(),
            7 => "DPI shift".into(),
            0x0A => "Profile cycle".into(),
            0x0B => "G-Shift".into(),
            0x10 => "Wheel down".into(),
            0x11 => "Wheel up".into(),
            c => format!("Special {c:#04X}"),
        },
        _ => "Unused".into(),
    }
}

/// Buttons of a stored profile (base layer) that differ from the factory layout: "G4: Nothing".
pub fn button_diffs(p: &OnboardProfile) -> Vec<String> {
    (0..SLOTS)
        .filter_map(|slot| {
            let now = describe_button(p.layers[0][slot]);
            let factory = describe_button(encode_button(&native(slot)).unwrap_or([0xFF; 4]));
            (now != factory).then(|| format!("G{}: {now}", slot + 1))
        })
        .collect()
}

/// Sector 1 as read from a G502 HERO (used by the `onboard` shot page and tests).
pub fn sample_sector1() -> [u8; SECTOR_SIZE] {
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

/// The profile directory (sector 0): 4-byte entries `[sector hi][sector lo][enabled][0]`,
/// ending at sector 0xFFFF.
pub fn parse_directory(s: &[u8; SECTOR_SIZE]) -> Vec<(u16, bool)> {
    s.as_chunks::<4>().0.iter()
        .map(|e| (u16::from_be_bytes([e[0], e[1]]), e[2] != 0))
        .take_while(|&(sector, _)| sector != 0xFFFF)
        .collect()
}

/// One entry of the mouse's on-board profile directory, read-only.
#[derive(Debug, Clone, PartialEq)]
pub struct OnboardSlot {
    pub sector: u16,
    pub enabled: bool,
    pub profile: Option<OnboardProfile>,
}

/// "1200 · 2400 · 3200 · 6400 DPI · 1000 Hz".
pub fn onboard_summary(p: &OnboardProfile) -> String {
    let dpi: Vec<String> = p.dpi.iter().map(|d| d.to_string()).collect();
    format!("{} DPI \u{b7} {} Hz", dpi.join(" \u{b7} "), crate::device::rate_hz(p.rate_ms))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn p180_presets_keep_their_indices() {
        let want = [("CS2/APEX", 0.022), ("VALORANT", 0.07), ("OW2/COD", 0.0066), ("CUSTOM", 0.0), ("R6 SIEGE", 0.005_729_578), ("FORTNITE", 0.005555)];
        assert_eq!(P180_GAMES, want);
        assert_eq!(P180_CUSTOM, 3);
        assert_eq!(p180_counts_for(0.005_729_578, 50.0), Some(628));
        assert_eq!(p180_counts_for(0.005555, 8.0), Some(4050));
    }

    #[test]
    fn turn_counts_scale_with_angle() {
        assert_eq!(turn_counts(8182, 180), 8182);
        assert_eq!(turn_counts(8182, 90), 4091);
        assert_eq!(turn_counts(8182, 360), 16364);
        assert_eq!(turn_counts(1801, 45), 450);
    }

    #[test]
    fn profile_turn_overrides_global() {
        let extras = Extras { p180_counts: 5000, p180_game: 2, p180_sens: 2.5, p180_angle: 120, ..Default::default() };
        let mut p = Profile::new("x");
        assert_eq!(turn_for(&p, &extras), TurnCal { counts: 5000, game: 2, sens: 2.5, angle: 120 });
        p.turn = Some(TurnCal { counts: 900, game: 4, sens: 0.5, angle: 90 });
        assert_eq!(turn_for(&p, &extras), TurnCal { counts: 900, game: 4, sens: 0.5, angle: 90 });
    }

    #[test]
    fn dpi_flash_colors() {
        let want = [[0xFF, 0x3B, 0x30], [0x34, 0xC7, 0x59], [0x0A, 0x84, 0xFF], [0xFF, 0xD6, 0x0A], [0xBF, 0x5A, 0xF2]];
        for (i, c) in want.iter().enumerate() {
            assert_eq!(dpi_flash_color(i), *c);
        }
        assert_eq!(dpi_flash_color(5), dpi_flash_color(0));
    }

    fn dir_app(name: &str, exe: Option<&str>, dir: Option<&str>) -> AppEntry {
        AppEntry { name: name.into(), exe: exe.map(Into::into), dir: dir.map(Into::into), ..AppEntry::desktop() }
    }

    fn sample_app() -> AppEntry {
        let mut a = dir_app("Game", None, Some(r"C:\Games\G"));
        a.ghub_id = Some("id-1".into());
        a.profiles.push(Profile::new("Second"));
        a.profiles[1].dpi.active = 800;
        a.active_profile = 1;
        a.macros.push(Macro { id: "m1".into(), name: "jump".into(), kind: MacroKind::Toggle, steps: vec![MacroStep::key(0x20, true, 5)], ..Default::default() });
        a.profiles[0].base[3] = Assignment::Macro("m1".into());
        a
    }

    #[test]
    fn export_import_round_trip() {
        let a = sample_app();
        assert_eq!(import_app(&export_app(&a)).unwrap(), a);
    }

    #[test]
    fn import_rejects_garbage() {
        assert!(import_app("hello").is_err());
        assert!(import_app("format = 2\n").is_err());
        let mut a = sample_app();
        a.profiles.clear();
        assert!(import_app(&export_app(&a)).is_err());
    }

    #[test]
    fn add_imported_makes_names_unique_and_unlinks_clashes() {
        let mut c = Config::default();
        let (i, linked) = c.add_imported(dir_app("Desktop", None, None));
        assert_eq!((c.apps[i].name.as_str(), linked), ("Desktop (2)", true));
        let (i, _) = c.add_imported(dir_app("Desktop", None, None));
        assert_eq!(c.apps[i].name, "Desktop (3)");
        c.apps.push(dir_app("CS", Some("cs2.exe"), Some(r"C:\Games\CS")));
        let (i, linked) = c.add_imported(dir_app("Other", None, Some("c:/games/cs/")));
        assert!(!linked);
        assert_eq!((c.apps[i].exe.clone(), c.apps[i].dir.clone()), (None, None));
        let (i, linked) = c.add_imported(dir_app("Other", Some("CS2.EXE"), None));
        assert!(!linked);
        assert_eq!(c.apps[i].name, "Other (2)");
    }

    fn sample_theme(name: &str) -> CustomTheme {
        CustomTheme { id: String::new(), name: name.into(), bg: [0x1A, 0x10, 0x23], surface: [0x2A, 0x1B, 0x3D], text: [0xF6, 0xEE, 0xFC], accent: [0xFF, 0x7A, 0x59] }
    }

    #[test]
    fn theme_export_import_round_trip() {
        let mut t = sample_theme("Sunset");
        t.id = "custom-x".into();
        let back = import_theme(&export_theme(&t)).unwrap();
        assert_eq!(back, CustomTheme { id: String::new(), ..t });
    }

    #[test]
    fn import_theme_rejects_garbage() {
        assert!(import_theme("hello").is_err());
        assert!(import_theme("format = 2\n").is_err());
        assert!(import_theme("format = 1\n").is_err());
    }

    #[test]
    fn add_theme_names_ids_and_limit() {
        let mut a = Appearance::default();
        let i = a.add_theme(sample_theme("Sunset")).unwrap();
        let j = a.add_theme(sample_theme("sunset")).unwrap();
        assert_eq!((a.custom[i].name.as_str(), a.custom[j].name.as_str()), ("Sunset", "sunset (2)"));
        assert!(a.custom[i].id.starts_with("custom-") && a.custom[j].id.starts_with("custom-"));
        assert_ne!(a.custom[i].id, a.custom[j].id);
        let k = a.add_theme(sample_theme("   ")).unwrap();
        assert_eq!(a.custom[k].name, "My Theme");
        while a.custom.len() < Appearance::MAX_CUSTOM {
            a.add_theme(sample_theme("Filler")).unwrap();
        }
        assert!(a.add_theme(sample_theme("One too many")).is_none());
    }

    #[test]
    fn host_effects_round_trip_and_old_configs_load() {
        let mut c = Config::default();
        let host = HostFx { area: ScreenArea::Center, boost: 70, smoothing: 10, color2: [1, 2, 3], intensity: 20, auto_sens: false, sensitivity: 80, bass_only: true };
        c.apps[0].profiles[0].lighting[1] = Lighting { effect: Effect::Screen, host, ..Default::default() };
        let text = toml::to_string_pretty(&c).unwrap();
        let back: Config = toml::from_str(&text).unwrap();
        assert_eq!(back.apps[0].profiles[0].lighting[1].host, host);
        assert_eq!(back.apps[0].profiles[0].lighting[1].effect, Effect::Screen);
        let plain = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!plain.contains("host"), "default host written out");
        let back: Config = toml::from_str(&plain).unwrap();
        assert_eq!(back.apps[0].profiles[0].lighting[0].host, HostFx::default());
    }

    fn fixture(b: &[u8]) -> [u8; 256] {
        b.try_into().expect("256 byte fixture")
    }

    fn ghub_default_profile() -> (Profile, Vec<Macro>) {
        let macros = vec![
            Macro { id: "jump".into(), name: "jump".into(), ..Default::default() },
            Macro { id: "cl".into(), name: "cl".into(), ..Default::default() },
        ];
        let mut p = Profile { name: "Default".into(), dpi: DpiTable { levels: vec![600, 1000, 1600], active: 600, default: 1600, shift: 600 }, rate_ms: 1, base: default_slots(), gshift: default_slots(), lighting: [Lighting::default(); 2], turn: None };
        p.base[3] = Assignment::Macro("jump".into());
        p.base[4] = Assignment::Macro("cl".into());
        p.base[5] = Assignment::Key(KeyCombo { vk: 0x53, mods: 0 });
        p.lighting = [Lighting { effect: Effect::Cycle, period_ms: 8000, brightness: 100, ..Default::default() }; 2];
        (p, macros)
    }

    #[test]
    fn encodes_the_sector_ghub_wrote() {
        let (p, macros) = ghub_default_profile();
        let e = encode_onboard(&p, "PROFILE_NAME_DEFAULT", &macros);
        assert_eq!(e.sector, fixture(include_bytes!("../tests/fixtures/ghub_slot5_desktop_default.bin")));
        let mut got: Vec<(usize, bool)> = e.not_stored.iter().map(|(s, g, _)| (*s, *g)).collect();
        got.sort();
        assert_eq!(got, vec![(3, false), (3, true), (4, false), (4, true)]);
    }

    #[test]
    fn encodes_directory_and_rom_restore() {
        assert_eq!(encode_directory([true; 5]), fixture(include_bytes!("../tests/fixtures/factory_directory.bin")));
        assert_eq!(restore_from_rom(&fixture(include_bytes!("../tests/fixtures/rom_slot5.bin"))), fixture(include_bytes!("../tests/fixtures/factory_slot5.bin")));
    }

    #[test]
    fn encoded_profile_parses_back() {
        let (mut p, macros) = ghub_default_profile();
        p.lighting[1] = Lighting { effect: Effect::Breathing, color: [0, 0xDC, 0xFF], period_ms: 8000, brightness: 100, ..Default::default() };
        let e = encode_onboard(&p, "Desktop: Default", &macros);
        let back = OnboardProfile::parse(&e.sector).unwrap();
        assert_eq!((back.dpi.clone(), back.rate_ms, back.name.as_str()), (vec![600, 1000, 1600], 1, "Desktop: Default"));
        assert_eq!(back.leds[0], onboard_zone(&p.lighting[0]));
        assert_eq!(back.leds[1], onboard_zone(&p.lighting[1]));
        let rom2 = fixture(include_bytes!("../tests/fixtures/factory_slot2.bin"));
        let mut mine = onboard_zone(&p.lighting[1]);
        mine[7] = rom2[0xD0 + 7];
        assert_eq!(mine, rom2[0xD0..0xDB]);
    }

    fn rich_macro() -> Macro {
        let steps = vec![
            MacroStep::key(0x41, true, 0),
            MacroStep::pause(25),
            MacroStep::text("h\u{e9}llo \"q\"\n", 5),
            MacroStep::run(Assignment::Launch { name: "App".into(), path: "C:\\x\\y.exe".into() }, 0),
            MacroStep::run(Assignment::System(SystemFn::DpiUp), 1),
            MacroStep::run(Assignment::Mouse(MouseFn::WheelUp), 2),
            MacroStep::run(Assignment::Shortcut { name: "Copy".into(), combo: KeyCombo { vk: 0x43, mods: MOD_CTRL } }, 3),
            MacroStep::run(Assignment::ToggleScrollAccel, 4),
        ];
        Macro { id: "m".into(), name: "rich".into(), kind: MacroKind::Sequence, hold: steps.clone(), release: steps[..2].to_vec(), steps, std_delay: Some(35), show_up_down: false, color: Some([1, 2, 3]) }
    }

    #[test]
    fn old_macro_files_still_load() {
        let old = "id = \"m\"\nname = \"x\"\nkind = \"Toggle\"\n[[steps]]\nvk = 65\ndown = true\ndelay_ms = 0\n";
        let m: Macro = toml::from_str(old).unwrap();
        assert!(m.show_up_down && m.hold.is_empty() && m.std_delay.is_none() && m.color.is_none() && m.steps[0].act.is_none());
        let seed: Config = toml::from_str(include_str!("../tools/ui_seed_config.toml")).unwrap();
        let ms = &seed.apps[0].macros;
        assert_eq!(ms.len(), 3);
        assert!(ms.iter().all(|m| m.standard_delay() == Some(50)));
    }

    #[test]
    fn macros_round_trip_through_toml_and_export() {
        let mut c = Config::default();
        c.apps[0].macros.push(rich_macro());
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.apps[0].macros, c.apps[0].macros);
        let text = export_app(&c.apps[0]);
        let imported = import_app(&text).unwrap();
        assert_eq!(imported.macros, c.apps[0].macros);
        let plain = toml::to_string_pretty(&Macro { id: "p".into(), ..Default::default() }).unwrap();
        assert!(!plain.contains("hold") && !plain.contains("release") && !plain.contains("std_delay") && !plain.contains("color"));
    }

    #[test]
    fn standard_delay_rules() {
        let k = MacroStep::key;
        let mut m = Macro { std_delay: Some(70), ..Default::default() };
        assert_eq!(m.standard_delay(), Some(70));
        m.std_delay = None;
        assert_eq!(m.standard_delay(), None);
        m.steps = vec![k(1, true, 0), k(1, false, 50), MacroStep::pause(50)];
        assert_eq!(m.standard_delay(), Some(50));
        m.steps = vec![k(1, true, 0), k(1, false, 50), k(2, true, 80)];
        assert_eq!(m.standard_delay(), None);
        m.steps = vec![k(1, true, 0), MacroStep::pause(50), k(1, false, 50)];
        assert_eq!(m.standard_delay(), None);
        m.steps = vec![k(1, true, 0), k(1, false, 0)];
        assert_eq!(m.standard_delay(), None);
    }

    #[test]
    fn apply_standard_delay_per_kind() {
        let base = |kind| Macro { kind, steps: vec![MacroStep::key(1, true, 7), MacroStep::pause(9), MacroStep::key(1, false, 3)], hold: vec![MacroStep::key(2, true, 4)], release: vec![MacroStep::key(3, true, 4), MacroStep::key(3, false, 5)], ..Default::default() };
        let delays = |l: &[MacroStep]| l.iter().map(|s| (s.is_pause(), s.delay_ms)).collect::<Vec<_>>();
        let mut m = base(MacroKind::NoRepeat);
        m.apply_standard_delay(30);
        assert_eq!(delays(&m.steps), [(false, 0), (false, 30)]);
        assert_eq!(delays(&m.hold), [(false, 0)]);
        assert_eq!(m.std_delay, Some(30));
        let mut m = base(MacroKind::Toggle);
        m.apply_standard_delay(30);
        assert_eq!(delays(&m.steps), [(false, 0), (false, 30), (true, 30)]);
        assert_eq!(delays(&m.release), [(false, 0), (false, 30)]);
        let mut m = base(MacroKind::Sequence);
        m.apply_standard_delay(30);
        assert_eq!(delays(&m.steps), [(false, 0), (false, 30)]);
        assert_eq!(delays(&m.hold), [(false, 0), (true, 30)]);
        assert_eq!(delays(&m.release), [(false, 0), (false, 30)]);
        let mut m = base(MacroKind::RepeatWhileHolding);
        m.apply_standard_delay(30);
        assert_eq!(delays(&m.steps), [(false, 0), (false, 30), (true, 30)]);
    }

    fn every_obs_action() -> Vec<ObsAction> {
        let mut v = ObsAction::SIMPLE.to_vec();
        v.extend(["a", "b c"].iter().flat_map(|n| {
            let n = n.to_string();
            [ObsAction::Scene(n.clone()), ObsAction::Collection(n.clone()), ObsAction::SourceOn(n.clone()), ObsAction::SourceOff(n.clone()), ObsAction::SourceToggle(n.clone()), ObsAction::MixerMute(n.clone()), ObsAction::MixerUnmute(n.clone()), ObsAction::MixerToggle(n)]
        }));
        v
    }

    #[test]
    fn obs_actions_round_trip_and_are_not_stored_on_the_mouse() {
        let mut c = Config { obs_actions: every_obs_action().into_iter().filter(|a| !ObsAction::SIMPLE.contains(a)).collect(), ..Config::default() };
        for (i, a) in every_obs_action().into_iter().enumerate() {
            c.apps[0].profiles[0].base[i % SLOTS] = Assignment::Obs(a.clone());
            let back: Config = toml::from_str(&toml::to_string_pretty(&{
                let mut one = Config::default();
                one.apps[0].profiles[0].base[0] = Assignment::Obs(a.clone());
                one
            }).unwrap()).unwrap();
            assert_eq!(back.apps[0].profiles[0].base[0], Assignment::Obs(a));
        }
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.obs_actions, c.obs_actions);
        let plain = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(!plain.contains("obs_actions"));
        let old: Config = toml::from_str(&plain).unwrap();
        assert!(old.obs_actions.is_empty());
        let mut p = Profile::new("x");
        p.base[2] = Assignment::Obs(ObsAction::ToggleRecord);
        let enc = encode_onboard(&p, "x", &[]);
        assert!(enc.not_stored.iter().any(|(s, g, _)| *s == 2 && !*g));
        assert!(step_allowed(&Assignment::Obs(ObsAction::StartStream)));
    }

    #[test]
    fn p180_travel_and_percent_presets() {
        let cm = |game: usize, sens: f32| p180_travel_cm(p180_counts_for(P180_GAMES[game].1, sens).unwrap(), 600);
        assert!((cm(5, 0.138) - 1988.0).abs() < 2.0, "{}", cm(5, 0.138));
        assert!((cm(5, 13.8) - 19.9).abs() < 0.2);
        assert!(cm(0, 0.51) < 150.0);
        assert_eq!(p180_travel_cm(1000, 0), 0.0);
        assert!((p180_sens_for(5, 0.138) - 13.8).abs() < 1e-4);
        assert_eq!((p180_sens_for(5, 13.8), p180_sens_for(5, 1.0), p180_sens_for(0, 0.51)), (13.8, 1.0, 0.51));
        assert_eq!(p180_preset_counts(5, 0.138), Some(2348));
        assert_eq!(p180_preset_counts(5, 13.8), Some(2348));
        assert_eq!(p180_preset_counts(3, 1.0), None);
    }

    #[test]
    fn old_fortnite_counts_are_migrated_once() {
        let turn = |g, s, c| TurnCal { counts: c, game: g, sens: s, angle: 180 };
        let mut cfg = Config::default();
        cfg.extras.p180_game = 5;
        cfg.extras.p180_sens = 0.138;
        cfg.extras.p180_counts = 234_806;
        cfg.apps[0].profiles[0].turn = Some(turn(5, 0.138, 234_806));
        cfg.sanitize();
        assert_eq!(cfg.extras.p180_counts, 2348);
        assert_eq!(cfg.apps[0].profiles[0].turn.as_ref().unwrap().counts, 2348);
        let again = cfg.clone();
        cfg.sanitize();
        assert_eq!(cfg, again);
        cfg.apps[0].profiles[0].turn = Some(turn(5, 0.138, 5000));
        cfg.sanitize();
        assert_eq!(cfg.apps[0].profiles[0].turn.as_ref().unwrap().counts, 5000);
        cfg.apps[0].profiles[0].turn = Some(turn(0, 0.51, 16043));
        cfg.sanitize();
        assert_eq!(cfg.apps[0].profiles[0].turn.as_ref().unwrap().counts, 16043);
    }

    #[test]
    fn step_allowed_rules() {
        assert!(step_allowed(&Assignment::System(SystemFn::DpiUp)));
        assert!(step_allowed(&Assignment::Launch { name: "a".into(), path: "b".into() }));
        assert!(step_allowed(&Assignment::Perfect180 { left: true }));
        for a in [Assignment::Default, Assignment::Disabled, Assignment::Macro("x".into()), Assignment::System(SystemFn::DpiShift), Assignment::System(SystemFn::GShift)] {
            assert!(!step_allowed(&a), "{a:?}");
        }
    }

    #[test]
    fn normalize_name_defaults_before_uniqueness() {
        let mut a = Appearance::default();
        a.add_theme(sample_theme("My Theme")).unwrap();
        a.add_theme(sample_theme("B")).unwrap();
        assert_eq!(a.normalize_name("  ", Some(1)), "My Theme (2)");
        assert_eq!(a.normalize_name("my theme", Some(0)), "my theme");
    }

    #[test]
    fn custom_themes_survive_the_config_file() {
        let mut c = Config::default();
        c.appearance.add_theme(sample_theme("Sunset"));
        c.appearance.add_theme(sample_theme("Paper"));
        c.appearance.theme = c.appearance.custom[1].id.clone();
        let back: Config = toml::from_str(&toml::to_string_pretty(&c).unwrap()).unwrap();
        assert_eq!(back.appearance, c.appearance);
        let old: Config = toml::from_str("apps = []").unwrap();
        assert!(old.appearance.custom.is_empty());
    }

    #[test]
    fn resolve_matches_install_dir() {
        let mut c = Config { profile_switching: true, ..Config::default() };
        c.apps.push(dir_app("CS", None, Some(r"C:\Games\CS")));
        assert_eq!(c.resolve(Some("c:/games/cs/game/bin/win64/cs2.exe")), (1, 0));
        assert_eq!(c.resolve(Some(r"C:\Games\CSX\a.exe")), (0, 0));
        c.apps.push(dir_app("A", Some("a.exe"), None));
        assert_eq!(c.resolve(Some(r"C:\Games\CS\a.exe")), (2, 0));
        let mut d = Config { profile_switching: true, ..Config::default() };
        d.apps.push(dir_app("Games", None, Some(r"C:\Games")));
        d.apps.push(dir_app("CS", None, Some(r"C:\Games\CS")));
        assert_eq!(d.resolve(Some(r"C:\Games\CS\x.exe")), (2, 0));
        assert_eq!(d.resolve(Some(r"C:\Games\Other\x.exe")), (1, 0));
    }

    #[test]
    fn old_config_without_dir_loads() {
        let c: Config = toml::from_str("[[apps]]\nname = \"Old\"\nexe = \"old.exe\"\n\n[[apps.profiles]]\nname = \"Default\"\n").unwrap();
        assert_eq!((c.apps[0].dir.clone(), c.apps[0].ghub_id.clone()), (None, None));
    }

    #[test]
    fn config_without_appearance_loads_defaults() {
        let c: Config = toml::from_str("apps = []").unwrap();
        assert_eq!(c.appearance.theme, "ghub-dark");
        assert_eq!(c.appearance.accent, None);
        assert!(!c.appearance.follow_lighting);
    }

    /// Sector 1 as read from the user's G502 HERO (probe round 8).
    fn real_sector1() -> [u8; 256] {
        sample_sector1()
    }

    #[test]
    fn parses_profile_directory() {
        let mut s = [0xFFu8; 256];
        s[..12].copy_from_slice(&[0, 1, 1, 0, 0, 2, 1, 0, 0, 3, 0, 0]);
        assert_eq!(parse_directory(&s), vec![(1, true), (2, true), (3, false)]);
    }

    #[test]
    fn onboard_summary_text() {
        let p = OnboardProfile::parse(&real_sector1()).unwrap();
        assert_eq!(onboard_summary(&p), "1200 \u{b7} 2400 \u{b7} 3200 \u{b7} 6400 DPI \u{b7} 1000 Hz");
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
