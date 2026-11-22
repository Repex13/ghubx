//! Display names and the assignment catalog (COMMANDS / KEYS / ACTIONS /
//! MACROS / SYSTEM), following G HUB's card presets.

use crate::profile::*;

pub fn slot_name(slot: usize) -> String {
    format!("G{}", slot + 1)
}

pub fn mouse_fn_name(m: MouseFn) -> &'static str {
    match m {
        MouseFn::Primary => "Primary Click",
        MouseFn::Secondary => "Secondary Click",
        MouseFn::Middle => "Middle Click",
        MouseFn::Back => "Back",
        MouseFn::Forward => "Forward",
        MouseFn::DoubleClick => "Double Click",
        MouseFn::WheelUp => "Scroll Up",
        MouseFn::WheelDown => "Scroll Down",
        MouseFn::WheelLeft => "Scroll Left",
        MouseFn::WheelRight => "Scroll Right",
    }
}

pub fn system_fn_name(s: SystemFn) -> &'static str {
    match s {
        SystemFn::DpiUp => "DPI Up",
        SystemFn::DpiDown => "DPI Down",
        SystemFn::DpiCycle => "DPI Cycle",
        SystemFn::DpiShift => "DPI Shift",
        SystemFn::DpiDefault => "DPI Default",
        SystemFn::GShift => "G-Shift",
        SystemFn::ProfileCycle => "Profile Cycle",
        SystemFn::OnboardProfileCycle => "Onboard Profile Cycle",
        SystemFn::ReportRateCycle => "Report Rate Cycle",
    }
}

/// How G HUB renders a callout label.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum LabelStyle {
    Plain,
    /// DPI functions are drawn in amber.
    Dpi,
    /// Keystrokes are drawn as a keycap.
    Key,
    /// Macros get an "M" badge.
    Macro,
}

pub fn label(a: &Assignment, slot: usize, macros: &[Macro]) -> (String, LabelStyle) {
    match a {
        Assignment::Default => label(&native(slot), slot, macros),
        Assignment::Disabled => ("Disabled".into(), LabelStyle::Plain),
        Assignment::Mouse(m) => (mouse_fn_name(*m).into(), LabelStyle::Plain),
        Assignment::System(s) => (
            system_fn_name(*s).into(),
            if matches!(s, SystemFn::DpiUp | SystemFn::DpiDown | SystemFn::DpiCycle | SystemFn::DpiShift | SystemFn::DpiDefault) {
                LabelStyle::Dpi
            } else {
                LabelStyle::Plain
            },
        ),
        Assignment::Key(k) => (combo_name(*k), LabelStyle::Key),
        Assignment::Shortcut { name, .. } => (name.clone(), LabelStyle::Plain),
        Assignment::Macro(id) => (
            macros.iter().find(|m| &m.id == id).map(|m| m.name.clone()).unwrap_or_else(|| "Missing macro".into()),
            LabelStyle::Macro,
        ),
        Assignment::Launch { name, .. } => (name.clone(), LabelStyle::Plain),
        Assignment::Perfect180 { left } => (if *left { "Perfect 180 (Left)" } else { "Perfect 180" }.into(), LabelStyle::Plain),
        Assignment::ToggleScrollAccel => ("Scroll Acceleration".into(), LabelStyle::Plain),
    }
}

