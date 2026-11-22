//! App settings (gear in the header): General, Diagnostics, About.

use super::profiles::dropdown;
use super::theme::*;
use super::{GhubApp, Overlay, Route};
use crate::engine::EngineMsg;
use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

const NAV: [&str; 3] = ["General", "Diagnostics", "About"];

impl GhubApp {
    pub(crate) fn settings_page(&mut self, ui: &Ui, win: Rect, tab: usize) {
        let top = win.top() + 88.0;
        let col = win.left() + 300.0;
        ui.painter().line_segment([pos2(col, top + 1.0), pos2(col, win.bottom())], Stroke::new(1.0, DIVIDER));
        for (i, name) in NAV.iter().enumerate() {
            let r = Rect::from_min_size(pos2(win.left() + 48.0, top + 22.0 + i as f32 * 52.0), vec2(224.0, 50.0));
            let resp = click(ui, r, ("snav", i));
            let on = tab == i;
            if on {
                ui.painter().rect_filled(r, CornerRadius::same(8), Color32::from_rgb(0x06, 0x26, 0x3d));
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(8), SURFACE);
            }
            let c = if on { PRIMARY } else { WHITE };
            let ic = pos2(r.left() + 24.0, r.center().y);
            match i {
                0 => gear(ui.painter(), ic, 18.0, Stroke::new(1.5, c)),
                1 => chip(ui.painter(), ic, c),
                _ => info(ui.painter(), ic, c),
            }
            text(ui, pos2(r.left() + 53.0, r.center().y), Align2::LEFT_CENTER, name, Ts::new(if on { Fam::Reg } else { Fam::Bold }, 16.0, c).ls(0.2));
            if resp.clicked() {
                self.route = Route::Settings(i);
            }
        }
        let main = Rect::from_min_max(pos2(win.left() + 389.0, top), pos2(win.right() - 48.0, win.bottom()));
        match tab {
            0 => self.general(ui, main),
            1 => self.diagnostics(ui, main),
            _ => self.about(ui, main),
        }
    }

    fn general(&mut self, ui: &Ui, main: Rect) {
        let cfg = self.cfg();
        let g = cfg.general.clone();
        text(ui, pos2(main.left(), main.top() + 36.0), Align2::LEFT_CENTER, "General Settings", Ts::new(Fam::Bold, 22.0, WHITE).ls(0.0));
        let mut y = main.top() + 95.0;
        if row(ui, main, y, "Launch app on login", "Always start after logging in", g.launch_on_login, "s-login") {
            let on = !g.launch_on_login;
            if crate::win::set_autostart(on, g.start_minimized) {
                self.edit(|c| c.general.launch_on_login = on);
            } else {
                self.say("Couldn't update the Windows startup entry");
            }
        }
        y += 52.0;
        if row(ui, main, y, "Close to tray", "Keep GHUB-X running in the background when the window is closed", g.close_to_tray, "s-tray") {
            self.edit(|c| c.general.close_to_tray = !c.general.close_to_tray);
        }
        y += 52.0;
        if row(ui, main, y, "Start minimized", "Start in the tray when launched with Windows", g.start_minimized, "s-minimized") {
            let on = !g.start_minimized;
            if !g.launch_on_login || crate::win::set_autostart(true, on) {
                self.edit(|c| c.general.start_minimized = on);
            } else {
                self.say("Couldn't update the Windows startup entry");
            }
        }
        y += 52.0;
        let sleep_on = g.lighting_sleep_min > 0;
        if row(ui, main, y, "Automatically disable lighting", if sleep_on { "" } else { "Turn off lighting after user inactivity" }, sleep_on, "s-sleep") {
            self.edit(|c| c.general.lighting_sleep_min = if sleep_on { 0 } else { 10 });
        }
        if sleep_on {
            let mut x = main.left();
            for m in [1u32, 5, 10, 15, 30] {
                let r = Rect::from_min_size(pos2(x, y + 11.0), vec2(if m < 10 { 52.0 } else { 58.0 }, 22.0));
                if pill(ui, r, &format!("{m} MIN"), g.lighting_sleep_min == m, ("sleep", m)).clicked() {
                    self.edit(|c| c.general.lighting_sleep_min = m);
                }
                x = r.right() + 8.0;
            }
        }
        y += 52.0;
        if row(ui, main, y, "Take control when G HUB is closed", "GHUB-X drives the mouse whenever G HUB isn't running", g.take_control, "s-take") {
            self.edit(|c| c.general.take_control = !c.general.take_control);
        }
        y += 52.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "Import from G HUB", Ts::new(Fam::Bold, 15.0, WHITE).ls(0.2));
        text(ui, pos2(main.left(), y + 22.0), Align2::LEFT_CENTER, "Copy profiles, macros, DPI and lighting from G HUB", Ts::new(Fam::Reg, 13.0, LABEL).ls(0.3));
        let armed = self.import_armed.is_some_and(|t| t.elapsed().as_secs_f32() < 4.0);
        let ib = Rect::from_min_size(pos2(main.right() - 190.0, y - 5.0), vec2(190.0, 32.0));
        let label = if armed { "CLICK AGAIN TO REPLACE" } else { "IMPORT" };
        if dark_button(ui, ib, label, "s-import", crate::ghub_import::settings_db().is_some()).clicked() {
            if armed {
                self.import_armed = None;
                let mut c = self.cfg();
                match crate::ghub_import::import(&mut c) {
                    Ok(s) => {
                        self.edit(|cfg| *cfg = c);
                        self.say(format!("Imported {} profile(s), {} macro(s), {} assignment(s)", s.profiles, s.macros, s.assignments));
                    }
                    Err(e) => self.say(format!("Import failed: {e}")),
                }
            } else {
                self.import_armed = Some(std::time::Instant::now());
            }
        }
        if armed {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(250));
        }
        y += 48.0;
        ui.painter().line_segment([pos2(main.left(), y), pos2(main.right(), y)], Stroke::new(1.0, DIVIDER));
        y += 26.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "G HUB", Ts::new(Fam::Bold, 20.0, WHITE).ls(0.0));
        y += 34.0;
        let (status, color) = if self.status.ghub_running {
            ("G HUB is running and controls the mouse.", AMBER)
        } else {
            ("G HUB is closed.", MUTED)
        };
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, status, Ts::new(Fam::Reg, 13.0, color).ls(0.3));
        let br = Rect::from_min_size(pos2(main.right() - 150.0, y - 16.0), vec2(150.0, 32.0));
        if self.status.ghub_running {
            if blue_button(ui, br, "CLOSE G HUB", "s-close-ghub").clicked() {
                let _ = self.eng.send(EngineMsg::CloseGhub);
            }
        } else if dark_button(ui, br, "START G HUB", "s-start-ghub", true).clicked() {
            let _ = self.eng.send(EngineMsg::StartGhub);
            self.say("Starting G HUB — GHUB-X hands the mouse over");
        }
        y += 38.0;
        ui.painter().line_segment([pos2(main.left(), y), pos2(main.right(), y)], Stroke::new(1.0, DIVIDER));
        y += 26.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "Persistent Profile", Ts::new(Fam::Bold, 20.0, WHITE).ls(0.0));
        let dd = Rect::from_min_size(pos2(main.left(), y + 36.0), vec2(312.0, 48.0));
        let label = match cfg.persistent {
            Some((a, p)) => (cfg.apps[a].name.clone(), cfg.apps[a].profiles[p].name.clone()),
            None => ("None".to_string(), String::new()),
        };
        if dropdown(ui, dd, &label.0, &label.1, "s-persist").clicked() {
            self.open_overlay(ui.ctx(), Overlay::Persistent(dd));
        }
        para(ui, pos2(main.left(), dd.bottom() + 14.0), main.width(), "This Persistent Profile overrides all other profiles in all games and apps until you set it back to None.", Ts::new(Fam::Reg, 13.0, LABEL).ls(0.3));
    }

    fn diagnostics(&mut self, ui: &Ui, main: Rect) {
        text(ui, pos2(main.left(), main.top() + 36.0), Align2::LEFT_CENTER, "Diagnostics", Ts::new(Fam::Bold, 22.0, WHITE).ls(0.0));
        let d = &self.dev;
        let mode = match d.mode {
            1 => "On-board",
            2 => "Host (software)",
            _ => "—",
        };
        let map: Vec<String> = d.spy_map[..11].iter().map(|b| b.to_string()).collect();
        let rows = [
            ("Device", if d.connected { d.info.name.clone() } else { "Not connected".into() }),
            ("Firmware", format!("{}   (bootloader {})", d.info.fw, d.info.bootloader)),
            ("HID++", format!("{}.{}  ·  {} features", d.info.protocol.0, d.info.protocol.1, d.info.features.len())),
            ("Mode", mode.to_string()),
            ("DPI", format!("{} (sensor default {})", d.dpi, d.default_dpi)),
            ("Report rate", format!("{} Hz", crate::device::rate_hz(d.rate_ms))),
            ("Button map (0x8110)", map.join(" ")),
            ("Held buttons", format!("{:011b}", self.mask).chars().rev().collect::<String>()),
            ("Control", self.control_text().0),
            ("Fonts", if self.fonts_from_ghub { "Brown Pro (from G HUB)".into() } else { "Segoe UI (G HUB not found)".into() }),
            ("Renders", if self.tex.front.is_some() { "G HUB depot".into() } else { "Not found".into() }),
            ("Settings file", crate::profile::Config::path().display().to_string()),
        ];
        let mut y = main.top() + 84.0;
        for (k, v) in rows {
            text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, k, Ts::new(Fam::Reg, 12.0, LABEL));
            text(ui, pos2(main.left() + 170.0, y), Align2::LEFT_CENTER, &v, Ts::new(Fam::Reg, 12.0, WHITE).ls(0.3));
            y += 24.0;
        }
        y += 14.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "LOG", T_SECTION);
        y += 14.0;
        let box_r = Rect::from_min_max(pos2(main.left(), y), pos2(main.right(), main.bottom() - 24.0));
        ui.painter().rect_filled(box_r, CornerRadius::same(8), SURFACE);
        let mut ly = box_r.top() + 12.0;
        for l in self.log.iter().take(((box_r.height() - 16.0) / 18.0).max(0.0) as usize) {
            text(ui, pos2(box_r.left() + 12.0, ly + 6.0), Align2::LEFT_CENTER, l, Ts::new(Fam::Reg, 11.0, HIGH).ls(0.2));
            ly += 18.0;
        }
        let _ = StrokeKind::Inside;
    }

    fn about(&mut self, ui: &Ui, main: Rect) {
        logo_g(ui.painter(), pos2(main.left() + 22.0, main.top() + 50.0), 40.0, WHITE);
        text(ui, pos2(main.left() + 58.0, main.top() + 50.0), Align2::LEFT_CENTER, "GHUB-X", Ts::new(Fam::Bold, 28.0, WHITE).ls(0.0));
        text(ui, pos2(main.left() + 58.0, main.top() + 76.0), Align2::LEFT_CENTER, &format!("Version {}", env!("CARGO_PKG_VERSION")), Ts::new(Fam::Reg, 12.0, MUTED));
        let body = Ts::new(Fam::Reg, 13.0, HIGH).ls(0.3);
        let mut y = main.top() + 120.0;
        for p in [
            "A G HUB-style control centre for the Logitech G502 HERO, written in Rust. It talks HID++ 2.0 to the mouse directly: DPI, report rate, LIGHTSYNC zones, on-board memory, and button capture through the 0x8110 spy feature in host mode.",
            "Extras inspired by the Razer Basilisk V4: Perfect 180, sensitivity clutch and scroll acceleration.",
            "Fonts and device renders are read from your local G HUB installation at runtime. Nothing from Logitech is bundled; without G HUB the app falls back to Windows fonts.",
            "Close G HUB to let GHUB-X take control. Quitting GHUB-X returns the mouse to its on-board profile.",
        ] {
            let r = para(ui, pos2(main.left(), y), main.width().min(560.0), p, body);
            y = r.bottom() + 16.0;
        }
    }
}

/// Settings row: title, grey subtitle and a switch on the right.
fn row(ui: &Ui, main: Rect, y: f32, title: &str, sub: &str, on: bool, id: &str) -> bool {
    text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, title, Ts::new(Fam::Bold, 15.0, WHITE).ls(0.2));
    text(ui, pos2(main.left(), y + 22.0), Align2::LEFT_CENTER, sub, Ts::new(Fam::Reg, 13.0, LABEL).ls(0.3));
    switch_sized(ui, pos2(main.right() - 24.0, y + 1.0), on, PRIMARY, 1.38, id).clicked()
}
