//! Sensitivity (DPI): G HUB's DPI SPEEDS axis with draggable levels,
//! DPI-shift diamond, inline editing and report rate.

use super::theme::*;
use super::{GhubApp, Overlay};
use crate::device::RATES_MS;
use crate::profile::*;
use egui::{Align2, Rect, Sense, Stroke, Ui, pos2, vec2};

/// G HUB's axis: equal-width segments between these values.
const STOPS: [f32; 7] = [100.0, 1000.0, 2000.0, 3000.0, 4000.0, 14800.0, 25600.0];

fn dpi_to_t(d: f32) -> f32 {
    let d = d.clamp(STOPS[0], STOPS[6]);
    let i = STOPS.windows(2).position(|w| d <= w[1]).unwrap_or(5);
    (i as f32 + (d - STOPS[i]) / (STOPS[i + 1] - STOPS[i])) / 6.0
}

fn t_to_dpi(t: f32) -> u16 {
    let x = (t.clamp(0.0, 1.0) * 6.0).min(5.9999);
    let i = x.floor() as usize;
    let d = STOPS[i] + (x - i as f32) * (STOPS[i + 1] - STOPS[i]);
    ((d / 50.0).round() * 50.0).clamp(100.0, 25600.0) as u16
}

#[derive(Default)]
pub struct SensState {
    /// (level being dragged, preview value, pointer dy)
    drag: Option<(u16, u16, f32)>,
    /// (level being edited, text)
    editing: Option<(u16, String)>,
}