pub fn vk_name(vk: u16) -> String {
    let s = match vk {
        0x00 => "Delay",
        0x01 => "Left Click",
        0x02 => "Right Click",
        0x04 => "Middle Click",
        0x05 => "Back",
        0x06 => "Forward",
        0x08 => "Backspace",
        0x09 => "Tab",
        0x0D => "Enter",
        0x10 => "Shift",
        0x11 => "Ctrl",
        0x12 => "Alt",
        0x13 => "Pause",
        0x14 => "Caps Lock",
        0x1B => "Esc",
        0x20 => "Space",
        0x21 => "Page Up",
        0x22 => "Page Down",
        0x23 => "End",
        0x24 => "Home",
        0x25 => "Left",
        0x26 => "Up",
        0x27 => "Right",
        0x28 => "Down",
        0x2C => "Print Screen",
        0x2D => "Insert",
        0x2E => "Delete",
        0x5B => "Left Windows",
        0x5C => "Right Windows",
        0x5D => "Menu",
        0x6A => "Num *",
        0x6B => "Num +",
        0x6D => "Num -",
        0x6E => "Num .",
        0x6F => "Num /",
        0x90 => "Num Lock",
        0x91 => "Scroll Lock",
        0xA0 => "Left Shift",
        0xA1 => "Right Shift",
        0xA2 => "Left Ctrl",
        0xA3 => "Right Ctrl",
        0xA4 => "Left Alt",
        0xA5 => "Right Alt",
        0xAD => "Mute",
        0xAE => "Volume Down",
        0xAF => "Volume Up",
        0xB0 => "Next Track",
        0xB1 => "Previous Track",
        0xB2 => "Stop",
        0xB3 => "Play/Pause",
        0xBA => ";",
        0xBB => "=",
        0xBC => ",",
        0xBD => "-",
        0xBE => ".",
        0xBF => "/",
        0xC0 => "`",
        0xDB => "[",
        0xDC => "\\",
        0xDD => "]",
        0xDE => "'",
        _ => "",
    };
    if !s.is_empty() {
        return s.into();
    }
    match vk {
        0x30..=0x39 | 0x41..=0x5A => ((vk as u8) as char).to_string(),
        0x60..=0x69 => format!("Num {}", vk - 0x60),
        0x70..=0x87 => format!("F{}", vk - 0x6F),
        _ => format!("Key {vk:#04X}"),
    }
}

pub fn combo_name(k: KeyCombo) -> String {
    let mut parts = Vec::new();
    for (bit, n) in [(MOD_CTRL, "Ctrl"), (MOD_SHIFT, "Shift"), (MOD_ALT, "Alt"), (MOD_WIN, "Left Windows")] {
        if k.mods & bit != 0 {
            parts.push(n.to_string());
        }
    }
    parts.push(vk_name(k.vk));
    parts.join(" + ")
}

/// Modifier VKs in press order.
pub fn mod_vks(mods: u8) -> Vec<u16> {
    [(MOD_CTRL, 0xA2), (MOD_SHIFT, 0xA0), (MOD_ALT, 0xA4), (MOD_WIN, 0x5B)]
        .into_iter()
        .filter(|(b, _)| mods & b != 0)
        .map(|(_, vk)| vk)
        .collect()
}

#[derive(Clone, Debug)]
pub struct Card {
    /// Left column (white).
    pub name: String,
    /// Right column (grey), e.g. what a shortcut does.
    pub detail: String,
    pub assignment: Assignment,
}

#[derive(Clone, Debug)]
pub struct Section {
    pub title: String,
    /// G HUB category tag colour.
    pub tag: Option<[u8; 3]>,
    pub cards: Vec<Card>,
}

fn sc(name: &str, combo: &str, vk: u16, mods: u8) -> Card {
    Card {
        name: combo.into(),
        detail: name.into(),
        assignment: Assignment::Shortcut { name: name.into(), combo: KeyCombo { vk, mods } },
    }
}

