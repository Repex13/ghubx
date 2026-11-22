//! Device Settings: the slot menu (DETAILS, DISABLE, RESTORE, REPLACE WITH), the confirmation bar
//! and the details panel. Nothing is written until WRITE TO MOUSE is pressed.

use super::theme::*;
use super::GhubApp;
use crate::engine::EngineMsg;
use crate::profile::{NOT_A_SLOT, OnboardSlot, button_diffs, encode_directory, encode_onboard, onboard_summary};
use egui::{Align2, CornerRadius, Rect, Stroke, Ui, pos2, vec2};

/// What the confirmation bar will do.
pub enum SlotJob {
    Write(Vec<(u16, [u8; 256])>),
    Restore(Vec<u16>),
}

pub struct SlotAction {
    pub text: String,
    pub job: SlotJob,
    pub saved: String,
}

pub struct SlotMenu {
    pub slot: usize,
    pub opened_frame: u64,
}

/// What a slot dropdown in the ASSIGN PROFILES overlay shows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Sel {
    /// Do not touch this slot.
    Unchanged,
    /// Write this (app, profile) pair's encoded sector to the slot.
    Pick(usize, usize),
}

/// The ASSIGN PROFILES overlay: one dropdown per on-board slot; confirming
/// produces a single `SlotAction` with every changed slot's sector.
pub struct SlotAssign {
    /// (app, profile) pair the slot already stores, matched by name.
    pub stored: [Option<(usize, usize)>; 5],
    /// The dropdown selection per slot.
    pub sel: [Sel; 5],
    /// Row whose dropdown list is open.
    pub open: Option<usize>,
    /// Scroll inside the open dropdown list.
    pub scroll: f32,
    /// First frame the overlay was seen (for `live()`).
    pub opened_frame: u64,
}

impl SlotAssign {
    /// Pre-fill each slot with the profile the read directory says lives there.
    pub fn new(onboard: &[OnboardSlot], cfg: &crate::profile::Config, opened_frame: u64) -> Self {
        let mut stored = [None; 5];
        let mut sel = [Sel::Unchanged; 5];
        for i in 0..5 {
            stored[i] = onboard.iter().find(|s| s.sector == i as u16 + 1).and_then(|s| s.profile.as_ref()).and_then(|p| match_slot_name(&p.name, cfg));
            if let Some(pair) = stored[i] {
                sel[i] = Sel::Pick(pair.0, pair.1);
            }
        }
        Self { stored, sel, open: None, scroll: 0.0, opened_frame }
    }
}

/// The (app, profile) pair an on-board profile name refers to: first the
/// combined `"<app>: <profile>"` GHUB-X writes, then a bare profile name —
/// but only when it is unique across all apps.
pub fn match_slot_name(name: &str, cfg: &crate::profile::Config) -> Option<(usize, usize)> {
    let name = name.trim();
    if name.is_empty() {
        return None;
    }
    for (ai, a) in cfg.apps.iter().enumerate() {
        for (pi, p) in a.profiles.iter().enumerate() {
            if format!("{}: {}", a.name, p.name).eq_ignore_ascii_case(name) {
                return Some((ai, pi));
            }
        }
    }
    let mut found = None;
    for (ai, a) in cfg.apps.iter().enumerate() {
        for (pi, p) in a.profiles.iter().enumerate() {
            if p.name.eq_ignore_ascii_case(name) {
                if found.is_some() {
                    return None; // ambiguous bare name: no match
                }
                found = Some((ai, pi));
            }
        }
    }
    found
}

/// Every "app: profile" pair in the config, in order.
fn all_pairs(cfg: &crate::profile::Config) -> Vec<(usize, usize, String)> {
    let mut out = Vec::new();
    for (ai, a) in cfg.apps.iter().enumerate() {
        for (pi, p) in a.profiles.iter().enumerate() {
            out.push((ai, pi, format!("{}: {}", a.name, p.name)));
        }
    }
    out
}

