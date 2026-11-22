//! Import an existing G HUB setup (`%LOCALAPPDATA%\LGHUB\settings.db`):
//! profiles, G502 HERO slot assignments (both layers), macros, keystroke
//! shortcuts, the DPI table, report rate and LIGHTSYNC zones.
//!
//! The database (and its WAL) is copied to a temp folder first, so G HUB's
//! live files are only ever read.

use crate::device::SLOTS;
use crate::profile::*;
use serde_json::Value;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug, Default, Clone)]
pub struct Summary {
    pub profiles: usize,
    pub assignments: usize,
    pub macros: usize,
    pub skipped: usize,
}

pub fn settings_db() -> Option<PathBuf> {
    let p = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join("LGHUB").join("settings.db");
    p.exists().then_some(p)
}

/// Read G HUB's settings JSON blob from a private copy of the database.
pub fn read_settings_json() -> Result<Value, String> {
    let src = settings_db().ok_or("G HUB settings not found")?;
    let dir = std::env::temp_dir().join(format!("ghubx-import-{}", std::process::id()));
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let db = dir.join("settings.db");
    std::fs::copy(&src, &db).map_err(|e| format!("copy settings.db: {e}"))?;
    for ext in ["db-wal", "db-shm"] {
        let s = src.with_extension(ext);
        if s.exists() {
            let _ = std::fs::copy(&s, db.with_extension(ext));
        }
    }
    let blob: Vec<u8> = {
        let conn = rusqlite::Connection::open(&db).map_err(|e| e.to_string())?;
        conn.query_row("SELECT file FROM data ORDER BY _id DESC LIMIT 1", [], |r| r.get(0)).map_err(|e| e.to_string())?
    };
    let _ = std::fs::remove_dir_all(&dir);
    serde_json::from_slice(&blob).map_err(|e| format!("settings JSON: {e}"))
}

/// USB HID keyboard usage (page 0x07) -> Windows virtual key.
pub fn hid_to_vk(u: u32) -> Option<u16> {
    Some(match u {
        0x04..=0x1D => 0x41 + (u - 0x04) as u16,
        0x1E..=0x26 => 0x31 + (u - 0x1E) as u16,
        0x27 => 0x30,
        0x28 => 0x0D,
        0x29 => 0x1B,
        0x2A => 0x08,
        0x2B => 0x09,
        0x2C => 0x20,
        0x2D => 0xBD,
        0x2E => 0xBB,
        0x2F => 0xDB,
        0x30 => 0xDD,
        0x31 => 0xDC,
        0x33 => 0xBA,
        0x34 => 0xDE,
        0x35 => 0xC0,
        0x36 => 0xBC,
        0x37 => 0xBE,
        0x38 => 0xBF,
        0x39 => 0x14,
        0x3A..=0x45 => 0x70 + (u - 0x3A) as u16,
        0x46 => 0x2C,
        0x47 => 0x91,
        0x48 => 0x13,
        0x49 => 0x2D,
        0x4A => 0x24,
        0x4B => 0x21,
        0x4C => 0x2E,
        0x4D => 0x23,
        0x4E => 0x22,
        0x4F => 0x27,
        0x50 => 0x25,
        0x51 => 0x28,
        0x52 => 0x26,
        0x53 => 0x90,
        0x54 => 0x6F,
        0x55 => 0x6A,
        0x56 => 0x6D,
        0x57 => 0x6B,
        0x58 => 0x0D,
        0x59..=0x61 => 0x61 + (u - 0x59) as u16,
        0x62 => 0x60,
        0x63 => 0x6E,
        0x65 => 0x5D,
        0x68..=0x73 => 0x7C + (u - 0x68) as u16,
        0xE0 => 0xA2,
        0xE1 => 0xA0,
        0xE2 => 0xA4,
        0xE3 => 0x5B,
        0xE4 => 0xA3,
        0xE5 => 0xA1,
        0xE6 => 0xA5,
        0xE7 => 0x5C,
        _ => return None,
    })
}

fn mod_bit(u: u64) -> u8 {
    match u {
        224 | 228 => MOD_CTRL,
        225 | 229 => MOD_SHIFT,
        226 | 230 => MOD_ALT,
        227 | 231 => MOD_WIN,
        _ => 0,
    }
}

fn num(v: &Value) -> Option<u64> {
    v.as_u64().or_else(|| v.as_str().and_then(|s| s.parse().ok()))
}