/// COMMANDS tab: the Windows shortcut cards G HUB ships, by category.
pub fn commands() -> Vec<Section> {
    const W: u8 = MOD_WIN;
    const C: u8 = MOD_CTRL;
    const S: u8 = MOD_SHIFT;
    const A: u8 = MOD_ALT;
    vec![
        Section {
            title: "Windows".into(),
            tag: Some([0x43, 0xEE, 0xFF]),
            cards: vec![
                sc("Open Task View", "Left Windows + Tab", 0x09, W),
                sc("Open Quick Links", "Left Windows + X", 0x58, W),
                sc("Open Search", "Left Windows + S", 0x53, W),
                sc("Run dialog", "Left Windows + R", 0x52, W),
                sc("Previous Desktop", "Ctrl + Left Windows + Left", 0x25, C | W),
                sc("Open Ease of Access Center", "Left Windows + U", 0x55, W),
                sc("Open Magnifier", "Left Windows + =", 0xBB, W),
                sc("Next Desktop", "Ctrl + Left Windows + Right", 0x27, C | W),
                sc("Open Copilot", "Left Windows + Shift + F23", 0x86, W | S),
                sc("Lock PC", "Left Windows + L", 0x4C, W),
                sc("Hide/Show Desktop", "Left Windows + D", 0x44, W),
                sc("Open Action Center", "Left Windows + A", 0x41, W),
                sc("Cycle Task Bar Apps", "Left Windows + T", 0x54, W),
                sc("Set Focus in Notification Area", "Left Windows + B", 0x42, W),
                sc("Open Emoji Panel", "Left Windows + .", 0xBE, W),
                sc("Open File Explorer", "Left Windows + E", 0x45, W),
                sc("Open Windows Game Bar", "Left Windows + G", 0x47, W),
                sc("Open Windows Setting", "Left Windows + I", 0x49, W),
                sc("Open Connect Quick Action", "Left Windows + K", 0x4B, W),
                sc("Minimize All Windows", "Left Windows + M", 0x4D, W),
                sc("Open Narrator", "Left Windows + Ctrl + Enter", 0x0D, W | C),
                sc("Screenshot of Selected Area", "Left Windows + Shift + S", 0x53, W | S),
            ],
        },
        Section {
            title: "Editing".into(),
            tag: Some([0xFF, 0x43, 0xEE]),
            cards: vec![
                sc("Cut", "Ctrl + X", 0x58, C),
                sc("Copy", "Ctrl + C", 0x43, C),
                sc("Paste", "Ctrl + V", 0x56, C),
                sc("Undo", "Ctrl + Z", 0x5A, C),
                sc("Redo", "Ctrl + Y", 0x59, C),
                sc("Select All", "Ctrl + A", 0x41, C),
                sc("Save", "Ctrl + S", 0x53, C),
                sc("New", "Ctrl + N", 0x4E, C),
                sc("Open", "Ctrl + O", 0x4F, C),
                sc("New tab", "Ctrl + T", 0x54, C),
                sc("Close tab", "Ctrl + W", 0x57, C),
                sc("Next Tab", "Ctrl + Tab", 0x09, C),
                sc("Previous Tab", "Ctrl + Shift + Tab", 0x09, C | S),
                sc("Zoom In", "Ctrl + =", 0xBB, C),
                sc("Zoom Out", "Ctrl + -", 0xBD, C),
                sc("Zoom Reset", "Ctrl + 0", 0x30, C),
            ],
        },
        Section {
            title: "Productivity".into(),
            tag: Some([0xEE, 0xFF, 0x43]),
            cards: vec![
                sc("Switch Between Apps", "Alt + Tab", 0x09, A),
                sc("Cycle Through Apps", "Alt + Esc", 0x1B, A),
                sc("Open Start", "Ctrl + Esc", 0x1B, C),
                sc("Open Task Manager", "Ctrl + Shift + Esc", 0x1B, C | S),
                sc("Exit Active App", "Alt + F4", 0x73, A),
            ],
        },
        Section {
            title: "Navigation".into(),
            tag: Some([0xFF, 0x43, 0x90]),
            cards: vec![sc("Go Back", "Alt + Left", 0x25, A), sc("Go Forward", "Alt + Right", 0x27, A)],
        },
    ]
}

