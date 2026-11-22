//! Device page frame (back arrow, title, profile selector, left rail, 310px
//! panel) and the full-page Device Settings view.

use super::theme::*;
use super::{DevTab, GhubApp, Route};
use crate::engine::EngineMsg;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Stroke, StrokeKind, TextureHandle, Ui, Vec2, pos2, vec2};

/// A device render placed on screen; maps metadata image px to screen.
#[derive(Clone, Copy)]
pub(crate) struct Placed {
    pub rect: Rect,
    pub scale: f32,
}

impl Placed {
    pub fn new(origin: Vec2, center_x: f32, top: f32, height: f32) -> Self {
        let scale = height / origin.y;
        let w = origin.x * scale;
        Self { rect: Rect::from_min_size(pos2(center_x - w / 2.0, top), vec2(w, height)), scale }
    }
    pub fn at(&self, x: f32, y: f32) -> Pos2 {
        self.rect.min + vec2(x, y) * self.scale
    }
}

pub(crate) fn draw_render(ui: &Ui, tex: Option<&TextureHandle>, placed: Placed, tint: Color32) {
    match tex {
        Some(t) => {
            ui.painter().image(t.id(), placed.rect, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        None => {
            // Fallback silhouette when G HUB's renders aren't installed.
            let r = placed.rect.shrink2(vec2(placed.rect.width() * 0.12, placed.rect.height() * 0.08));
            ui.painter().rect_filled(r, CornerRadius::same(60), pal().n(0x1c));
            ui.painter().rect_stroke(r, CornerRadius::same(60), Stroke::new(1.0, pal().outline_hi), StrokeKind::Inside);
        }
    }
}

impl GhubApp {
    pub(crate) fn device_page(&mut self, ui: &mut Ui, win: Rect, tab: DevTab) {
        self.asg.last_tab = Some(tab);
        let header = Rect::from_min_max(win.left_top(), pos2(win.right(), win.top() + 88.0));
        self.drag_area(ui, header);
        let back = click(ui, Rect::from_center_size(pos2(win.left() + 37.0, win.top() + 56.0), vec2(40.0, 40.0)), "back");
        arrow_back(ui.painter(), back.rect.center(), Stroke::new(2.0, if back.hovered() { pal().text } else { pal().text_high }));
        if back.clicked() {
            self.route = Route::Home;
        }
        text(ui, pos2(win.left() + 82.0, win.top() + 59.0), Align2::LEFT_CENTER, "G502 HERO", t_device());
        let sel = Rect::from_center_size(pos2(win.right() - 254.0, win.top() + 56.0), vec2(280.0, 42.0));
        self.profile_selector(ui, sel);
        self.rail(ui, win, tab);

        let panel = Rect::from_min_max(pos2(win.left() + 76.0, win.top() + 96.0), pos2(win.left() + 386.0, win.bottom() - 20.0));
        ui.painter().rect_filled(panel, CornerRadius::same(12), pal().surface);
        let content = Rect::from_min_max(pos2(panel.right(), header.bottom()), win.right_bottom());
        match tab {
            DevTab::Assignments => self.assignments(ui, panel, content),
            DevTab::Sensitivity => self.sensitivity(ui, panel, content),
            DevTab::Lighting => self.lighting(ui, panel, content),
        }
        let room = sel.left() - (win.left() + 300.0);
        if room > 200.0 {
            let w = room.min(260.0) - 16.0;
            self.ghub_banner_compact(ui, Rect::from_min_size(pos2(sel.left() - 16.0 - w, sel.center().y - 18.0), vec2(w, 36.0)));
        }
    }

    fn rail(&mut self, ui: &Ui, win: Rect, tab: DevTab) {
        let items = [
            (DevTab::Sensitivity, "Sensitivity (DPI)", 140.0),
            (DevTab::Assignments, "Assignments", 192.0),
            (DevTab::Lighting, "LIGHTSYNC", 244.0),
        ];
        let mut tip: Option<(Pos2, &str)> = None;
        for (t, name, y) in items {
            let c = pos2(win.left() + 38.0, win.top() + y);
            let r = click(ui, Rect::from_center_size(c, vec2(48.0, 48.0)), ("rail", name));
            let active = t == tab;
            if active {
                ui.painter().rect_filled(r.rect, CornerRadius::same(8), pal().primary);
            } else if r.hovered() {
                ui.painter().rect_filled(r.rect, CornerRadius::same(8), pal().surface);
            }
            let color = if active { pal().on_primary } else if r.hovered() { pal().text } else { pal().text_high };
            let s = Stroke::new(1.6, color);
            match t {
                DevTab::Sensitivity => dpi_icon(ui.painter(), c, 24.0, s),
                DevTab::Assignments => plus_square(ui.painter(), c, 19.0, s),
                DevTab::Lighting => sun(ui.painter(), c, 23.0, s),
            }
            if r.hovered() {
                tip = Some((pos2(r.rect.right() + 8.0, c.y), name));
            }
            if r.clicked() {
                self.route = Route::Device(t);
            }
        }
        let gc = pos2(win.left() + 38.0, win.bottom() - 44.0);
        let g = click(ui, Rect::from_center_size(gc, vec2(48.0, 48.0)), "device-settings");
        ui.painter().rect_filled(g.rect, CornerRadius::same(8), if g.hovered() { pal().surface_hi } else { pal().n(0x0c) });
        ui.painter().rect_stroke(g.rect, CornerRadius::same(8), Stroke::new(1.0, pal().n(0x2f)), StrokeKind::Inside);
        gear(ui.painter(), gc, 19.0, Stroke::new(1.6, pal().text));
        if g.hovered() {
            tip = Some((pos2(g.rect.right() + 8.0, gc.y), "Device Settings"));
        }
        if g.clicked() {
            self.route = Route::DeviceSettings;
        }
        if let Some((p, s)) = tip {
            tooltip(ui, p, s);
        }
    }

    /// Full-page settings, like G HUB's device gear page.
    pub(crate) fn device_settings(&mut self, ui: &Ui, win: Rect) {
        let header = Rect::from_min_max(win.left_top(), pos2(win.right(), win.top() + 88.0));
        self.drag_area(ui, header);
        let back = click(ui, Rect::from_center_size(pos2(win.left() + 39.0, win.top() + 57.0), vec2(40.0, 40.0)), "ds-back");
        arrow_back(ui.painter(), back.rect.center(), Stroke::new(2.0, pal().text));
        if back.clicked() {
            self.route = Route::Device(self.asg.last_tab.unwrap_or(DevTab::Assignments));
        }
        let cx = win.center().x;
        text(ui, pos2(cx, win.top() + 93.0), Align2::CENTER_CENTER, "G502 HERO", Ts::new(Fam::Bold, 44.0, pal().n(0x8c)).ls(0.0));
        let placed = Placed::new(self.meta.front.origin, cx, win.top() + 121.0, 179.0);
        draw_render(ui, self.tex.front.as_ref(), placed, Color32::WHITE);

        let rx = win.right() - 70.0;
        text(ui, pos2(rx, win.top() + 81.0), Align2::RIGHT_CENTER, "FIRMWARE VERSION", t_small_label());
        let fw = if self.dev.connected { format!("G502 HERO : {}", self.dev.info.fw) } else { "G502 HERO : —".into() };
        text(ui, pos2(rx, win.top() + 103.0), Align2::RIGHT_CENTER, &fw, t_body_bold());
        if !self.dev.info.bootloader.is_empty() {
            text(ui, pos2(rx, win.top() + 125.0), Align2::RIGHT_CENTER, &format!("BOOTLOADER : {}", self.dev.info.bootloader), t_small_label());
        }
        text(ui, pos2(rx, win.top() + 147.0), Align2::RIGHT_CENTER, &format!("HID++ {}.{}", self.dev.info.protocol.0, self.dev.info.protocol.1), t_small_label());

        let cfg = self.cfg();
        // left column: GHUB-X / Basilisk options
        let lx = win.left() + 176.0;
        let mut y = win.top() + 372.0;
        text(ui, pos2(lx, y), Align2::LEFT_CENTER, "DPI LIGHTING", t_small_label());
        y += 24.0;
        if checkbox(ui, pos2(lx, y - 7.0), "ALWAYS ON", cfg.general.dpi_led_always_on, true, "ds-dpi-led").clicked() {
            self.edit(|c| c.general.dpi_led_always_on = !c.general.dpi_led_always_on);
        }
        y += 32.0;
        text(ui, pos2(lx, y), Align2::LEFT_CENTER, "DEVICE STARTUP EFFECT", t_small_label());
        y += 24.0;
        let can_write = !crate::engine::observer_mode() && !self.status.ghub_running && self.dev.connected;
        let startup = self.dev.startup_effect.unwrap_or(true);
        if checkbox(ui, pos2(lx, y - 7.0), "ENABLE", startup, can_write || self.shot.is_some(), "ds-startup").clicked() && can_write {
            let _ = self.eng.send(EngineMsg::SetStartupEffect(!startup));
        }
        y += 32.0;
        text(ui, pos2(lx, y), Align2::LEFT_CENTER, "SOFTWARE CONTROL", t_small_label());
        y += 24.0;
        if checkbox(ui, pos2(lx, y - 7.0), "TAKE CONTROL WHEN G HUB IS CLOSED", cfg.general.take_control, true, "ds-take").clicked() {
            self.edit(|c| c.general.take_control = !c.general.take_control);
        }
        y += 22.0;
        let (st, col) = self.control_text();
        para(ui, pos2(lx + 24.0, y - 6.0), 300.0, &st, Ts::new(Fam::Reg, 11.0, col).ls(0.4));
        y += 34.0;
        text(ui, pos2(lx, y), Align2::LEFT_CENTER, "SCROLL ACCELERATION", t_small_label());
        y += 24.0;
        if checkbox(ui, pos2(lx, y - 7.0), "ENABLE", cfg.extras.scroll_accel, true, "ds-accel").clicked() {
            self.edit(|c| c.extras.scroll_accel = !c.extras.scroll_accel);
        }
        let mut boost = cfg.extras.scroll_boost;
        let sr = Rect::from_min_size(pos2(lx + 110.0, y - 6.0), vec2(150.0, 12.0));
        if slider(ui, sr, &mut boost, 0.5..=8.0, "ds-boost") {
            self.edit(|c| c.extras.scroll_boost = (boost * 2.0).round() / 2.0);
        }
        text(ui, pos2(sr.right() + 14.0, y), Align2::LEFT_CENTER, &format!("{:.1}x", cfg.extras.scroll_boost), t_body_bold());
        y += 22.0;
        para(ui, pos2(lx + 24.0, y - 6.0), 300.0, "Fast flicks of the wheel scroll further, like Razer's scroll acceleration.", Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4));
        y += 36.0;
        text(ui, pos2(lx, y), Align2::LEFT_CENTER, "DPI FEEDBACK", t_small_label());
        y += 24.0;
        if checkbox(ui, pos2(lx, y - 7.0), "FLASH LIGHTING ON DPI CHANGE", cfg.general.dpi_flash, true, "ds-flash").clicked() {
            self.edit(|c| c.general.dpi_flash = !c.general.dpi_flash);
        }
        y += 22.0;
        para(ui, pos2(lx + 24.0, y - 6.0), 300.0, "Both lighting zones show the speed's color for a moment.", Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4));
        y += 36.0;
        text(ui, pos2(lx, y), Align2::LEFT_CENTER, "PERFECT 180", t_small_label());
        para(ui, pos2(lx, y + 14.0), 330.0, "Calibrate and assign it from Assignments > ACTIONS.", Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4));

        // right column: on-board memory
        let ox = win.left() + 600.0;
        let mut y = win.top() + 364.0;
        let onboard = !cfg.general.take_control;
        text(ui, pos2(ox, y), Align2::LEFT_CENTER, "ON-BOARD MEMORY MODE", t_small_label().color(pal().text));
        if onoff(ui, pos2(ox + 216.0, y), onboard, "ds-onboard").clicked() {
            self.edit(|c| c.general.take_control = !c.general.take_control);
        }
        y += 28.0;
        if !self.onboard_asked && self.shot.is_none() && self.dev.connected {
            self.onboard_asked = true;
            let _ = self.eng.send(EngineMsg::ReadOnboard);
        }
        let n = self.dev.info.profile_count.clamp(1, 5);
        let active_slot = self.dev.onboard_profile;
        for slot in 1..=n {
            let enabled = onboard && self.dev.connected && !self.status.ghub_running;
            let on = enabled && active_slot == slot;
            let c = pos2(ox + 6.0, y);
            let r = click(ui, Rect::from_min_size(pos2(ox - 4.0, y - 10.0), vec2(160.0, 20.0)), ("slot", slot));
            let ring = if enabled { pal().text } else { pal().n(0x3a) };
            if on {
                ui.painter().circle_filled(c, 5.0, pal().text);
            } else {
                ui.painter().circle_filled(c, 5.0, if enabled { Color32::TRANSPARENT } else { pal().n(0x2a) });
                ui.painter().circle_stroke(c, 5.0, Stroke::new(1.2, ring));
            }
            text(ui, pos2(ox + 21.0, y), Align2::LEFT_CENTER, &format!("SLOT {slot}"), Ts::new(Fam::Reg, 12.0, if enabled { pal().text } else { pal().n(0x55) }));
            let stored = self.onboard.get(slot as usize - 1).and_then(|o| o.profile.as_ref());
            let ts = Ts::new(Fam::Reg, 12.0, if enabled { pal().primary } else { pal().primary_disabled });
            let dim = self.onboard.get(slot as usize - 1).is_some_and(|o| !o.enabled);
            let label = match stored {
                Some(p) if !p.name.is_empty() => {
                    let mut s = p.name.clone();
                    let max_w = win.right() - 24.0 - (ox + 78.0);
                    while s.chars().count() > 1 && galley(ui, &s, ts, f32::INFINITY).size().x > max_w {
                        s.pop();
                        s = s.trim_end().to_string();
                        if galley(ui, &format!("{s}\u{2026}"), ts, f32::INFINITY).size().x <= max_w {
                            s.push('\u{2026}');
                            break;
                        }
                    }
                    s
                }
                _ => format!("PROFILE {slot}"),
            };
            let ts = if dim { Ts::new(Fam::Reg, 12.0, pal().n(0x55)) } else { ts };
            let name_r = text(ui, pos2(ox + 78.0, y), Align2::LEFT_CENTER, &label, ts);
            let link = click(ui, name_r.expand2(vec2(4.0, 6.0)), ("slot-name", slot));
            if link.clicked() {
                self.slot_menu = Some(super::onboard::SlotMenu { slot: slot as usize, opened_frame: ui.ctx().cumulative_frame_nr() });
            }
            if let Some(p) = stored {
                let name = |l: &[u8; 11]| crate::profile::Lighting::from_led(l).map_or("unknown", |l| super::light::effect_name(l.effect));
                let start = p.dpi.get(p.default_dpi).map_or_else(|| "?".to_string(), |d| d.to_string());
                r.clone().on_hover_text(format!("Starts at {start} DPI
Primary: {}
Logo: {}", name(&p.leds[0]), name(&p.leds[1])));
            }
            if enabled && r.clicked() {
                let _ = self.eng.send(EngineMsg::OnboardSlot(slot));
            }
            y += 25.5;
        }
        y += 8.0;
        let rb = para(ui, pos2(ox, y), 210.0, "RESTORE ALL PROFILE SLOTS TO DEFAULT", Ts::new(Fam::Reg, 12.0, pal().text).ls(0.6));
        if click(ui, rb.expand(3.0), "slot-restore-all").clicked() {
            self.restore_all_slots();
        }
        y += rb.height() + 14.0;
        para(
            ui,
            pos2(ox, y),
            250.0,
            if onboard {
                "The mouse uses the profile stored in the selected slot. GHUB-X assignments are paused."
            } else {
                "GHUB-X drives the mouse in host mode: assignments, DPI and lighting come from your profile."
            },
            Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4),
        );
        if !matches!(self.route, Route::DeviceSettings) {
            self.slot_menu = None;
            self.slot_pending = None;
            self.slot_details = None;
        }
        self.slot_details_ui(ui, win);
        self.slot_menu_ui(ui, pos2(ox - 8.0, win.top() + 347.0));
        self.slot_bar(ui, win);
    }
}

pub(crate) fn tooltip(ui: &Ui, left_mid: Pos2, s: &str) {
    let g = galley(ui, s, Ts::new(Fam::Reg, 12.0, pal().on_invert).ls(0.0), f32::INFINITY);
    let r = Rect::from_min_size(pos2(left_mid.x + 6.0, left_mid.y - 16.0), vec2(g.size().x + 40.0, 32.0));
    let p = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("ghubx-tip")));
    p.rect_filled(r, CornerRadius::same(6), pal().invert);
    p.add(egui::Shape::convex_polygon(
        vec![pos2(r.left() - 6.0, left_mid.y), pos2(r.left() + 1.0, left_mid.y - 7.0), pos2(r.left() + 1.0, left_mid.y + 7.0)],
        pal().invert,
        Stroke::NONE,
    ));
    p.galley(pos2(r.left() + 20.0, r.center().y - g.size().y / 2.0), g, pal().on_invert);
}