/// The write payload for the picks: one encoded sector per slot whose pick
/// differs from what the slot already stores, plus the not-storable notes
/// (labelled with the slot they came from).
pub fn assign_sectors(cfg: &crate::profile::Config, stored: &[Option<(usize, usize)>; 5], sel: &[Sel; 5]) -> (Vec<(u16, [u8; 256])>, Vec<String>) {
    let mut sectors = Vec::new();
    let mut notes: Vec<String> = Vec::new();
    for (slot, s) in sel.iter().enumerate() {
        let Sel::Pick(ai, pi) = *s else { continue };
        if stored[slot] == Some((ai, pi)) {
            continue; // the slot already stores exactly this profile
        }
        let a = &cfg.apps[ai];
        let p = &a.profiles[pi];
        let name = format!("{}: {}", a.name, p.name);
        let enc = encode_onboard(p, &name, &a.macros);
        for &(s, gshift, ref label) in &enc.not_stored {
            let g = if gshift { "G-shift " } else { "" };
            let t = if s == NOT_A_SLOT { format!("slot {}: {label}", slot + 1) } else { format!("slot {} {g}G{} {label}", slot + 1, s + 1) };
            if !notes.contains(&t) {
                notes.push(t);
            }
        }
        sectors.push((slot as u16 + 1, enc.sector));
    }
    (sectors, notes)
}

impl GhubApp {
    fn dir_flags(&self) -> [bool; 5] {
        let mut f = [true; 5];
        for (i, s) in self.onboard.iter().enumerate().take(5) {
            f[i] = s.enabled;
        }
        f
    }