/// KEYS tab, in G HUB's order (punctuation, digits, letters, F-keys, the rest).
pub fn keys() -> Vec<u16> {
    let mut v: Vec<u16> = vec![0xC0, 0xBD, 0xBB, 0xDB, 0xDD, 0xDC, 0xBA, 0xDE, 0xBC, 0xBE, 0xBF];
    v.extend(0x30..=0x39);
    v.extend(0x41..=0x5A);
    v.extend(0x70..=0x87);
    v.extend([
        0x1B, 0x09, 0x14, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5, 0x5B, 0x5D, 0x20, 0x0D, 0x08, 0x2D, 0x2E, 0x24, 0x23,
        0x21, 0x22, 0x25, 0x26, 0x27, 0x28, 0x2C, 0x91, 0x13, 0x90,
    ]);
    v.extend(0x60..=0x69);
    v.extend([0x6A, 0x6B, 0x6D, 0x6E, 0x6F]);
    v
}

fn card(a: Assignment) -> Card {
    let (name, _) = label(&a, 0, &[]);
    Card { name, detail: String::new(), assignment: a }
}

/// SYSTEM tab (G HUB card_presets.json categories that apply to a mouse).
pub fn system() -> Vec<Section> {
    use Assignment::{Mouse, System};
    let key = |name: &str, vk: u16| Card {
        name: name.into(),
        detail: String::new(),
        assignment: Assignment::Shortcut { name: name.into(), combo: KeyCombo { vk, mods: 0 } },
    };
    vec![
        Section {
            title: "Mouse".into(),
            tag: None,
            cards: [
                Mouse(MouseFn::Primary),
                Mouse(MouseFn::Secondary),
                Mouse(MouseFn::Middle),
                Mouse(MouseFn::Back),
                Mouse(MouseFn::Forward),
                Mouse(MouseFn::DoubleClick),
                Mouse(MouseFn::WheelUp),
                Mouse(MouseFn::WheelDown),
                Mouse(MouseFn::WheelLeft),
                Mouse(MouseFn::WheelRight),
                System(SystemFn::DpiUp),
                System(SystemFn::DpiDown),
                System(SystemFn::DpiShift),
                System(SystemFn::DpiDefault),
                System(SystemFn::DpiCycle),
            ]
            .into_iter()
            .map(card)
            .collect(),
        },
        Section {
            title: "Device".into(),
            tag: None,
            cards: [
                System(SystemFn::GShift),
                System(SystemFn::ProfileCycle),
                System(SystemFn::OnboardProfileCycle),
                System(SystemFn::ReportRateCycle),
                Assignment::Disabled,
            ]
            .into_iter()
            .map(card)
            .collect(),
        },
        Section {
            title: "Media".into(),
            tag: None,
            cards: vec![
                key("Play/Pause", 0xB3),
                key("Stop", 0xB2),
                key("Previous Track", 0xB1),
                key("Next Track", 0xB0),
                key("Volume Up", 0xAF),
                key("Volume Down", 0xAE),
                key("Mute", 0xAD),
            ],
        },
    ]
}

/// ACTIONS tab: GHUB-X's Razer Basilisk–style functions.
pub fn actions() -> Vec<Section> {
    vec![Section {
        title: "Basilisk".into(),
        tag: Some([0x44, 0xD6, 0x2C]),
        cards: vec![
            Card {
                name: "Perfect 180".into(),
                detail: "Turn right".into(),
                assignment: Assignment::Perfect180 { left: false },
            },
            Card {
                name: "Perfect 180 (Left)".into(),
                detail: "Turn left".into(),
                assignment: Assignment::Perfect180 { left: true },
            },
            Card {
                name: "Sensitivity Clutch".into(),
                detail: "Hold for DPI Shift".into(),
                assignment: Assignment::System(SystemFn::DpiShift),
            },
            Card {
                name: "Scroll Acceleration".into(),
                detail: "Toggle on/off".into(),
                assignment: Assignment::ToggleScrollAccel,
            },
        ],
    }]
}
