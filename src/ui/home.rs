//! "Devices" home: grid/list toggle and the G502 HERO tile.

use super::theme::*;
use super::{DevTab, GhubApp, Route};
use crate::engine::EngineMsg;
use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

impl GhubApp {
    pub(crate) fn home(&mut self, ui: &Ui, win: Rect) {
        // grid / list switch
        let sw = Rect::from_center_size(pos2(win.right() - 92.0, win.top() + 140.0), vec2(82.0, 40.0));
        ui.painter().rect_filled(sw, CornerRadius::same(20), pal().surface);
        ui.painter().line_segment([pos2(sw.center().x, sw.top() + 10.0), pos2(sw.center().x, sw.bottom() - 10.0)], Stroke::new(1.0, pal().outline_hi));
        let g = click(ui, Rect::from_center_size(sw.center() - vec2(20.0, 0.0), vec2(36.0, 36.0)), "grid");
        let l = click(ui, Rect::from_center_size(sw.center() + vec2(21.0, 0.0), vec2(36.0, 36.0)), "list");
        grid_icon(ui.painter(), g.rect.center(), if self.list_view { pal().text } else { pal().primary });
        list_icon(ui.painter(), l.rect.center(), if self.list_view { pal().primary } else { pal().text });
        if g.on_hover_text("Tile view").clicked() {
            self.list_view = false;
        }
        if l.on_hover_text("List view").clicked() {
            self.list_view = true;
        }

        let banner = Rect::from_center_size(pos2(win.center().x, win.top() + 140.0), vec2(520.0, 52.0));
        self.ghub_banner(ui, banner);

        if self.list_view {
            self.device_row(ui, Rect::from_center_size(pos2(win.center().x, win.top() + 240.0), vec2(600.0, 112.0)));
        } else {
            let top = win.top() + 184.0;
            self.device_tile(ui, Rect::from_min_size(pos2(win.center().x - 148.0, top), vec2(296.0, 450.0)));
        }
        let (status, color) = self.control_text();
        text(ui, pos2(win.center().x, win.bottom() - 70.0), Align2::CENTER_CENTER, &status, Ts::new(Fam::Reg, 12.0, color));
    }

    pub(crate) fn control_text(&self) -> (String, Color32) {
        let cfg = self.cfg();
        if !self.dev.connected {
            ("Connect your G502 HERO".into(), pal().muted)
        } else if self.status.ghub_running {
            ("G HUB is controlling the mouse".into(), pal().amber)
        } else if self.status.controlled {
            let (a, p) = self.status.active;
            let name = cfg.apps.get(a).map(|x| format!("{}: {}", x.name, x.profiles.get(p).map(|p| p.name.as_str()).unwrap_or(""))).unwrap_or_default();
            (format!("GHUB-X is in control  ·  {name}"), pal().muted)
        } else if !cfg.general.take_control {
            ("On-board memory mode: the mouse runs its stored profile".into(), pal().muted)
        } else {
            ("Taking control…".into(), pal().muted)
        }
    }

    fn open_device(&mut self) {
        self.route = Route::Device(self.asg.last_tab.unwrap_or(DevTab::Assignments));
    }

