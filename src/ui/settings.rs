//! App settings (gear in the header): General, Diagnostics, About.

use super::profiles::dropdown;
use super::theme::*;
use super::{GhubApp, Overlay, Route};
use crate::engine::EngineMsg;
use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui, pos2, vec2};

const NAV: [&str; 4] = ["General", "Appearance", "Diagnostics", "About"];
const ACCENTS: [[u8; 3]; 8] = [
    [0x11, 0x96, 0xff],
    [0x44, 0xd6, 0x2c],
    [0x9e, 0x5d, 0xff],
    [0xff, 0x5e, 0x01],
    [0xff, 0x3b, 0x4e],
    [0xff, 0x4f, 0xa3],
    [0x00, 0xc8, 0xb4],
    [0xff, 0xc8, 0x00],
];

impl GhubApp {
    pub(crate) fn settings_page(&mut self, ui: &mut Ui, win: Rect, tab: usize) {
        let top = win.top() + 88.0;
        let col = win.left() + 300.0;
        ui.painter().line_segment([pos2(col, top + 1.0), pos2(col, win.bottom())], Stroke::new(1.0, pal().divider));
        for (i, name) in NAV.iter().enumerate() {
            let r = Rect::from_min_size(pos2(win.left() + 48.0, top + 22.0 + i as f32 * 52.0), vec2(224.0, 50.0));
            let resp = click(ui, r, ("snav", i));
            let on = tab == i;
            if on {
                ui.painter().rect_filled(r, CornerRadius::same(8), pal().primary_tint);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(8), pal().surface);
            }
            let c = if on { pal().primary } else { pal().text };
            let ic = pos2(r.left() + 24.0, r.center().y);
            match i {
                0 => gear(ui.painter(), ic, 18.0, Stroke::new(1.5, c)),
                1 => {
                    ui.painter().circle_stroke(ic, 9.0, Stroke::new(1.5, c));
                    for d in [vec2(-3.5, -2.5), vec2(2.0, -4.0), vec2(4.0, 1.5)] {
                        ui.painter().circle_filled(ic + d, 1.7, c);
                    }
                }
                2 => chip(ui.painter(), ic, c),
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
            1 => self.appearance(ui, main),
            2 => self.diagnostics(ui, main),
            _ => self.about(ui, main),
        }
    }