/// G HUB's built-in cards: `0f82f693-5b78-4cf5-867e-CCNN00000000`.
fn builtin(id: &str, slot: usize) -> Option<Assignment> {
    let tail = id.strip_prefix("0f82f693-5b78-4cf5-867e-")?;
    let cat = u8::from_str_radix(tail.get(0..2)?, 16).ok()?;
    let code = u8::from_str_radix(tail.get(2..4)?, 16).ok()?;
    let a = match (cat, code) {
        (0x01, k) => Assignment::Key(KeyCombo { vk: hid_to_vk(k as u32)?, mods: 0 }),
        (0x02, 1) => Assignment::Mouse(MouseFn::Primary),
        (0x02, 2) => Assignment::Mouse(MouseFn::Secondary),
        (0x02, 3) => Assignment::Mouse(MouseFn::Middle),
        (0x02, 4) => Assignment::Mouse(MouseFn::Back),
        (0x02, 5) => Assignment::Mouse(MouseFn::Forward),
        (0x04, 0x01) => Assignment::System(SystemFn::DpiUp),
        (0x04, 0x02) => Assignment::System(SystemFn::DpiDown),
        (0x04, 0x0B) => Assignment::Mouse(MouseFn::WheelLeft),
        (0x04, 0x0C) => Assignment::Mouse(MouseFn::WheelRight),
        (0x09, 0x01) => Assignment::System(SystemFn::OnboardProfileCycle),
        // "same as the default layer" in the G-Shift layer
        (0x09, 0x07) => Assignment::Default,
        _ => return None,
    };
    Some(if a == native(slot) { Assignment::Default } else { a })
}

fn keystroke(m: &Value) -> Option<KeyCombo> {
    let ks = m.get("keystroke")?;
    let vk = hid_to_vk(num(ks.get("code")?)? as u32)?;
    let mods = ks.get("modifiers").and_then(|x| x.as_array()).map(|a| a.iter().filter_map(num).map(mod_bit).fold(0, |a, b| a | b)).unwrap_or(0);
    Some(KeyCombo { vk, mods })
}

/// G HUB SEQUENCE macro -> GHUB-X macro.
fn sequence(card: &Value) -> Option<Macro> {
    let seq = card.get("macro")?.get("sequence")?;
    let default_delay = seq.get("defaultDelay").and_then(num).unwrap_or(50) as u32;
    let pick = |k: &str| seq.get(k).and_then(|s| s.get("components")).and_then(|c| c.as_array()).filter(|a| !a.is_empty());
    let (kind, comps) = pick("heldSequence")
        .map(|c| (MacroKind::RepeatWhileHolding, c))
        .or_else(|| pick("toggleSequence").map(|c| (MacroKind::Toggle, c)))
        .or_else(|| pick("simpleSequence").or_else(|| pick("pressSequence")).map(|c| (MacroKind::NoRepeat, c)))?;
    let mut steps = Vec::new();
    let mut pending = 0u32;
    for c in comps {
        if let Some(d) = c.get("delay") {
            pending += d.get("durationMs").or_else(|| d.get("duration")).and_then(num).map(|v| v as u32).unwrap_or(default_delay);
            continue;
        }
        let (vk, down) = if let Some(k) = c.get("keyboard") {
            (hid_to_vk(num(k.get("hidUsage")?)? as u32)?, k.get("isDown").and_then(|b| b.as_bool()).unwrap_or(false))
        } else if let Some(b) = c.get("mouse").and_then(|m| m.get("button")) {
            let vk = match num(b.get("hidUsage")?)? {
                1 => 0x01,
                2 => 0x02,
                3 => 0x04,
                4 => 0x05,
                5 => 0x06,
                _ => continue,
            };
            (vk, b.get("isDown").and_then(|b| b.as_bool()).unwrap_or(false))
        } else {
            continue;
        };
        steps.push(MacroStep { vk, down, delay_ms: pending });
        pending = 0;
    }
    if pending > 0 {
        // trailing gap matters for repeating macros; vk 0 = delay only
        steps.push(MacroStep { vk: 0, down: false, delay_ms: pending });
    }
    Some(Macro { id: card.get("id")?.as_str()?.to_string(), name: card.get("name")?.as_str()?.to_string(), kind, steps })
}