impl GhubApp {
    pub(crate) fn sensitivity(&mut self, ui: &mut Ui, panel: Rect, content: Rect) {
        let cfg = self.cfg();
        let prof = cfg.current().1.clone();
        let t = prof.dpi.clone();
        let x0 = panel.left() + 16.0;
        let top = panel.top();
        text(ui, pos2(x0, top + 17.0), Align2::LEFT_TOP, "Sensitivity (DPI)", t_panel_title());
        para(ui, pos2(x0, top + 56.0), 278.0, "DPI is the speed of your mouse on the screen. Use DPI buttons on your mouse to quickly change the DPI speed.", Ts::new(Fam::Reg, 12.0, pal().text_high).ls(0.6));
        text(ui, pos2(x0, top + 128.0), Align2::LEFT_TOP, "DPI SPEEDS", t_section());
        let mut x = x0;
        let mut pick: Option<u16> = None;
        let mut menu: Option<(u16, egui::Pos2)> = None;
        for &d in &t.levels {
            let color = if d == t.shift { pal().amber } else { pal().text };
            let s = d.to_string();
            let ts = Ts::new(Fam::Light, 16.0, color);
            let g = galley(ui, &s, ts, f32::INFINITY);
            let r = Rect::from_min_size(pos2(x, top + 164.0), g.size());
            let resp = click(ui, r.expand(4.0), ("lvl", d));
            ui.painter().galley(r.min, g, color);
            if d == t.active {
                ui.painter().line_segment([pos2(r.left(), r.bottom() + 1.0), pos2(r.right(), r.bottom() + 1.0)], Stroke::new(1.0, color));
            }
            if resp.clicked() {
                pick = Some(d);
            }
            if resp.secondary_clicked() {
                menu = Some((d, resp.interact_pointer_pos().unwrap_or(r.center())));
            }
            x = r.right() + 16.0;
        }
        let amber = Ts::new(Fam::Reg, 12.0, pal().amber).ls(0.6);
        let body = Ts::new(Fam::Reg, 12.0, pal().text_high).ls(0.6);
        rich(ui, pos2(x0, top + 210.0), 278.0, &[
            ("Your CURRENT DPI SPEED is underlined. Hold down a ", body),
            ("DPI SHIFT", amber),
            (" button to quickly toggle to another speed while gaming.", body),
        ]);
        let ar = Rect::from_min_size(pos2(x0 + 4.0, top + 263.0), vec2(20.0, 20.0));
        let link = click(ui, Rect::from_min_max(ar.min, pos2(x0 + 245.0, ar.bottom())), "assign-dpi");
        plus_square(ui.painter(), ar.center(), 18.0, Stroke::new(1.5, pal().text));
        let lr = text(ui, pos2(x0 + 42.0, ar.center().y), Align2::LEFT_CENTER, "ASSIGN THESE TO YOUR MOUSE", Ts::new(Fam::Light, 12.0, if link.hovered() { pal().primary } else { pal().text }));
        ui.painter().line_segment([pos2(lr.left(), lr.bottom()), pos2(lr.right(), lr.bottom())], Stroke::new(1.0, if link.hovered() { pal().primary } else { pal().text }));
        if link.clicked() {
            self.edit(|c| {
                let p = c.current_profile_mut();
                for s in [5, 6, 7] {
                    p.base[s] = Assignment::Default;
                }
            });
            self.say("G6 DPI Shift · G7 DPI Down · G8 DPI Up");
        }
        text(ui, pos2(x0, top + 302.0), Align2::LEFT_TOP, "REPORT RATE (PER SECOND)", t_section());
        para(ui, pos2(x0, top + 336.0), 278.0, "Choose how often the mouse reports information to your computer.", body);
        for (i, ms) in [1u8, 2, 4, 8].into_iter().enumerate() {
            let cx = x0 + 20.0 + i as f32 * 50.5;
            text(ui, pos2(cx, top + 374.0), Align2::CENTER_CENTER, &crate::device::rate_hz(ms).to_string(), Ts::new(Fam::Bold, 12.0, pal().muted));
            if radio(ui, pos2(cx, top + 405.0), prof.rate_ms == ms, ("rate", ms)).clicked() {
                self.edit(|c| c.current_profile_mut().rate_ms = ms);
            }
        }
        let _ = RATES_MS;
        if dark_button(ui, Rect::from_min_size(pos2(x0, top + 457.0), vec2(278.0, 32.0)), "RESTORE DEFAULT SETTINGS", "dpi-restore", true).clicked() {
            self.edit(|c| {
                let p = c.current_profile_mut();
                p.dpi = DpiTable::default();
                p.rate_ms = 1;
            });
            self.say("Sensitivity restored to defaults");
        }
        if let Some(d) = pick {
            self.edit(|c| c.current_profile_mut().dpi.active = d);
        }
        if let Some((d, pos)) = menu {
            self.open_overlay(ui.ctx(), Overlay::Dpi { dpi: d, pos });
        }
        self.dpi_axis(ui, content, &t, &prof);
    }

    fn dpi_axis(&mut self, ui: &mut Ui, content: Rect, t: &DpiTable, prof: &Profile) {
        let cx = content.center().x;
        let wt = content.top() - 88.0;
        text(ui, pos2(cx, wt + 132.0), Align2::CENTER_CENTER, "DPI SPEEDS", Ts::new(Fam::Reg, 24.0, pal().text).ls(1.2));
        let lg = galley(ui, "DPI SHIFT SPEED", t_body_bold(), f32::INFINITY);
        let lx = cx - (lg.size().x + 26.0) / 2.0;
        diamond(ui.painter(), pos2(lx + 7.0, wt + 192.0), 8.0, pal().amber);
        ui.painter().galley(pos2(lx + 26.0, wt + 192.0 - lg.size().y / 2.0), lg, pal().text);
        text(ui, pos2(cx, wt + 228.0), Align2::CENTER_CENTER, "Click on the DPI speed number to change it manually.", t_body_bold());
        text(ui, pos2(cx, wt + 252.0), Align2::CENTER_CENTER, "Click anywhere on the slider to create a new DPI speed.", t_body_bold());

        let x0 = content.left() + 87.0;
        let x1 = content.right() - 84.0;
        let y = wt + 372.0;
        let xf = |d: f32| x0 + dpi_to_t(d) * (x1 - x0);
        let p = ui.painter();
        p.line_segment([pos2(x0, y), pos2(x1, y)], Stroke::new(3.0, pal().button));
        for (i, s) in STOPS.iter().enumerate() {
            let x = x0 + i as f32 / 6.0 * (x1 - x0);
            p.line_segment([pos2(x, y - 4.0), pos2(x, y + 4.0)], Stroke::new(1.0, pal().muted));
            text(ui, pos2(x, y + 38.0), Align2::CENTER_CENTER, &format!("{}", *s as u32), Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.6));
        }
        for k in (5000..=25000).step_by(1000) {
            if k == 14800 {
                continue;
            }
            let x = xf(k as f32);
            ui.painter().line_segment([pos2(x, y - 3.0), pos2(x, y + 3.0)], Stroke::new(1.0, pal().disabled));
        }