    fn general(&mut self, ui: &Ui, main: Rect) {
        let cfg = self.cfg();
        let g = cfg.general.clone();
        text(ui, pos2(main.left(), main.top() + 36.0), Align2::LEFT_CENTER, "General Settings", Ts::new(Fam::Bold, 22.0, pal().text).ls(0.0));
        let mut y = main.top() + 95.0;
        if row(ui, main, y, "Launch app on login", "Always start after logging in", g.launch_on_login, "s-login") {
            let on = !g.launch_on_login;
            if crate::win::is_dev() {
                self.say("Not available in dev mode");
            } else if crate::win::set_autostart(on, g.start_minimized) {
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
            if crate::win::is_dev() {
                self.say("Not available in dev mode");
            } else if !g.launch_on_login || crate::win::set_autostart(true, on) {
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
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "Import from G HUB", Ts::new(Fam::Bold, 15.0, pal().text).ls(0.2));
        text(ui, pos2(main.left(), y + 22.0), Align2::LEFT_CENTER, "Copy profiles, macros, DPI and lighting from G HUB", Ts::new(Fam::Reg, 13.0, pal().label).ls(0.3));
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
        ui.painter().line_segment([pos2(main.left(), y), pos2(main.right(), y)], Stroke::new(1.0, pal().divider));
        y += 26.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "G HUB", Ts::new(Fam::Bold, 20.0, pal().text).ls(0.0));
        y += 34.0;
        let (status, color) = if self.status.ghub_running {
            ("G HUB is running and controls the mouse.", pal().amber)
        } else {
            ("G HUB is closed.", pal().muted)
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
        ui.painter().line_segment([pos2(main.left(), y), pos2(main.right(), y)], Stroke::new(1.0, pal().divider));
        y += 26.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "Persistent Profile", Ts::new(Fam::Bold, 20.0, pal().text).ls(0.0));
        let dd = Rect::from_min_size(pos2(main.left(), y + 36.0), vec2(312.0, 48.0));
        let label = match cfg.persistent {
            Some((a, p)) => (cfg.apps[a].name.clone(), cfg.apps[a].profiles[p].name.clone()),
            None => ("None".to_string(), String::new()),
        };
        if dropdown(ui, dd, &label.0, &label.1, "s-persist").clicked() {
            self.open_overlay(ui.ctx(), Overlay::Persistent(dd));
        }
        para(ui, pos2(main.left(), dd.bottom() + 14.0), main.width(), "This Persistent Profile overrides all other profiles in all games and apps until you set it back to None.", Ts::new(Fam::Reg, 13.0, pal().label).ls(0.3));
    }

    pub(crate) fn set_theme(&mut self, id: &str) {
        let id = id.to_string();
        self.edit(|c| c.appearance.theme = id);
    }

    fn appearance(&mut self, ui: &mut Ui, main: Rect) {
        let th = pal();
        let full = self.cfg();
        let ap = full.appearance;
        let cfg_general = full.general;
        text(ui, pos2(main.left(), main.top() + 36.0), Align2::LEFT_CENTER, "Appearance", Ts::new(Fam::Bold, 22.0, th.text).ls(0.0));
        let mut y = main.top() + 95.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "THEME", t_section());
        y += 20.0;
        for (i, (id, name)) in THEMES.iter().enumerate() {
            let tile = Rect::from_min_size(pos2(main.left() + i as f32 * 120.0, y), vec2(106.0, 84.0));
            let r = click(ui, tile, ("theme", i));
            let selected = ap.theme == *id;
            let pv = palette_for(id);
            let p = ui.painter();
            p.rect_filled(tile, CornerRadius::same(8), pv.bg);
            p.rect_filled(Rect::from_min_size(tile.min + vec2(10.0, 10.0), vec2(34.0, 52.0)), CornerRadius::same(4), pv.surface);
            p.rect_filled(Rect::from_min_size(tile.min + vec2(54.0, 14.0), vec2(36.0, 6.0)), CornerRadius::same(3), pv.text_high);
            p.rect_filled(Rect::from_min_size(tile.min + vec2(54.0, 28.0), vec2(24.0, 6.0)), CornerRadius::same(3), pv.label);
            p.rect_filled(Rect::from_min_size(tile.right_bottom() - vec2(40.0, 20.0), vec2(30.0, 10.0)), CornerRadius::same(5), pv.primary);
            let stroke = if selected { Stroke::new(2.0, th.primary) } else { Stroke::new(1.0, th.outline_hi) };
            p.rect_stroke(tile, CornerRadius::same(8), stroke, StrokeKind::Inside);
            text(ui, pos2(tile.center().x, tile.bottom() + 14.0), Align2::CENTER_CENTER, name, Ts::new(Fam::Reg, 12.0, if selected { th.text } else { th.label }));
            if r.clicked() {
                self.set_theme(id);
            }
        }
        y += 84.0 + 46.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "ACCENT COLOR", t_section());
        let cy = y + 32.0;
        let preset = palette_for(&ap.theme);
        let mut choice: Option<Option<[u8; 3]>> = None;
        for i in 0..=ACCENTS.len() {
            let center = pos2(main.left() + 14.0 + i as f32 * 34.0, cy);
            let (rgb, color) = match i.checked_sub(1) {
                None => (None, preset.primary),
                Some(k) => (Some(ACCENTS[k]), Color32::from_rgb(ACCENTS[k][0], ACCENTS[k][1], ACCENTS[k][2])),
            };
            let r = click(ui, Rect::from_center_size(center, vec2(28.0, 28.0)), ("accent", i));
            ui.painter().circle_filled(center, 11.0, color);
            if i == 0 {
                ui.painter().circle_stroke(center, 6.5, Stroke::new(1.5, preset.on_primary));
            }
            if ap.accent == rgb {
                ui.painter().circle_stroke(center, 14.0, Stroke::new(2.0, th.text));
            } else if r.hovered() {
                ui.painter().circle_stroke(center, 14.0, Stroke::new(1.0, th.outline_hi));
            }
            if r.clicked() {
                choice = Some(rgb);
            }
        }
        let hr = Rect::from_min_size(pos2(main.right() - 100.0, cy - 14.0), vec2(100.0, 28.0));
        ui.painter().rect_stroke(hr, CornerRadius::same(4), Stroke::new(1.0, th.outline_hi), StrokeKind::Inside);
        text(ui, pos2(hr.left() + 10.0, hr.center().y), Align2::LEFT_CENTER, "#", Ts::new(Fam::Reg, 12.0, th.muted));
        if let Some(c) = choice {
            self.appear_hex = c.map(|c| format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2])).unwrap_or_default();
            self.edit(|cfg| cfg.appearance.accent = c);
        }
        let hex_id = ui.make_persistent_id("appear-hex");
        if !ui.ctx().memory(|m| m.has_focus(hex_id)) {
            let c = ap.accent.unwrap_or_else(|| {
                let p = preset.primary;
                [p.r(), p.g(), p.b()]
            });
            self.appear_hex = format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
        }
        let mut hex = self.appear_hex.clone();
        let te = egui::TextEdit::singleline(&mut hex).id(hex_id).frame(egui::Frame::NONE).char_limit(6).font(Ts::new(Fam::Reg, 12.0, th.text).font()).text_color(th.text);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(hr));
        let resp = child.put(Rect::from_min_max(pos2(hr.left() + 22.0, hr.top() + 5.0), hr.right_bottom() - vec2(6.0, 2.0)), te);
        if hex != self.appear_hex {
            self.appear_hex = hex.to_uppercase().chars().filter(|c| c.is_ascii_hexdigit()).collect();
        }
        if (resp.lost_focus() || ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)))
            && self.appear_hex.len() == 6
            && let Ok(n) = u32::from_str_radix(&self.appear_hex, 16)
        {
            let c = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            if ap.accent != Some(c) {
                self.edit(|cfg| cfg.appearance.accent = Some(c));
            }
        }
        y = cy + 40.0;
        if row(ui, main, y, "Match mouse lighting", "Use the logo lighting color as the accent color", ap.follow_lighting, "s-follow") {
            self.edit(|c| c.appearance.follow_lighting = !c.appearance.follow_lighting);
        }
        y += 50.0;
        ui.painter().line_segment([pos2(main.left(), y), pos2(main.right(), y)], Stroke::new(1.0, th.divider));
        y += 26.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "PREVIEW", t_section());
        y += 22.0;
        let x = main.left();
        blue_button(ui, Rect::from_min_size(pos2(x, y), vec2(110.0, 32.0)), "BUTTON", "pv-blue");
        dark_button(ui, Rect::from_min_size(pos2(x + 122.0, y), vec2(110.0, 32.0)), "BUTTON", "pv-dark", true);
        switch(ui, pos2(x + 268.0, y + 16.0), true, th.primary, "pv-switch");
        checkbox(ui, pos2(x + 320.0, y + 9.0), "OPTION", true, true, "pv-check");
        y += 44.0;
        let mut v = 0.6;
        slider(ui, Rect::from_min_size(pos2(x, y + 15.0), vec2(160.0, 2.0)), &mut v, 0.0..=1.0, "pv-slider");
        pill(ui, Rect::from_min_size(pos2(x + 196.0, y + 5.0), vec2(58.0, 22.0)), "10 MIN", true, "pv-pill-on");
        pill(ui, Rect::from_min_size(pos2(x + 262.0, y + 5.0), vec2(58.0, 22.0)), "30 MIN", false, "pv-pill-off");
        y += 44.0;
        ui.painter().line_segment([pos2(main.left(), y), pos2(main.right(), y)], Stroke::new(1.0, th.divider));
        y += 24.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "ON-SCREEN DISPLAY", t_section());
        if dark_button(ui, Rect::from_center_size(pos2(main.right() - 55.0, y), vec2(110.0, 26.0)), "PREVIEW", "osd-preview", true).clicked() {
            let cfg = self.cfg();
            let d = &cfg.current().1.dpi;
            crate::osd::show(crate::osd::OsdKind::Dpi { value: d.active, levels: d.levels.len().min(255) as u8, active: d.active_index() as u8 });
        }
        y += 28.0;
        if row(ui, main, y, "Show DPI changes", "A small pop-up shows the new DPI speed", cfg_general.osd_dpi, "s-osd-dpi") {
            self.edit(|c| c.general.osd_dpi = !c.general.osd_dpi);
        }
        y += 52.0;
        if row(ui, main, y, "Show profile changes", "A small pop-up shows the profile that became active", cfg_general.osd_profile, "s-osd-profile") {
            self.edit(|c| c.general.osd_profile = !c.general.osd_profile);
        }
    }

    fn diagnostics(&mut self, ui: &Ui, main: Rect) {
        text(ui, pos2(main.left(), main.top() + 36.0), Align2::LEFT_CENTER, "Diagnostics", Ts::new(Fam::Bold, 22.0, pal().text).ls(0.0));
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
            text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, k, Ts::new(Fam::Reg, 12.0, pal().label));
            text(ui, pos2(main.left() + 170.0, y), Align2::LEFT_CENTER, &v, Ts::new(Fam::Reg, 12.0, pal().text).ls(0.3));
            y += 24.0;
        }
        y += 14.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "LOG", t_section());
        y += 14.0;
        let box_r = Rect::from_min_max(pos2(main.left(), y), pos2(main.right(), main.bottom() - 24.0));
        ui.painter().rect_filled(box_r, CornerRadius::same(8), pal().surface);
        let mut ly = box_r.top() + 12.0;
        for l in self.log.iter().take(((box_r.height() - 16.0) / 18.0).max(0.0) as usize) {
            text(ui, pos2(box_r.left() + 12.0, ly + 6.0), Align2::LEFT_CENTER, l, Ts::new(Fam::Reg, 11.0, pal().text_high).ls(0.2));
            ly += 18.0;
        }
        let _ = StrokeKind::Inside;
    }

    fn about(&mut self, ui: &Ui, main: Rect) {
        logo_g(ui.painter(), pos2(main.left() + 22.0, main.top() + 50.0), 40.0, pal().text);
        text(ui, pos2(main.left() + 58.0, main.top() + 50.0), Align2::LEFT_CENTER, "GHUB-X", Ts::new(Fam::Bold, 28.0, pal().text).ls(0.0));
        text(ui, pos2(main.left() + 58.0, main.top() + 76.0), Align2::LEFT_CENTER, &format!("Version {}", env!("CARGO_PKG_VERSION")), Ts::new(Fam::Reg, 12.0, pal().muted));
        let body = Ts::new(Fam::Reg, 13.0, pal().text_high).ls(0.3);
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
    text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, title, Ts::new(Fam::Bold, 15.0, pal().text).ls(0.2));
    text(ui, pos2(main.left(), y + 22.0), Align2::LEFT_CENTER, sub, Ts::new(Fam::Reg, 13.0, pal().label).ls(0.3));
    switch_sized(ui, pos2(main.right() - 24.0, y + 1.0), on, pal().primary, 1.38, id).clicked()
}