fn lighting(card: &Value) -> Option<[Lighting; 2]> {
    let effects = card.get("firmwareLightingSettings")?.get("effects")?.as_array()?;
    let mut out = [Lighting::default(); 2];
    for e in effects {
        let zone = match e.get("zoneType").and_then(|z| z.as_str()) {
            Some("ZONE_PRIMARY") => 0,
            Some("ZONE_BRANDING") | Some("ZONE_LOGO") => 1,
            _ => continue,
        };
        let id = e.get("id").and_then(|i| i.as_str()).unwrap_or("");
        let params = ["cycleParams", "breathingParams", "fixedParams", "staticParams", "colorParams"].iter().find_map(|k| e.get(*k));
        let mut l = Lighting { effect: Effect::Off, ..Default::default() };
        l.effect = match id {
            "CYCLE" => Effect::Cycle,
            "BREATHING" => Effect::Breathing,
            "FIXED" | "STATIC" | "SOLID" => Effect::Fixed,
            _ => Effect::Off,
        };
        if let Some(p) = params {
            if let Some(ms) = p.get("periodInMs").and_then(num) {
                l.period_ms = ms as u16;
            }
            if let Some(i) = p.get("intensity").and_then(|v| v.as_f64()) {
                l.brightness = if i <= 1.0 { (i * 100.0).round() } else { i.round() }.clamp(1.0, 100.0) as u8;
            }
            if let Some(c) = p.get("color") {
                let ch = |k: &str| c.get(k).and_then(num).map(|v| v as u8);
                if let (Some(r), Some(g), Some(b)) = (ch("red").or(ch("r")), ch("green").or(ch("g")), ch("blue").or(ch("b"))) {
                    l.color = [r, g, b];
                }
            }
        }
        out[zone] = l;
    }
    Some(out)
}

/// Build GHUB-X applications from G HUB's settings JSON.
pub fn convert(d: &Value) -> Result<(Vec<AppEntry>, bool, Summary), String> {
    let cards: HashMap<&str, &Value> = d
        .pointer("/cards/cards")
        .and_then(|c| c.as_array())
        .ok_or("no cards in G HUB settings")?
        .iter()
        .filter_map(|c| Some((c.get("id")?.as_str()?, c)))
        .collect();
    let app_names: HashMap<&str, String> = d
        .pointer("/applications/applications")
        .and_then(|a| a.as_array())
        .map(|a| {
            a.iter()
                .filter_map(|x| {
                    let id = x.get("applicationId")?.as_str()?;
                    let n = x.get("name")?.as_str()?;
                    Some((id, if n == "APPLICATION_NAME_DESKTOP" { "Desktop".to_string() } else { n.to_string() }))
                })
                .collect()
        })
        .unwrap_or_default();
    let mut sum = Summary::default();
    let mut apps: Vec<(String, AppEntry)> = Vec::new();
    let profiles = d.pointer("/profiles/profiles").and_then(|p| p.as_array()).ok_or("no profiles in G HUB settings")?;
    for p in profiles {
        let app_id = p.get("applicationId").and_then(|x| x.as_str()).unwrap_or("").to_string();
        let mut prof = Profile::new(match p.get("name").and_then(|n| n.as_str()) {
            Some("PROFILE_NAME_DEFAULT") | None => "Default",
            Some(n) => n,
        });
        let mut macros = Vec::new();
        for a in p.get("assignments").and_then(|x| x.as_array()).into_iter().flatten() {
            let (Some(slot_id), Some(card_id)) = (a.get("slotId").and_then(|s| s.as_str()), a.get("cardId").and_then(|s| s.as_str())) else { continue };
            let card = cards.get(card_id).copied();
            if slot_id.ends_with("_mouse_settings") {
                if let Some(ms) = card.and_then(|c| c.get("mouseSettings")) {
                    if let Some(t) = ms.get("dpiTable") {
                        let levels: Vec<u16> = t.get("levels").and_then(|l| l.as_array()).map(|l| l.iter().filter_map(num).map(|v| v as u16).collect()).unwrap_or_default();
                        if !levels.is_empty() {
                            let g = |k: &str| t.get(k).and_then(num).map(|v| v as u16);
                            prof.dpi = DpiTable { active: g("activeDpi").unwrap_or(levels[0]), default: g("defaultDpi").unwrap_or(levels[0]), shift: g("shiftDpi").unwrap_or(levels[0]), levels };
                        }
                    }
                    if let Some(hz) = ms.pointer("/reportRate/value").and_then(num).filter(|&h| h > 0) {
                        prof.rate_ms = (1000 / hz).clamp(1, 8) as u8;
                    }
                }
                continue;
            }
            if slot_id.contains("lighting_setting") {
                if let Some(l) = card.and_then(lighting) {
                    prof.lighting = l;
                }
                continue;
            }
            // g502hero_g7_m1[_shifted]
            let Some(g) = slot_id.split('_').find(|s| s.len() > 1 && s.starts_with('g') && s[1..].chars().all(|c| c.is_ascii_digit())) else { continue };
            let Some(slot) = g[1..].parse::<usize>().ok().and_then(|n| n.checked_sub(1)).filter(|&s| s < SLOTS) else { continue };
            let shifted = slot_id.ends_with("_shifted");
            let asg = if let Some(b) = builtin(card_id, slot) {
                Some(b)
            } else if let Some(c) = card {
                let m = c.get("macro");
                match m.and_then(|m| m.get("type")).and_then(|t| t.as_str()) {
                    Some("KEYSTROKE") => keystroke(m.unwrap()).map(|combo| {
                        let name = c.get("name").and_then(|n| n.as_str()).unwrap_or("Keystroke").to_string();
                        Assignment::Shortcut { name, combo }
                    }),
                    Some("SEQUENCE") => sequence(c).map(|mac| {
                        let id = mac.id.clone();
                        macros.push(mac);
                        Assignment::Macro(id)
                    }),
                    _ => None,
                }
            } else {
                None
            };
            match asg {
                Some(a) => {
                    if a != Assignment::Default {
                        sum.assignments += 1;
                    }
                    prof.layer_mut(shifted)[slot] = a;
                }
                None => sum.skipped += 1,
            }
        }
        let active = p.get("activeForApplication").and_then(|b| b.as_bool()).unwrap_or(false);
        let entry = match apps.iter_mut().find(|(id, _)| *id == app_id) {
            Some((_, e)) => e,
            None => {
                let name = app_names.get(app_id.as_str()).cloned().unwrap_or_else(|| "Desktop".into());
                apps.push((app_id.clone(), AppEntry { name, exe: None, enabled: true, profiles: vec![], active_profile: 0, macros: vec![] }));
                &mut apps.last_mut().unwrap().1
            }
        };
        if active {
            entry.active_profile = entry.profiles.len();
        }
        entry.profiles.push(prof);
        for m in macros {
            if !entry.macros.iter().any(|x| x.id == m.id) {
                entry.macros.push(m);
            }
        }
        sum.profiles += 1;
    }
    // every SEQUENCE macro of an app, assigned or not (G HUB lists them all)
    for c in cards.values() {
        if c.pointer("/macro/type").and_then(|t| t.as_str()) == Some("SEQUENCE")
            && let Some(app_id) = c.get("applicationId").and_then(|a| a.as_str())
            && let Some((_, e)) = apps.iter_mut().find(|(id, _)| id == app_id)
            && let Some(m) = sequence(c)
            && !e.macros.iter().any(|x| x.id == m.id)
        {
            e.macros.push(m);
        }
    }
    let mut out: Vec<AppEntry> = apps.into_iter().map(|(_, e)| e).collect();
    out.sort_by_key(|a| a.name != "Desktop");
    for a in &mut out {
        a.macros.sort_by_key(|m| m.name.to_lowercase());
    }
    sum.macros = out.iter().map(|a| a.macros.len()).sum();
    let switching = d.get("profiles/auto_switching/enabled").and_then(|b| b.as_bool()).unwrap_or(false);
    Ok((out, switching, sum))
}