        // empty-track click adds a level
        let track = hit(ui, Rect::from_min_max(pos2(x0 - 6.0, y - 10.0), pos2(x1 + 6.0, y + 10.0)), "dpi-track", Sense::click());
        let mut add: Option<u16> = None;
        if track.clicked()
            && let Some(pp) = track.interact_pointer_pos()
        {
            add = Some(t_to_dpi((pp.x - x0) / (x1 - x0)));
        }

        let mut commit: Option<(u16, Option<u16>)> = None; // (from, Some(to) | None = delete)
        let mut menu: Option<(u16, egui::Pos2)> = None;
        let mut start_edit: Option<u16> = None;
        let levels = t.levels.clone();
        for (i, &d) in levels.iter().enumerate() {
            let shown = match self.sens.drag {
                Some((from, v, _)) if from == d => v,
                _ => d,
            };
            let deleting = matches!(self.sens.drag, Some((from, _, dy)) if from == d && dy > 50.0);
            let px = xf(shown as f32);
            let pr = hit(ui, Rect::from_center_size(pos2(px, y), vec2(18.0, 22.0)), ("dpi-pt", d), Sense::click_and_drag()).on_hover_cursor(egui::CursorIcon::Grab);
            if pr.drag_started() {
                self.sens.drag = Some((d, d, 0.0));
            }
            if pr.dragged()
                && let Some(pp) = pr.interact_pointer_pos()
            {
                let v = t_to_dpi((pp.x - x0) / (x1 - x0));
                let free = !levels.iter().any(|&o| o != d && o == v);
                if let Some((_, cur, dy)) = &mut self.sens.drag {
                    if free {
                        *cur = v;
                    }
                    *dy = pp.y - y;
                }
            }
            if pr.drag_stopped()
                && let Some((from, v, dy)) = self.sens.drag.take()
            {
                commit = Some((from, if dy > 50.0 { None } else { Some(v) }));
            }
            if pr.secondary_clicked() {
                menu = Some((d, pr.interact_pointer_pos().unwrap_or(pos2(px, y))));
            }
            let is_shift = d == t.shift;
            let c = if deleting { pal().error } else if is_shift { pal().amber } else { pal().text };
            if is_shift {
                diamond(ui.painter(), pos2(px, y), 8.0, c);
            } else {
                ui.painter().circle_filled(pos2(px, y), 6.0, c);
            }
            // number above; stagger neighbours that would collide
            let up = i > 0 && (px - xf(levels[i - 1] as f32)).abs() < 44.0 && i % 2 == 1;
            let ly = if up { y - 46.0 } else { y - 23.0 };
            let editing = matches!(&self.sens.editing, Some((e, _)) if *e == d);
            if editing {
                let mut s = self.sens.editing.as_ref().map(|e| e.1.clone()).unwrap_or_default();
                let r = Rect::from_center_size(pos2(px, ly), vec2(64.0, 24.0));
                ui.painter().rect_filled(r, egui::CornerRadius::same(4), pal().surface);
                let te = egui::TextEdit::singleline(&mut s).font(Ts::new(Fam::Bold, 16.0, pal().text).font()).frame(egui::Frame::NONE).align(egui::Align2::CENTER_CENTER).char_limit(5);
                let mut child = ui.new_child(egui::UiBuilder::new().max_rect(r));
                let resp = child.put(r.shrink2(vec2(4.0, 2.0)), te);
                resp.request_focus();
                let (enter, esc) = ui.ctx().input(|i| (i.key_pressed(egui::Key::Enter), i.key_pressed(egui::Key::Escape)));
                if enter || resp.lost_focus() {
                    if let Ok(v) = s.trim().parse::<u16>() {
                        let v = ((v as f32 / 50.0).round() * 50.0).clamp(100.0, 25600.0) as u16;
                        commit = Some((d, Some(v)));
                    }
                    self.sens.editing = None;
                } else if esc {
                    self.sens.editing = None;
                } else {
                    self.sens.editing = Some((d, s.chars().filter(|c| c.is_ascii_digit()).collect()));
                }
            } else {
                let lt = Ts::new(Fam::Bold, 16.0, c);
                let r = text(ui, pos2(px, ly), Align2::CENTER_CENTER, &shown.to_string(), lt);
                if d == t.active {
                    ui.painter().line_segment([pos2(r.left(), r.bottom() + 1.0), pos2(r.right(), r.bottom() + 1.0)], Stroke::new(1.5, c));
                }
                if d == t.default && !is_shift {
                    ui.painter().circle_filled(pos2(px, y + 14.0), 2.0, pal().muted);
                }
                if click(ui, r.expand(4.0), ("dpi-num", d)).clicked() {
                    start_edit = Some(d);
                }
            }
        }
        if let Some(d) = start_edit {
            self.sens.editing = Some((d, d.to_string()));
        }
        if let Some((from, to)) = commit {
            match to {
                Some(v) if v != from => self.edit(|c| c.current_profile_mut().dpi.retarget(from, v)),
                None if levels.len() > 1 => {
                    self.edit(|c| c.current_profile_mut().dpi.remove(from));
                    self.say(format!("Removed {from} DPI"));
                }
                _ => {}
            }
        } else if let Some(v) = add {
            if levels.len() >= DpiTable::MAX_LEVELS {
                self.say("Up to 5 DPI speeds — drag one off the slider to remove it");
            } else if !levels.contains(&v) {
                self.edit(|c| {
                    c.current_profile_mut().dpi.insert(v);
                });
            }
        }
        if let Some((d, pos)) = menu {
            self.open_overlay(ui.ctx(), Overlay::Dpi { dpi: d, pos });
        }
        if self.sens.drag.is_some_and(|(_, _, dy)| dy > 50.0) {
            text(ui, pos2(cx, y + 70.0), Align2::CENTER_CENTER, "Release to remove this DPI speed", Ts::new(Fam::Bold, 12.0, pal().error));
        }
        let has_shift = (0..crate::device::SLOTS).any(|s| prof.effective(s, false) == Assignment::System(SystemFn::DpiShift));
        if !has_shift {
            para(
                ui,
                pos2(content.left() + 20.0, wt + 522.0),
                content.width() - 40.0,
                "DPI Shift needs to be assigned to a button on your mouse to quickly switch to the DPI Shift speed.",
                Ts::new(Fam::Bold, 12.0, pal().amber),
            );
        }
        text(ui, pos2(cx, content.bottom() - 30.0), Align2::CENTER_CENTER, "Right-click a speed for more options", Ts::new(Fam::Reg, 11.0, pal().n(0x5a)).ls(0.6));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn axis_matches_ghub_positions() {
        // G HUB @1024x768: 100 -> x473, 600 -> x516, 1000 -> x551, 1600 -> x597, 25600 -> x940
        let x = |d: f32| 473.0 + dpi_to_t(d) * 467.0;
        assert!((x(600.0) - 516.0).abs() < 1.5);
        assert!((x(1000.0) - 551.0).abs() < 1.5);
        assert!((x(1600.0) - 597.0).abs() < 1.5);
        assert_eq!(t_to_dpi(dpi_to_t(14800.0)), 14800);
        assert_eq!(t_to_dpi(1.0), 25600);
    }
}