    fn device_tile(&mut self, ui: &Ui, tile: Rect) {
        let r = click(ui, tile, "device-tile");
        let p = ui.painter();
        p.rect_filled(tile, CornerRadius::same(12), if r.hovered() { pal().n(0x27) } else { pal().surface });
        text(ui, tile.left_top() + vec2(25.0, 37.0), Align2::LEFT_TOP, "G502 HERO", Ts::new(Fam::Bold, 24.0, pal().text).ls(0.0));
        plug_icon(ui.painter(), tile.left_top() + vec2(35.0, 93.0), if self.dev.connected { pal().text_high } else { pal().disabled });
        if let Some(t) = &self.tex.front {
            let h = 240.0;
            let w = h * t.size_vec2().x / t.size_vec2().y;
            let ir = Rect::from_center_size(pos2(tile.center().x, tile.top() + 235.0), vec2(w, h));
            let tint = if self.dev.connected { pal().text } else { pal().n(0x5a) };
            ui.painter().image(t.id(), ir, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), tint);
        }
        if !self.dev.connected {
            text(ui, pos2(tile.center().x, tile.bottom() - 60.0), Align2::CENTER_CENTER, "NOT CONNECTED", Ts::new(Fam::Bold, 12.0, pal().muted));
        }
        // quick buttons: LIGHTSYNC, on-board memory
        let cfg = self.cfg();
        let lit = cfg.current().1.lighting.iter().any(|l| l.effect != crate::profile::Effect::Off);
        let b1 = Rect::from_min_size(pos2(tile.right() - 129.0, tile.bottom() - 73.0), vec2(48.0, 48.0));
        let b2 = Rect::from_min_size(pos2(tile.right() - 73.0, tile.bottom() - 73.0), vec2(48.0, 48.0));
        let r1 = click(ui, b1, "tile-light");
        let r2 = click(ui, b2, "tile-onboard");
        let onboard = !cfg.general.take_control;
        let p = ui.painter();
        p.rect_filled(b1, CornerRadius::same(8), if lit { pal().primary } else { pal().surface_hi });
        burst(p, b1.center(), if lit { pal().on_primary } else { pal().text });
        p.rect_filled(b2, CornerRadius::same(8), if onboard { pal().primary } else { pal().surface });
        p.rect_stroke(b2, CornerRadius::same(8), Stroke::new(1.0, pal().outline_hi), StrokeKind::Inside);
        chip(p, b2.center(), if onboard { pal().on_primary } else { pal().text });
        if r1.on_hover_text("LIGHTSYNC").clicked() {
            self.route = Route::Device(DevTab::Lighting);
        } else if r2.on_hover_text("Toggle On-board Memory Mode").clicked() {
            self.edit(|c| c.general.take_control = !c.general.take_control);
            let on = !self.cfg().general.take_control;
            self.say(if on { "On-board memory mode on" } else { "On-board memory mode off — GHUB-X controls the mouse" });
        } else if r.clicked() {
            self.open_device();
        }
    }

    fn device_row(&mut self, ui: &Ui, row: Rect) {
        let r = click(ui, row, "device-row");
        ui.painter().rect_filled(row, CornerRadius::same(12), if r.hovered() { pal().n(0x27) } else { pal().surface });
        if let Some(t) = &self.tex.front {
            let h = row.height() - 16.0;
            let w = h * t.size_vec2().x / t.size_vec2().y;
            ui.painter().image(t.id(), Rect::from_center_size(pos2(row.left() + 60.0, row.center().y), vec2(w, h)), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        }
        text(ui, pos2(row.left() + 120.0, row.center().y - 10.0), Align2::LEFT_CENTER, "G502 HERO", Ts::new(Fam::Bold, 20.0, pal().text).ls(0.0));
        let sub = if self.dev.connected {
            format!("{} DPI  ·  {} Hz  ·  firmware {}", self.dev.dpi, crate::device::rate_hz(self.dev.rate_ms), self.dev.info.fw)
        } else {
            "Not connected".into()
        };
        text(ui, pos2(row.left() + 120.0, row.center().y + 16.0), Align2::LEFT_CENTER, &sub, Ts::new(Fam::Reg, 12.0, pal().muted));
        if r.clicked() {
            self.open_device();
        }
        let _ = &self.eng as &crossbeam_channel::Sender<EngineMsg>;
    }
}

fn plug_icon(p: &egui::Painter, c: egui::Pos2, color: Color32) {
    let st = Stroke::new(1.5, color);
    let body = Rect::from_center_size(c + vec2(0.0, 1.0), vec2(10.0, 7.0));
    p.rect_stroke(body, CornerRadius::same(1), st, StrokeKind::Middle);
    p.line_segment([c + vec2(-2.5, -2.5), c + vec2(-2.5, -6.5)], st);
    p.line_segment([c + vec2(2.5, -2.5), c + vec2(2.5, -6.5)], st);
    p.line_segment([c + vec2(0.0, 4.5), c + vec2(0.0, 8.0)], st);
    p.line_segment([c + vec2(-4.0, 8.0), c + vec2(4.0, 8.0)], st);
}