/// Import into `cfg`, replacing its applications. Settings and extras are kept.
pub fn import(cfg: &mut Config) -> Result<Summary, String> {
    let d = read_settings_json()?;
    let (apps, switching, sum) = convert(&d)?;
    if apps.is_empty() {
        return Err("G HUB has no profiles to import".into());
    }
    cfg.apps = apps;
    cfg.current_app = 0;
    cfg.persistent = None;
    cfg.profile_switching = switching;
    cfg.sanitize();
    Ok(sum)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Trimmed from the user's real G HUB settings (2026.7).
    const SAMPLE: &str = r#"{
      "applications": {"applications": [{"applicationId": "app1", "name": "APPLICATION_NAME_DESKTOP"}]},
      "profiles/auto_switching/enabled": false,
      "cards": {"cards": [
        {"id": "jump-id", "applicationId": "app1", "name": "jump", "attribute": "MACRO_PLAYBACK",
         "macro": {"type": "SEQUENCE", "sequence": {"defaultDelay": 50,
           "heldSequence": {"components": [{"keyboard": {"displayName": "Space", "hidUsage": "44", "isDown": true}}, {"delay": {}},
                                           {"keyboard": {"displayName": "Space", "hidUsage": "44"}}, {"delay": {}}]},
           "pressSequence": {}, "releaseSequence": {}, "simpleSequence": {}, "toggleSequence": {}}}},
        {"id": "ping-id", "applicationId": "app1", "name": "ping", "attribute": "MACRO_PLAYBACK",
         "macro": {"type": "SEQUENCE", "sequence": {"defaultDelay": 50, "heldSequence": {},
           "toggleSequence": {"components": [{"mouse": {"button": {"hidUsage": "3", "isDown": true}}}, {"delay": {}}, {"mouse": {"button": {"hidUsage": "3"}}}, {"delay": {}}]}}}},
        {"id": "task-id", "applicationId": "app1", "name": "Open Task Manager", "attribute": "MACRO_PLAYBACK",
         "macro": {"type": "KEYSTROKE", "keystroke": {"code": 41, "modifiers": [224, 225]}}},
        {"id": "ms-id", "attribute": "MOUSE_SETTINGS", "mouseSettings": {"dpiTable": {"activeDpi": 600, "defaultDpi": 1600, "levels": [600, 1000, 1600], "shiftDpi": 600}, "reportRate": {"value": 1000}}},
        {"id": "li-id", "attribute": "FIRMWARE_LIGHTING_SETTINGS", "firmwareLightingSettings": {"effects": [
          {"cycleParams": {"intensity": 1, "periodInMs": 8000}, "id": "CYCLE", "zoneType": "ZONE_BRANDING"},
          {"cycleParams": {"intensity": 1, "periodInMs": 8000}, "id": "CYCLE", "zoneType": "ZONE_PRIMARY"}]}}
      ]},
      "profiles": {"profiles": [{"activeForApplication": true, "applicationId": "app1", "id": "p1", "name": "PROFILE_NAME_DEFAULT", "assignments": [
        {"cardId": "0f82f693-5b78-4cf5-867e-020100000000", "slotId": "g502hero_g1_m1"},
        {"cardId": "jump-id", "slotId": "g502hero_g4_m1"},
        {"cardId": "0f82f693-5b78-4cf5-867e-011600000000", "slotId": "g502hero_g6_m1"},
        {"cardId": "0f82f693-5b78-4cf5-867e-040100000000", "slotId": "g502hero_g8_m1"},
        {"cardId": "0f82f693-5b78-4cf5-867e-040c00000000", "slotId": "g502hero_g10_m1"},
        {"cardId": "task-id", "slotId": "g502hero_g2_m1_shifted"},
        {"cardId": "0f82f693-5b78-4cf5-867e-090700000000", "slotId": "g502hero_g1_m1_shifted"},
        {"cardId": "ms-id", "slotId": "g502hero_mouse_settings"},
        {"cardId": "li-id", "slotId": "g502hero_lighting_setting_firmware"}
      ]}]}
    }"#;

    #[test]
    fn converts_real_ghub_layout() {
        let d: Value = serde_json::from_str(SAMPLE).unwrap();
        let (apps, switching, sum) = convert(&d).unwrap();
        assert!(!switching);
        assert_eq!(apps.len(), 1);
        let a = &apps[0];
        assert_eq!(a.name, "Desktop");
        let p = &a.profiles[0];
        assert_eq!(p.name, "Default");
        assert_eq!(p.dpi, DpiTable { levels: vec![600, 1000, 1600], active: 600, default: 1600, shift: 600 });
        assert_eq!(p.rate_ms, 1);
        assert_eq!(p.base[0], Assignment::Default); // primary click is native
        assert_eq!(p.base[3], Assignment::Macro("jump-id".into()));
        assert_eq!(p.base[5], Assignment::Key(KeyCombo { vk: 0x53, mods: 0 })); // "S"
        assert_eq!(p.base[7], Assignment::Default); // DPI Up is G8's native function
        assert_eq!(p.base[9], Assignment::Default); // Scroll Right is G10's native function
        assert_eq!(p.gshift[1], Assignment::Shortcut { name: "Open Task Manager".into(), combo: KeyCombo { vk: 0x1B, mods: MOD_CTRL | MOD_SHIFT } });
        assert_eq!(p.lighting[0].effect, Effect::Cycle);
        assert_eq!((p.lighting[1].period_ms, p.lighting[1].brightness), (8000, 100));
        let jump = a.macros.iter().find(|m| m.name == "jump").unwrap();
        assert_eq!(jump.kind, MacroKind::RepeatWhileHolding);
        assert_eq!(jump.steps, vec![
            MacroStep { vk: 0x20, down: true, delay_ms: 0 },
            MacroStep { vk: 0x20, down: false, delay_ms: 50 },
            MacroStep { vk: 0, down: false, delay_ms: 50 },
        ]);
        let ping = a.macros.iter().find(|m| m.name == "ping").unwrap();
        assert_eq!((ping.kind, ping.steps[0].vk), (MacroKind::Toggle, 0x04));
        assert_eq!(sum.macros, 2);
    }
}
