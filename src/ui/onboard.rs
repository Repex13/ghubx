//! Device Settings: the slot menu (DETAILS, DISABLE, RESTORE, REPLACE WITH), the confirmation bar
//! and the details panel. Nothing is written until WRITE TO MOUSE is pressed.

use super::theme::*;
use super::GhubApp;
use crate::engine::EngineMsg;
use crate::profile::{OnboardSlot, button_diffs, encode_directory, encode_onboard, onboard_summary};
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
            let t = format!("G{} {label}", s + 1);
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
}