    fn why_cannot_write(&self) -> Option<&'static str> {
        if self.status.ghub_running {
            Some("G HUB is running")
        } else if crate::engine::observer_mode() {
            Some("not available in this mode")
        } else if !self.dev.connected {
            Some("the mouse is not connected")
        } else {
            None
        }
    }

    /// The popup opened by the slot name.
    pub(crate) fn slot_menu_ui(&mut self, ui: &Ui, top_left: egui::Pos2) {
        let Some(menu) = &self.slot_menu else { return };
        let (slot, opened) = (menu.slot, menu.opened_frame);
        let cfg = self.cfg();
        let th = pal();
        let enabled_now = self.onboard.get(slot - 1).is_none_or(|s| s.enabled);
        let mut items: Vec<(String, bool, u8, usize, usize)> = vec![
            ("DETAILS".into(), false, 1, 0, 0),
            (if enabled_now { "DISABLE" } else { "ENABLE" }.into(), false, 2, 0, 0),
            ("RESTORE DEFAULT PROFILE".into(), false, 3, 0, 0),
            ("REPLACE WITH".into(), true, 0, 0, 0),
        ];
        for (ai, a) in cfg.apps.iter().enumerate() {
            items.push((a.name.to_uppercase(), true, 0, ai, usize::MAX));
            for (pi, p) in a.profiles.iter().enumerate() {
                items.push((p.name.clone(), false, 4, ai, pi));
            }
        }
        let row_h = 24.0;
        let w = 240.0;
        let h = (items.len() as f32 * row_h + 8.0).min(ui.ctx().content_rect().bottom() - top_left.y - 12.0);
        let rect = Rect::from_min_size(top_left, vec2(w, h));
        ui.painter().rect_filled(rect, CornerRadius::same(2), th.n(0x3a));
        let mut chosen: Option<(u8, usize, usize)> = None;
        for (i, (label, header, kind, ai, pi)) in items.iter().enumerate() {
            let rr = Rect::from_min_size(pos2(rect.left(), rect.top() + 4.0 + i as f32 * row_h), vec2(w, row_h));
            if rr.bottom() > rect.bottom() {
                break;
            }
            let is_profile = *kind == 4;
            let r = click(ui, rr, ("slot-menu", i));
            if r.hovered() && (*kind != 0) {
                ui.painter().rect_filled(rr, CornerRadius::ZERO, th.n(0x48));
            }
            let (x, color) = if is_profile {
                (rr.left() + 28.0, if r.hovered() { th.text } else { th.n(0x88) })
            } else if *header && *ai < usize::MAX && label == "REPLACE WITH" {
                (rr.left() + 8.0, th.label)
            } else if *header {
                (rr.left() + 8.0, th.text)
            } else {
                (rr.left() + 8.0, if r.hovered() { th.text } else { th.label })
            };
            let font = if *header && label != "REPLACE WITH" { Fam::Bold } else { Fam::Reg };
            text(ui, pos2(x, rr.center().y), Align2::LEFT_CENTER, label, Ts::new(font, 12.0, color).ls(0.6));
            if r.clicked() && *kind != 0 {
                chosen = Some((*kind, *ai, *pi));
            }
        }
        let area_clicked_outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p)));
        let frame = ui.ctx().cumulative_frame_nr();
        if let Some((kind, ai, pi)) = chosen {
            self.slot_menu = None;
            match kind {
                1 => self.slot_details = Some(slot),
                2 => {
                    let mut flags = self.dir_flags();
                    flags[slot - 1] = !flags[slot - 1];
                    let verb = if flags[slot - 1] { "Enable" } else { "Disable" };
                    self.slot_pending = Some(SlotAction {
                        text: format!("{verb} slot {slot}?"),
                        job: SlotJob::Write(vec![(0, encode_directory(flags))]),
                        saved: format!("Slot {slot} {}d and checked", verb.to_lowercase()),
                    });
                }
                3 => {
                    self.slot_pending = Some(SlotAction {
                        text: format!("Restore slot {slot} to the factory profile?"),
                        job: SlotJob::Restore(vec![slot as u16]),
                        saved: format!("Slot {slot} restored and checked"),
                    });
                }
                _ => self.replace_slot(slot, ai, pi),
            }
        } else if (area_clicked_outside && frame > opened) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.slot_menu = None;
        }
    }

    pub(crate) fn replace_slot(&mut self, slot: usize, ai: usize, pi: usize) {
        let cfg = self.cfg();
        let (Some(a), Some(p)) = (cfg.apps.get(ai), cfg.apps.get(ai).and_then(|a| a.profiles.get(pi))) else { return };
        let name = format!("{}: {}", a.name, p.name);
        let enc = encode_onboard(p, &name, &a.macros);
        let mut seen = Vec::new();
        for (s, _, label) in &enc.not_stored {
            let t = if *s == NOT_A_SLOT { label.clone() } else { format!("G{} {label}", s + 1) };
            if !seen.contains(&t) {
                seen.push(t);
            }
        }
        let mut text = format!("Replace slot {slot} with \"{name}\"?");
        if !seen.is_empty() {
            text += &format!(" Not stored on the mouse: {}.", seen.join(", "));
        }
        self.slot_pending = Some(SlotAction { text, job: SlotJob::Write(vec![(slot as u16, enc.sector)]), saved: format!("Saved to slot {slot} and checked") });
    }

    /// RESTORE ALL PROFILE SLOTS TO DEFAULT.
    pub(crate) fn restore_all_slots(&mut self) {
        self.slot_pending = Some(SlotAction { text: "Restore all five slots to the factory profiles?".into(), job: SlotJob::Restore((1..=5).collect()), saved: "All slots restored and checked".into() });
    }

    /// The bar across the bottom of the window with WRITE TO MOUSE / CANCEL.
    pub(crate) fn slot_bar(&mut self, ui: &Ui, win: Rect) {
        let Some(action) = &self.slot_pending else { return };
        let th = pal();
        let bar = Rect::from_min_max(pos2(win.left(), win.bottom() - 56.0), win.right_bottom());
        ui.painter().rect_filled(bar, CornerRadius::ZERO, th.primary);
        let why = self.why_cannot_write();
        let msg = match why {
            Some(w) => format!("{}  ({w}: nothing can be written)", action.text),
            None => action.text.clone(),
        };
        para(ui, pos2(bar.left() + 24.0, bar.top() + 10.0), bar.width() - 330.0, &msg, Ts::new(Fam::Reg, 12.0, th.on_primary));
        let write = Rect::from_min_size(pos2(bar.right() - 290.0, bar.top() + 12.0), vec2(150.0, 32.0));
        let cancel = Rect::from_min_size(pos2(bar.right() - 130.0, bar.top() + 12.0), vec2(110.0, 32.0));
        let go = dark_button(ui, write, "WRITE TO MOUSE", "slot-write", why.is_none()).clicked();
        let no = dark_button(ui, cancel, "CANCEL", "slot-cancel", true).clicked();
        if no {
            self.slot_pending = None;
        } else if go && why.is_none() {
            let Some(a) = self.slot_pending.take() else { return };
            self.slot_saved_msg = Some(a.saved);
            let _ = self.eng.send(match a.job {
                SlotJob::Write(sectors) => EngineMsg::WriteOnboard { sectors, active: None },
                SlotJob::Restore(slots) => EngineMsg::RestoreOnboard { slots },
            });
        }
    }

    /// DETAILS panel for a slot.
    pub(crate) fn slot_details_ui(&mut self, ui: &Ui, win: Rect) {
        let Some(slot) = self.slot_details else { return };
        let th = pal();
        let r = Rect::from_min_size(pos2(win.left() + 176.0, win.top() + 372.0), vec2(360.0, 230.0));
        ui.painter().rect_filled(r, CornerRadius::same(6), th.surface_hi);
        let s: Option<&OnboardSlot> = self.onboard.get(slot - 1);
        let (mut y, x) = (r.top() + 20.0, r.left() + 18.0);
        text(ui, pos2(x, y), Align2::LEFT_CENTER, &format!("SLOT {slot} DETAILS"), Ts::new(Fam::Bold, 13.0, th.text).ls(0.6));
        y += 28.0;
        match s.and_then(|s| s.profile.as_ref()) {
            None => {
                para(ui, pos2(x, y - 8.0), 320.0, "This slot could not be read.", Ts::new(Fam::Reg, 12.0, th.label));
            }
            Some(p) => {
                let zone = |l: &[u8; 11]| crate::profile::Lighting::from_led(l).map_or("unknown", |l| super::light::effect_name(l.effect));
                let dpi = p.dpi.get(p.active_dpi).map_or("?".into(), |d| d.to_string());
                for line in [
                    format!("Name: {}", if p.name.is_empty() { format!("PROFILE {slot}") } else { p.name.clone() }),
                    onboard_summary(p),
                    format!("Starts at {dpi} DPI"),
                    format!("Primary: {}   Logo: {}", zone(&p.leds[0]), zone(&p.leds[1])),
                ] {
                    text(ui, pos2(x, y), Align2::LEFT_CENTER, &line, Ts::new(Fam::Reg, 12.0, th.text));
                    y += 22.0;
                }
                let diffs = button_diffs(p);
                let t = if diffs.is_empty() { "Buttons: factory layout".to_string() } else { format!("Buttons that differ: {}", diffs.join(", ")) };
                para(ui, pos2(x, y - 8.0), 324.0, &t, Ts::new(Fam::Reg, 12.0, th.label));
            }
        }
        if dark_button(ui, Rect::from_min_size(pos2(r.right() - 90.0, r.bottom() - 40.0), vec2(72.0, 26.0)), "CLOSE", "slot-details-close", true).clicked() {
            self.slot_details = None;
        }
        let _ = Stroke::NONE;
    }

    /// Open the ASSIGN PROFILES overlay (pre-filled from the read directory).
    pub(crate) fn open_slot_assign(&mut self, opened_frame: u64) {
        let cfg = self.cfg();
        self.slot_assign = Some(SlotAssign::new(&self.onboard, &cfg, opened_frame));
    }

    /// The ASSIGN PROFILES overlay: five slots, one dropdown each. Confirming
    /// queues a single pending action; WRITE TO MOUSE still does the write.
    pub(crate) fn slot_assign_ui(&mut self, ui: &Ui, win: Rect) {
        let Some(mut sa) = self.slot_assign.take() else { return };
        let th = pal();
        let cfg = self.cfg();
        let pairs = all_pairs(&cfg);
        let mut keep = true;
        let r = Rect::from_min_size(pos2(win.left() + 596.0, win.top() + 344.0), vec2(360.0, 304.0));
        ui.painter().rect_filled(r, CornerRadius::same(6), th.n(0x3a));
        text(ui, pos2(r.left() + 18.0, r.top() + 24.0), Align2::LEFT_CENTER, "ASSIGN PROFILES TO SLOTS", Ts::new(Fam::Bold, 13.0, th.text).ls(0.6));
        // rows
        let row_h = 32.0;
        for i in 0..5 {
            let rr = Rect::from_min_size(pos2(r.left() + 10.0, r.top() + 46.0 + i as f32 * row_h), vec2(r.width() - 20.0, row_h));
            let resp = click(ui, rr, ("assign-row", i));
            if resp.hovered() || sa.open == Some(i) {
                ui.painter().rect_filled(rr, CornerRadius::ZERO, th.n(0x48));
            }
            text(ui, pos2(rr.left() + 10.0, rr.center().y), Align2::LEFT_CENTER, &format!("SLOT {}", i + 1), Ts::new(Fam::Bold, 12.0, th.text).ls(0.6));
            let (label, changed) = match sa.sel[i] {
                Sel::Unchanged => ("(unchanged)".to_owned(), false),
                Sel::Pick(ai, pi) => {
                    let l = pairs.iter().find(|(a, p, _)| (*a, *p) == (ai, pi)).map(|(_, _, l)| l.clone()).unwrap_or_else(|| "?".to_owned());
                    (l, sa.stored[i] != Some((ai, pi)))
                }
            };
            let color = match sa.sel[i] {
                Sel::Unchanged => th.muted,
                _ if changed => th.primary,
                _ => th.text,
            };
            let g = galley(ui, &label, Ts::new(Fam::Reg, 12.0, color).ls(0.4), 190.0);
            ui.painter().galley(pos2(rr.right() - 24.0 - g.size().x, rr.center().y - g.size().y / 2.0), g, color);
            chevron(ui.painter(), pos2(rr.right() - 12.0, rr.center().y), 4.0, sa.open != Some(i), Stroke::new(1.4, th.muted));
            if resp.clicked() {
                sa.open = if sa.open == Some(i) { None } else { Some(i) };
                sa.scroll = 0.0;
            }
        }
        // the open row's dropdown: drawn over the following rows, clipped
        if let Some(oi) = sa.open {
            let row_r = Rect::from_min_size(pos2(r.left() + 10.0, r.top() + 46.0 + oi as f32 * row_h), vec2(r.width() - 20.0, row_h));
            let mut opts: Vec<(String, Sel)> = vec![("(unchanged)".into(), Sel::Unchanged)];
            for (ai, pi, l) in &pairs {
                opts.push((l.clone(), Sel::Pick(*ai, *pi)));
            }
            let opt_h = 24.0;
            let list_h = (opts.len() as f32 * opt_h + 6.0).min(168.0);
            let lr = Rect::from_min_size(pos2(row_r.left() + 110.0, (row_r.bottom() + 2.0).min(r.bottom() - 64.0 - list_h)), vec2(224.0, list_h));
            ui.painter().rect_filled(lr, CornerRadius::same(2), th.n(0x2c));
            if ui.input(|i| i.pointer.interact_pos().is_some_and(|p| lr.contains(p))) {
                sa.scroll = (sa.scroll - ui.input(|i| i.smooth_scroll_delta.y)).clamp(0.0, (opts.len() as f32 * opt_h + 6.0 - list_h).max(0.0));
            }
            let paint = ui.painter().with_clip_rect(lr);
            let mut picked: Option<(usize, Sel)> = None;
            for (j, (label, s)) in opts.iter().enumerate() {
                let or = Rect::from_min_size(pos2(lr.left(), lr.top() + 3.0 + j as f32 * opt_h - sa.scroll), vec2(lr.width(), opt_h));
                if or.bottom() < lr.top() || or.top() > lr.bottom() {
                    continue;
                }
                let resp = click(ui, or, ("assign-opt", j));
                if resp.hovered() {
                    paint.rect_filled(or, CornerRadius::ZERO, th.n(0x48));
                }
                let on = *s == sa.sel[oi];
                paint.text(pos2(or.left() + 10.0, or.center().y), Align2::LEFT_CENTER, label, Ts::new(Fam::Reg, 12.0, if on { th.primary } else { th.text }).font(), if on { th.primary } else { th.text });
                if resp.clicked() {
                    picked = Some((oi, *s));
                }
            }
            if let Some((i, s)) = picked {
                sa.sel[i] = s;
                sa.open = None;
            }
        }
        // footer buttons
        let (sectors, notes) = assign_sectors(&cfg, &sa.stored, &sa.sel);
        let n = sectors.len();
        let br = Rect::from_min_size(pos2(r.left() + 14.0, r.bottom() - 44.0), vec2(190.0, 30.0));
        if n == 0 {
            ui.painter().rect_filled(br, CornerRadius::ZERO, th.primary_disabled);
            text(ui, br.center(), Align2::CENTER_CENTER, "SAVE 0 PROFILES", Ts::new(Fam::Reg, 12.0, th.text).ls(1.2));
        } else if blue_button(ui, br, &format!("SAVE {n} PROFILE{}", if n == 1 { "" } else { "S" }), "assign-apply").clicked() {
            let mut text = format!("Save {n} profile{} to the mouse?", if n == 1 { "" } else { "s" });
            if !notes.is_empty() {
                text += &format!(" Not stored on the mouse: {}.", notes.join(", "));
            }
            self.slot_pending = Some(SlotAction { text, job: SlotJob::Write(sectors), saved: format!("{n} profile(s) saved and checked") });
            keep = false;
        }
        if dark_button(ui, Rect::from_min_size(pos2(r.right() - 104.0, r.bottom() - 44.0), vec2(90.0, 30.0)), "CANCEL", "assign-cancel", true).clicked() {
            keep = false;
        }
        // dismiss on outside click (after the opening frame) or Escape
        let outside = ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !r.contains(p)));
        if (outside && ui.ctx().cumulative_frame_nr() > sa.opened_frame) || ui.input(|i| i.key_pressed(egui::Key::Escape)) {
            keep = false;
        }
        if keep {
            self.slot_assign = Some(sa);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::profile::{AppEntry, Config, Profile};

    fn cfg_two_apps() -> Config {
        let mut c = Config::default();
        c.apps[0].name = "Desktop".into();
        c.apps[0].profiles[0].name = "Default".into();
        let mut app = AppEntry::desktop();
        app.name = "Game".into();
        app.profiles = vec![Profile::new("Default"), Profile::new("FPS")];
        c.apps.push(app);
        c
    }

    #[test]
    fn match_slot_name_finds_combined_and_unique_bare_names() {
        let c = cfg_two_apps();
        // GHUB-X writes "App: Profile" — exact (and case-insensitive) match
        assert_eq!(match_slot_name("Desktop: Default", &c), Some((0, 0)));
        assert_eq!(match_slot_name("game: fps", &c), Some((1, 1)));
        // bare names only match when unique across apps
        assert_eq!(match_slot_name("FPS", &c), Some((1, 1)));
        assert_eq!(match_slot_name("Default", &c), None, "Default exists in both apps");
        assert_eq!(match_slot_name("", &c), None);
        assert_eq!(match_slot_name("Nope", &c), None);
    }

    #[test]
    fn assign_sectors_writes_only_changed_slots() {
        let c = cfg_two_apps();
        // slot 1 already stores Desktop: Default, slot 3 stores Game: FPS
        let stored = [Some((0, 0)), None, Some((1, 1)), None, None];
        // pick the SAME profile for slot 1 -> skipped; Unchanged slots -> skipped;
        // Game: FPS for slot 2 -> written even though slot 3 already has it
        let sel = [Sel::Pick(0, 0), Sel::Pick(1, 1), Sel::Unchanged, Sel::Pick(1, 1), Sel::Unchanged];
        let (sectors, notes) = assign_sectors(&c, &stored, &sel);
        assert_eq!(sectors.iter().map(|s| s.0).collect::<Vec<_>>(), vec![2, 4], "only the changed picks");
        assert!(notes.is_empty(), "plain profiles have nothing un-storable: {notes:?}");
    }

    #[test]
    fn assign_sectors_reports_not_stored_per_slot() {
        let mut c = cfg_two_apps();
        c.apps[0].profiles[0].script = "function OnEvent() end".into();
        c.apps[0].macros.push(crate::profile::Macro { id: "m".into(), name: "m".into(), ..Default::default() });
        // a macro on a button is not storable either — reported with its G-slot
        c.apps[0].profiles[0].base[2] = crate::profile::Assignment::Macro("m".into());
        let stored = [None; 5];
        let sel = [Sel::Pick(0, 0), Sel::Pick(0, 0), Sel::Unchanged, Sel::Unchanged, Sel::Unchanged];
        let (sectors, notes) = assign_sectors(&c, &stored, &sel);
        assert_eq!(sectors.iter().map(|s| s.0).collect::<Vec<_>>(), vec![1, 2]);
        // the script + the macro are not storable, tagged with their slot
        assert!(notes.iter().any(|t| t.contains("slot 1: Lua script")), "{notes:?}");
        assert!(notes.iter().any(|t| t.contains("slot 2: Lua script")), "{notes:?}");
        assert!(notes.iter().any(|t| t.contains("slot 1 G3") && t.contains("macro")), "{notes:?}");
    }

    #[test]
    fn slot_assign_prefills_from_the_directory() {
        let c = cfg_two_apps();
        let mk = |sector: u16, name: &str| OnboardSlot {
            sector,
            enabled: true,
            profile: crate::profile::OnboardProfile::parse(&crate::profile::sample_sector1()).map(|mut p| {
                p.name = name.into();
                p
            }),
        };
        let dir = vec![mk(1, "Desktop: Default"), mk(2, "FPS"), mk(3, "Default"), mk(4, "whatever"), mk(5, "Game: FPS")];
        let sa = SlotAssign::new(&dir, &c, 0);
        assert_eq!(sa.stored[0], Some((0, 0)));
        assert_eq!(sa.stored[1], Some((1, 1)));
        assert_eq!(sa.stored[2], None, "ambiguous bare name stays unmatched");
        assert_eq!(sa.stored[3], None);
        assert_eq!(sa.stored[4], Some((1, 1)));
        // matched slots pre-select what they already hold; unmatched stay (unchanged)
        assert_eq!(sa.sel[0], Sel::Pick(0, 0));
        assert_eq!(sa.sel[1], Sel::Pick(1, 1));
        assert_eq!(sa.sel[2], Sel::Unchanged);
        assert_eq!(sa.sel[3], Sel::Unchanged);
        assert_eq!(sa.sel[4], Sel::Pick(1, 1));
    }
}
