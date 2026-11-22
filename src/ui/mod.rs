//! GHUB-X user interface, laid out after G HUB 2026.7 (measured from the
//! live app over CDP): borderless window, 88px header, device pages with
//! a left rail + 310px panel, all text in Brown Pro.

pub mod assets;
mod assign;
mod device;
mod home;
mod light;
pub mod palette;
mod profiles;
mod sens;
mod settings;
pub mod theme;

use crate::device::DeviceState;
use crate::engine::{EngineMsg, Status, UiEvent};
use crate::profile::{Config, Effect};
use assets::{DeviceMeta, Textures};
use crossbeam_channel::{Receiver, Sender};
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, ViewportCommand, pos2, vec2};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};
use theme::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum DevTab {
    Sensitivity,
    Assignments,
    Lighting,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Route {
    Home,
    Device(DevTab),
    DeviceSettings,
    Profiles,
    Settings(usize),
}

pub enum Overlay {
    Profiles(Rect),
    Effect(Rect),
    Slot { slot: usize, pos: Pos2 },
    Dpi { dpi: u16, pos: Pos2 },
    Persistent(Rect),
}

/// `--shot <page> <out.png>`: render one page, save a screenshot, exit.
pub struct Shot {
    pub page: String,
    pub out: PathBuf,
    started: Instant,
    frames: u32,
    requested: bool,
    done: Arc<Mutex<bool>>,
    /// Switch to this theme on frame 10 (exercises the live-switch path).
    pub switch_to: Option<String>,
    ready_at: Option<u32>,
}

impl Shot {
    pub fn new(page: String, out: PathBuf) -> Self {
        Self { page, out, started: Instant::now(), frames: 0, requested: false, done: Default::default(), switch_to: None, ready_at: None }
    }
}

/// Shutdown sequence shared by the window's exit path and the tray's Quit:
/// save the config, stop the engine (releases the mouse) and wait for the IO thread.
pub struct Exit {
    cfg: Arc<Mutex<Config>>,
    eng: Sender<EngineMsg>,
    io_done: Receiver<()>,
    save: bool,
    done: Mutex<bool>,
}

impl Exit {
    pub fn new(cfg: Arc<Mutex<Config>>, eng: Sender<EngineMsg>, io_done: Receiver<()>, save: bool) -> Self {
        Self { cfg, eng, io_done, save, done: Mutex::new(false) }
    }

    pub fn run(&self) {
        let mut done = self.done.lock().unwrap();
        if *done {
            return;
        }
        *done = true;
        if self.save {
            let _ = self.cfg.lock().unwrap().save();
        }
        let _ = self.eng.send(EngineMsg::Quit);
        let _ = self.io_done.recv_timeout(Duration::from_secs(2));
        crate::win::remove_tray_icon();
    }
}

type Stash = Arc<Mutex<Vec<UiEvent>>>;

/// While the window is hidden no frames run, so nothing drains the event
/// channel; keep only the latest state events until the UI pumps again.
fn spawn_stash(rx: Receiver<UiEvent>, stash: Stash) {
    std::thread::spawn(move || {
        loop {
            std::thread::sleep(Duration::from_millis(250));
            let mut held = stash.lock().unwrap();
            if !crate::win::main_hidden() {
                continue;
            }
            while let Ok(ev) = rx.try_recv() {
                if !matches!(ev, UiEvent::Log(_)) {
                    held.retain(|h| std::mem::discriminant(h) != std::mem::discriminant(&ev));
                }
                held.push(ev);
                if held.len() > 60 {
                    held.remove(0);
                }
            }
        }
    });
}

pub struct GhubApp {
    pub(crate) cfg: Arc<Mutex<Config>>,
    pub(crate) eng: Sender<EngineMsg>,
    rx: Receiver<UiEvent>,
    stash: Stash,
    exit: Arc<Exit>,
    pub(crate) dev: DeviceState,
    pub(crate) status: Status,
    pub(crate) mask: u16,
    pub(crate) log: VecDeque<String>,
    pub(crate) route: Route,
    pub(crate) meta: DeviceMeta,
    pub(crate) tex: Textures,
    art_rx: Option<Receiver<assets::Art>>,
    pub(crate) fonts_from_ghub: bool,
    pub(crate) asg: assign::AsgState,
    pub(crate) sens: sens::SensState,
    pub(crate) light: light::LightState,
    pub(crate) prof: profiles::ProfState,
    pub(crate) overlay: Option<Overlay>,
    overlay_frame: u64,
    pub(crate) toast: Option<(String, Instant)>,
    dirty_at: Option<Instant>,
    shot: Option<Shot>,
    pub(crate) seen_exes: Vec<String>,
    pub(crate) p180_at: Option<Instant>,
    pub(crate) list_view: bool,
    pub(crate) import_armed: Option<Instant>,
    pub(crate) appear_hex: String,
}

impl GhubApp {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Arc<Mutex<Config>>,
        eng: Sender<EngineMsg>,
        rx: Receiver<UiEvent>,
        exit: Arc<Exit>,
        shot: Option<Shot>,
    ) -> Self {
        let stash = Stash::default();
        if shot.is_none() {
            spawn_stash(rx.clone(), stash.clone());
        }
        let ctx = &cc.egui_ctx;
        let fonts = assets::load_fonts();
        ctx.set_fonts(fonts.defs);
        set_palette(resolve(&cfg.lock().unwrap().appearance, None));
        style(ctx);
        let depot = assets::find_depot();
        let meta = assets::load_meta(depot.as_deref());
        let (tx, art_rx) = crossbeam_channel::bounded(1);
        let c2 = ctx.clone();
        std::thread::spawn(move || {
            let _ = tx.send(assets::decode_art(depot));
            c2.request_repaint();
        });
        let tex = Textures { front: None, side: None, glow: ctx.load_texture("glow", assets::glow_image(), assets::TEX_OPTS) };
        let mut app = Self {
            cfg,
            eng,
            rx,
            stash,
            exit,
            dev: DeviceState::default(),
            status: Status::default(),
            mask: 0,
            log: VecDeque::new(),
            route: Route::Home,
            meta,
            tex,
            art_rx: Some(art_rx),
            fonts_from_ghub: fonts.from_ghub,
            asg: Default::default(),
            sens: Default::default(),
            light: Default::default(),
            prof: Default::default(),
            overlay: None,
            overlay_frame: 0,
            toast: None,
            dirty_at: None,
            shot: None,
            seen_exes: Vec::new(),
            p180_at: None,
            list_view: false,
            import_armed: None,
            appear_hex: String::new(),
        };
        if let Some(s) = shot {
            app.prepare_shot(&s.page);
            app.shot = Some(s);
        }
        app
    }

    fn prepare_shot(&mut self, page: &str) {
        self.route = match page {
            "home" | "dropdown" => Route::Home,
            "assign" | "assign2" | "gshift" | "macro" | "macroedit" | "select" | "keys" | "actions" => Route::Device(DevTab::Assignments),
            "sens" => Route::Device(DevTab::Sensitivity),
            "light" | "fixed" => Route::Device(DevTab::Lighting),
            "devset" => Route::DeviceSettings,
            "profiles" | "appdetail" => Route::Profiles,
            "settings" | "sleep" => Route::Settings(0),
            "appearance" => Route::Settings(1),
            "diag" => Route::Settings(2),
            _ => Route::Home,
        };
        self.asg.side = page == "assign2";
        self.asg.gshift = page == "gshift";
        self.asg.tab = match page {
            "macro" | "macroedit" => 3,
            "keys" => 1,
            "actions" => 2,
            _ => 0,
        };
        if page == "select" {
            self.asg.selected = Some(3);
        }
        if page == "macroedit" {
            let first = self.cfg().current().0.macros.first().cloned();
            self.asg.editor = Some(first.map(|m| assign::MacroEditor::from(&m)).unwrap_or_else(assign::MacroEditor::new));
        }
        if page == "fixed" {
            let mut c = self.cfg.lock().unwrap();
            let p = c.current_profile_mut();
            p.lighting[1] = crate::profile::Lighting { effect: crate::profile::Effect::Fixed, color: [0x00, 0xB8, 0xFC], period_ms: 4000, brightness: 80 };
            drop(c);
            self.light.zone = 1;
        }
        if page == "sleep" {
            self.cfg.lock().unwrap().general.lighting_sleep_min = 10;
        }
        if page == "appdetail" {
            self.prof.open_detail(0);
        }
        if page == "dropdown" {
            self.overlay = Some(Overlay::Profiles(Rect::from_min_size(pos2(670.0, 38.0), vec2(208.0, 42.0))));
        }
    }

    /// Mutate the shared config, persist (debounced) and notify the engine.
    pub(crate) fn edit(&mut self, f: impl FnOnce(&mut Config)) {
        {
            let mut c = self.cfg.lock().unwrap();
            f(&mut c);
            c.sanitize();
        }
        self.dirty_at = Some(Instant::now());
        let _ = self.eng.send(EngineMsg::ConfigChanged);
    }

    pub(crate) fn cfg(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }

    pub(crate) fn say(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    /// Show a message once the window is up (e.g. after the first-run import).
    pub fn notify(&mut self, s: impl Into<String>) {
        let s = s.into();
        self.log.push_front(s.clone());
        self.say(s);
    }

    pub(crate) fn open_overlay(&mut self, ctx: &egui::Context, o: Overlay) {
        self.overlay = Some(o);
        self.overlay_frame = ctx.cumulative_frame_nr();
    }

    fn pump(&mut self, ctx: &egui::Context) {
        let mut events = std::mem::take(&mut *self.stash.lock().unwrap());
        while let Ok(ev) = self.rx.try_recv() {
            events.push(ev);
        }
        for ev in events {
            match ev {
                UiEvent::Device(d) => self.dev = d,
                UiEvent::Buttons(m) => {
                    if self.shot.is_none() {
                        self.mask = m;
                    }
                }
                UiEvent::Status(s) => {
                    if let Some(fg) = &s.foreground
                        && !self.seen_exes.contains(fg)
                        && !fg.eq_ignore_ascii_case("ghubx.exe")
                    {
                        self.seen_exes.insert(0, fg.clone());
                        self.seen_exes.truncate(12);
                    }
                    self.status = s;
                }
                UiEvent::Log(l) => {
                    self.log.push_front(l);
                    self.log.truncate(50);
                }
            }
        }
        if let Some(rx) = &self.art_rx
            && let Ok(art) = rx.try_recv()
        {
            self.tex.front = art.front.map(|i| ctx.load_texture("front", i, assets::TEX_OPTS));
            self.tex.side = art.side.map(|i| ctx.load_texture("side", i, assets::TEX_OPTS));
            self.art_rx = None;
        }
    }

    /// Preset + accent from the config. The lighting accent follows the logo zone
    /// of the profile the UI is showing.
    fn wanted_palette(&self, ctx: &egui::Context) -> Palette {
        let cfg = self.cfg.lock().unwrap();
        let lighting = if cfg.appearance.follow_lighting {
            let l = cfg.current().1.lighting[1];
            match l.effect {
                Effect::Fixed | Effect::Breathing if l.color.iter().max().copied().unwrap_or(0) >= 40 => Some(l.color),
                Effect::Cycle => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    let period = (l.period_ms.max(1) as f64) / 1000.0;
                    let h = (ctx.input(|i| i.time) / period).fract() as f32;
                    let c = egui::ecolor::Hsva::new(h, 1.0, 1.0, 1.0).to_srgb();
                    Some([c[0], c[1], c[2]])
                }
                _ => None,
            }
        } else {
            None
        };
        resolve(&cfg.appearance, lighting)
    }

    fn hide_to_tray(&mut self) {
        if self.dirty_at.take().is_some()
            && let Err(e) = self.cfg.lock().unwrap().save()
        {
            self.log.push_front(format!("Saving settings failed: {e}"));
        }
        crate::win::hide_main();
    }

    fn autosave(&mut self) {
        if self.shot.is_some() {
            return;
        }
        if let Some(t) = self.dirty_at
            && t.elapsed() > Duration::from_millis(400)
        {
            self.dirty_at = None;
            if let Err(e) = self.cfg.lock().unwrap().save() {
                self.log.push_front(format!("Saving settings failed: {e}"));
            }
        }
    }

    fn shot_tick(&mut self, ctx: &egui::Context) {
        let Some(s) = &mut self.shot else { return };
        s.frames += 1;
        if s.frames == 10
            && let Some(id) = s.switch_to.take()
        {
            self.set_theme(&id);
        }
        let Some(s) = &mut self.shot else { return };
        let ready = (self.tex.front.is_some() && self.dev.connected) || s.started.elapsed() > Duration::from_secs(6);
        if ready && s.ready_at.is_none() {
            s.ready_at = Some(s.frames);
        }
        if s.ready_at.is_some_and(|r| s.frames >= r + 12) && s.frames > 20 && !s.requested {
            s.requested = true;
            let out = s.out.clone();
            let done = s.done.clone();
            ctx.request_screenshot(move |img| {
                let [w, h] = img.size;
                let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255]).collect();
                let _ = image::save_buffer(&out, &bytes, w as u32, h as u32, image::ExtendedColorType::Rgba8);
                *done.lock().unwrap() = true;
            });
        }
        if *s.done.lock().unwrap() {
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        ctx.request_repaint();
    }

    // ------------------------------------------------------------ chrome

    /// Background drag area for a borderless window (call before widgets).
    pub(crate) fn drag_area(&self, ui: &Ui, rect: Rect) {
        let r = hit(ui, rect, "drag-area", Sense::click_and_drag());
        if r.drag_started() {
            ui.ctx().send_viewport_cmd(ViewportCommand::StartDrag);
        }
        if r.double_clicked() {
            let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
            ui.ctx().send_viewport_cmd(ViewportCommand::Maximized(!max));
        }
    }

    fn caption(&self, ui: &Ui, win: Rect) {
        let max = ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false));
        for (i, x) in [80.0, 48.0, 16.0].into_iter().enumerate() {
            let c = pos2(win.right() - x, win.top() + 16.0);
            let r = click(ui, Rect::from_center_size(c, vec2(32.0, 32.0)), ("caption", i));
            if r.hovered() {
                let fill = if i == 2 { Color32::from_rgb(0xe8, 0x11, 0x23) } else { pal().ink(24) };
                ui.painter().rect_filled(r.rect, CornerRadius::ZERO, fill);
            }
            caption_glyph(ui.painter(), c, i as u8, if r.hovered() { pal().text } else { pal().text_high });
            if r.clicked() {
                ui.ctx().send_viewport_cmd(match i {
                    0 => ViewportCommand::Minimized(true),
                    1 => ViewportCommand::Maximized(!max),
                    _ => ViewportCommand::Close,
                });
            }
        }
    }

    fn resize_edges(&self, ui: &Ui, win: Rect) {
        use egui::{CursorIcon as C, ResizeDirection as D};
        if ui.ctx().input(|i| i.viewport().maximized.unwrap_or(false)) {
            return;
        }
        let e = 5.0;
        let zones = [
            (Rect::from_min_max(win.left_top(), pos2(win.left() + e, win.bottom())), D::West, C::ResizeWest),
            (Rect::from_min_max(pos2(win.right() - e, win.top()), win.right_bottom()), D::East, C::ResizeEast),
            (Rect::from_min_max(pos2(win.left(), win.bottom() - e), win.right_bottom()), D::South, C::ResizeSouth),
            (Rect::from_min_max(pos2(win.right() - 12.0, win.bottom() - 12.0), win.right_bottom()), D::SouthEast, C::ResizeSouthEast),
            (Rect::from_min_max(pos2(win.left(), win.bottom() - 12.0), pos2(win.left() + 12.0, win.bottom())), D::SouthWest, C::ResizeSouthWest),
        ];
        for (i, (r, dir, cur)) in zones.into_iter().enumerate() {
            let resp = hit(ui, r, ("resize", i), Sense::drag()).on_hover_cursor(cur);
            if resp.drag_started() {
                ui.ctx().send_viewport_cmd(ViewportCommand::BeginResize(dir));
            }
        }
    }

    /// Top bar for Devices / Profiles / Settings.
    fn top_nav(&mut self, ui: &Ui, win: Rect) {
        let bar = Rect::from_min_max(win.left_top(), pos2(win.right(), win.top() + 88.0));
        self.drag_area(ui, bar);
        let p = ui.painter();
        p.line_segment([pos2(win.left(), bar.bottom()), pos2(win.right(), bar.bottom())], Stroke::new(1.0, pal().divider));
        let logo = click(ui, Rect::from_center_size(pos2(win.left() + 52.0, win.top() + 59.0), vec2(30.0, 30.0)), "logo");
        logo_g(ui.painter(), logo.rect.center(), 22.0, pal().text);
        if logo.clicked() {
            self.route = Route::Home;
        }
        let mut x = win.left() + 121.0;
        for (label, route) in [("Devices", Route::Home), ("Profiles", Route::Profiles)] {
            let active = self.route == route;
            let ts = if active { Ts::new(Fam::PanBold, 14.0, pal().primary).ls(-0.42) } else { t_nav() };
            let g = galley(ui, label, ts, f32::INFINITY);
            let rect = Rect::from_min_size(pos2(x, win.top() + 50.0), g.size());
            let r = click(ui, rect.expand2(vec2(10.0, 14.0)), ("nav", label));
            let color = if active { pal().primary } else if r.hovered() { pal().text } else { pal().nav };
            ui.painter().galley(rect.min, galley(ui, label, ts.color(color), f32::INFINITY), color);
            if active {
                ui.painter().line_segment([pos2(rect.left(), bar.bottom() - 1.5), pos2(rect.right(), bar.bottom() - 1.5)], Stroke::new(3.0, pal().primary));
            }
            if r.clicked() {
                self.route = route;
            }
            x += g.size().x + 37.0;
        }
        let sel = Rect::from_center_size(pos2(win.right() - 250.0, win.top() + 59.0), vec2(208.0, 42.0));
        self.profile_selector(ui, sel);
        let gear_c = pos2(win.right() - 110.0, win.top() + 59.0);
        let g = click(ui, Rect::from_center_size(gear_c, vec2(36.0, 36.0)), "settings");
        let active = matches!(self.route, Route::Settings(_));
        if active || g.hovered() {
            ui.painter().circle_filled(gear_c, 17.0, if active { pal().primary } else { pal().surface_hi });
        }
        gear(ui.painter(), gear_c, 19.0, Stroke::new(1.6, if active { pal().on_primary } else { pal().text }));
        if g.on_hover_text("General Settings").clicked() {
            self.route = Route::Settings(0);
        }
    }

    /// "Desktop: Default" capsule; opens the profile menu.
    pub(crate) fn profile_selector(&mut self, ui: &Ui, rect: Rect) {
        let cfg = self.cfg();
        let (app, prof) = cfg.current();
        let open = matches!(self.overlay, Some(Overlay::Profiles(_)));
        let r = click(ui, rect, "profile-selector");
        let p = ui.painter();
        p.rect_filled(rect, CornerRadius::same(6), if r.hovered() && !open { pal().surface_hi } else { pal().surface });
        if open {
            p.rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, pal().outline_hi), StrokeKind::Inside);
        }
        profile_icon(p, pos2(rect.left() + 29.0, rect.center().y), pal().text);
        let label = format!("{}: {}", app.name, prof.name);
        let tr = text(ui, pos2(rect.left() + 49.0, rect.center().y), Align2::LEFT_CENTER, &label, Ts::new(Fam::PanBold, 14.0, pal().text).ls(-0.42));
        let _ = tr;
        chevron(ui.painter(), pos2(rect.right() - 21.0, rect.center().y), 5.5, open, Stroke::new(1.8, pal().text));
        if r.clicked() {
            if open {
                self.overlay = None;
            } else {
                self.open_overlay(ui.ctx(), Overlay::Profiles(rect));
            }
        }
    }

    /// One-line banner for the device header (keeps the content area clear).
    pub(crate) fn ghub_banner_compact(&mut self, ui: &Ui, rect: Rect) {
        if !(self.status.ghub_running && self.dev.connected) {
            return;
        }
        ui.painter().rect_filled(rect, CornerRadius::same(6), pal().amber_tint);
        ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, pal().amber_line), StrokeKind::Inside);
        text(ui, pos2(rect.left() + 12.0, rect.center().y), Align2::LEFT_CENTER, "G HUB IN CONTROL", Ts::new(Fam::Bold, 11.0, pal().amber));
        let b = Rect::from_min_size(pos2(rect.right() - 104.0, rect.center().y - 13.0), vec2(96.0, 26.0));
        if blue_button(ui, b, "CLOSE G HUB", "close-ghub-h").on_hover_text("Changes are saved now and apply when GHUB-X takes over").clicked() {
            let _ = self.eng.send(EngineMsg::CloseGhub);
            self.say("Closing G HUB — GHUB-X is taking control");
        }
    }

    pub(crate) fn ghub_banner(&mut self, ui: &Ui, rect: Rect) -> bool {
        let busy = self.status.ghub_running && self.dev.connected;
        if !busy {
            return false;
        }
        ui.painter().rect_filled(rect, CornerRadius::same(8), pal().amber_tint);
        ui.painter().rect_stroke(rect, CornerRadius::same(8), Stroke::new(1.0, pal().amber_line), StrokeKind::Inside);
        let tr = Rect::from_min_max(rect.min + vec2(14.0, 0.0), pos2(rect.right() - 130.0, rect.bottom()));
        text(ui, pos2(tr.left(), rect.center().y - 7.0), Align2::LEFT_CENTER, "G HUB IS CONTROLLING THIS MOUSE", Ts::new(Fam::Bold, 12.0, pal().amber));
        text(ui, pos2(tr.left(), rect.center().y + 9.0), Align2::LEFT_CENTER, "Changes are saved and apply when GHUB-X takes over.", Ts::new(Fam::Reg, 11.0, pal().text_high).ls(0.4));
        let b = Rect::from_min_size(pos2(rect.right() - 120.0, rect.center().y - 15.0), vec2(108.0, 30.0));
        if blue_button(ui, b, "CLOSE G HUB", "close-ghub").clicked() {
            let _ = self.eng.send(EngineMsg::CloseGhub);
            self.say("Closing G HUB — GHUB-X is taking control");
        }
        true
    }

    // ------------------------------------------------------------ overlays

    fn draw_overlay(&mut self, ctx: &egui::Context) {
        let Some(kind) = &self.overlay else { return };
        let (anchor, size) = match kind {
            Overlay::Profiles(r) => (r.left_bottom() + vec2(0.0, 4.0), vec2(r.width().max(240.0), 0.0)),
            Overlay::Effect(r) => (r.left_bottom() + vec2(0.0, 2.0), vec2(160.0, 0.0)),
            Overlay::Slot { pos, .. } | Overlay::Dpi { pos, .. } => (*pos, vec2(200.0, 0.0)),
            Overlay::Persistent(r) => (r.left_bottom() + vec2(0.0, 4.0), vec2(r.width(), 0.0)),
        };
        let mut chosen: Option<usize> = None;
        let items = self.overlay_items();
        let resp = egui::Area::new(egui::Id::unique("ghubx-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .constrain(true)
            .show(ctx, |ui| {
                let row_h = 36.0;
                let h = items.len() as f32 * row_h + 12.0;
                let (rect, _) = ui.allocate_exact_size(vec2(size.x, h), Sense::hover());
                soft_shadow(ui, rect, 6);
                let th = pal();
                ui.painter().rect_filled(rect, CornerRadius::same(6), if th.dark { th.surface_hi } else { th.surface });
                if !th.dark {
                    ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, th.outline_hi), StrokeKind::Inside);
                }
                for (i, (label, active, enabled)) in items.iter().enumerate() {
                    let rr = Rect::from_min_size(pos2(rect.left(), rect.top() + 6.0 + i as f32 * row_h), vec2(rect.width(), row_h));
                    if label.is_empty() {
                        ui.painter().line_segment([pos2(rr.left() + 12.0, rr.center().y), pos2(rr.right() - 12.0, rr.center().y)], Stroke::new(1.0, pal().outline_hi));
                        continue;
                    }
                    let r = if *enabled { click(ui, rr, ("ov", i)) } else { hit(ui, rr, ("ov", i), Sense::hover()) };
                    if r.hovered() && *enabled {
                        ui.painter().rect_filled(rr.shrink2(vec2(6.0, 1.0)), CornerRadius::same(4), pal().ink(14));
                    }
                    let color = if !enabled { pal().disabled } else if *active { pal().primary } else { pal().text };
                    let bold = label.chars().all(|c| !c.is_lowercase());
                    let ts = if bold { Ts::new(Fam::Bold, 12.0, color) } else { Ts::new(Fam::Pan, 14.0, color).ls(-0.2) };
                    let pos = if bold && matches!(kind, Overlay::Profiles(_)) {
                        text(ui, rr.center(), Align2::CENTER_CENTER, label, ts)
                    } else {
                        text(ui, pos2(rr.left() + 18.0, rr.center().y), Align2::LEFT_CENTER, label, ts)
                    };
                    let _ = pos;
                    if *active {
                        check(ui.painter(), pos2(rr.right() - 22.0, rr.center().y), 4.5, Stroke::new(1.8, pal().primary));
                    }
                    if r.clicked() {
                        chosen = Some(i);
                    }
                }
                rect
            });
        let area_rect = resp.inner;
        let anchor_rect = match kind {
            Overlay::Profiles(r) | Overlay::Effect(r) | Overlay::Persistent(r) => Some(*r),
            _ => None,
        };
        if let Some(i) = chosen {
            self.overlay_pick(ctx, i);
            return;
        }
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed()
                && i.pointer.interact_pos().is_some_and(|p| !area_rect.contains(p) && !anchor_rect.is_some_and(|a| a.contains(p)))
        });
        if (pressed_outside && ctx.cumulative_frame_nr() > self.overlay_frame) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.overlay = None;
        }
    }

    /// (label, active, enabled); "" = separator.
    fn overlay_items(&self) -> Vec<(String, bool, bool)> {
        let cfg = self.cfg();
        match &self.overlay {
            Some(Overlay::Profiles(_)) => {
                let a = &cfg.apps[cfg.current_app];
                let mut v: Vec<_> = a.profiles.iter().enumerate().map(|(i, p)| (format!("{}: {}", a.name, p.name), i == a.active_profile, true)).collect();
                v.push((String::new(), false, false));
                v.push(("MANAGE PROFILES".into(), false, true));
                v
            }
            Some(Overlay::Effect(_)) => light::EFFECTS.iter().map(|(n, e)| (n.to_string(), cfg.current().1.lighting[self.light.zone].effect == *e, true)).collect(),
            Some(Overlay::Slot { slot, .. }) => {
                let layer = cfg.current().1.layer(self.asg.gshift);
                let is_macro = matches!(layer.get(*slot), Some(crate::profile::Assignment::Macro(_)));
                let mut v = vec![("Use default".to_string(), false, true), ("Disable".to_string(), false, true)];
                if is_macro {
                    v.push(("Edit macro".into(), false, true));
                }
                v
            }
            Some(Overlay::Dpi { dpi, .. }) => {
                let t = &cfg.current().1.dpi;
                vec![
                    ("Set as current speed".into(), t.active == *dpi, true),
                    ("Set as DPI shift speed".into(), t.shift == *dpi, true),
                    ("Set as default speed".into(), t.default == *dpi, true),
                    ("Delete".into(), false, t.levels.len() > 1),
                ]
            }
            Some(Overlay::Persistent(_)) => {
                let mut v = vec![("None".to_string(), cfg.persistent.is_none(), true)];
                for (ai, a) in cfg.apps.iter().enumerate() {
                    for (pi, p) in a.profiles.iter().enumerate() {
                        v.push((format!("{} - {}", a.name, p.name), cfg.persistent == Some((ai, pi)), true));
                    }
                }
                v
            }
            None => vec![],
        }
    }

    fn overlay_pick(&mut self, ctx: &egui::Context, i: usize) {
        let Some(o) = self.overlay.take() else { return };
        match o {
            Overlay::Profiles(_) => {
                let n = self.cfg().current().0.profiles.len();
                if i < n {
                    self.edit(|c| {
                        let ca = c.current_app;
                        c.apps[ca].active_profile = i;
                    });
                } else if i == n + 1 {
                    self.route = Route::Profiles;
                }
            }
            Overlay::Effect(_) => {
                let z = self.light.zone;
                let e = light::EFFECTS[i].1;
                self.edit(|c| c.current_profile_mut().lighting[z].effect = e);
            }
            Overlay::Slot { slot, .. } => {
                let gs = self.asg.gshift;
                match i {
                    0 => self.edit(|c| c.current_profile_mut().layer_mut(gs)[slot] = crate::profile::Assignment::Default),
                    1 => self.edit(|c| c.current_profile_mut().layer_mut(gs)[slot] = crate::profile::Assignment::Disabled),
                    _ => {
                        let cfg = self.cfg();
                        if let Some(crate::profile::Assignment::Macro(id)) = cfg.current().1.layer(gs).get(slot)
                            && let Some(m) = cfg.current().0.macros.iter().find(|m| &m.id == id)
                        {
                            self.asg.tab = 3;
                            self.asg.editor = Some(assign::MacroEditor::from(m));
                        }
                    }
                }
            }
            Overlay::Dpi { dpi, .. } => self.edit(|c| {
                let t = &mut c.current_profile_mut().dpi;
                match i {
                    0 => t.active = dpi,
                    1 => t.shift = dpi,
                    2 => t.default = dpi,
                    _ => t.remove(dpi),
                }
            }),
            Overlay::Persistent(_) => {
                let cfg = self.cfg();
                let mut pairs = vec![None];
                for (ai, a) in cfg.apps.iter().enumerate() {
                    for pi in 0..a.profiles.len() {
                        pairs.push(Some((ai, pi)));
                    }
                }
                let v = pairs.get(i).copied().flatten();
                self.edit(|c| c.persistent = v);
            }
        }
        let _ = ctx;
    }

    fn draw_toast(&mut self, ui: &Ui, win: Rect) {
        let Some((msg, t)) = &self.toast else { return };
        let age = t.elapsed().as_secs_f32();
        if age > 2.8 {
            self.toast = None;
            return;
        }
        let a = ((2.8 - age) / 0.3).clamp(0.0, 1.0);
        let g = galley(ui, msg, Ts::new(Fam::Reg, 12.0, pal().text.gamma_multiply(a)), 600.0);
        let rect = Rect::from_center_size(pos2(win.center().x, win.bottom() - 20.0), g.size() + vec2(36.0, 14.0));
        ui.painter().rect_filled(rect, CornerRadius::same(100), pal().surface_hi.gamma_multiply(a));
        ui.painter().galley(rect.center() - g.size() / 2.0, g, pal().text);
        ui.ctx().request_repaint();
    }
}

fn style(ctx: &egui::Context) {
    let th = pal();
    ctx.all_styles_mut(|s| {
        s.visuals = if th.dark { egui::Visuals::dark() } else { egui::Visuals::light() };
        s.visuals.panel_fill = th.bg;
        s.visuals.window_fill = th.surface_hi;
        s.visuals.window_stroke = Stroke::new(1.0, th.outline_hi);
        s.visuals.extreme_bg_color = th.bg;
        s.visuals.hyperlink_color = th.primary;
        s.visuals.selection.bg_fill = th.primary.gamma_multiply(0.5);
        s.visuals.selection.stroke = Stroke::new(1.0, th.text);
        s.visuals.text_cursor.stroke = Stroke::new(1.5, th.text);
        s.visuals.widgets.inactive.bg_fill = th.ink(40);
        s.visuals.widgets.hovered.bg_fill = th.ink(90);
        s.visuals.widgets.active.bg_fill = th.ink(150);
        s.spacing.scroll = egui::style::ScrollStyle::thin();
        s.spacing.scroll.bar_width = 4.0;
        s.spacing.item_spacing = vec2(0.0, 0.0);
        s.interaction.selectable_labels = false;
    });
}

impl eframe::App for GhubApp {
    fn ui(&mut self, ui: &mut Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();
        let want = self.wanted_palette(&ctx);
        if want != pal() {
            set_palette(want);
            style(&ctx);
        }
        if self.shot.is_none() && crate::win::tray_ready() && self.cfg.lock().unwrap().general.close_to_tray && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_to_tray();
        }
        self.pump(&ctx);
        let win = ui.max_rect();
        ui.painter().rect_filled(win, CornerRadius::ZERO, pal().bg);
        match self.route {
            Route::Home => {
                self.top_nav(ui, win);
                self.home(ui, win);
            }
            Route::Profiles => {
                self.top_nav(ui, win);
                self.profiles_page(ui, win);
            }
            Route::Settings(tab) => {
                self.top_nav(ui, win);
                self.settings_page(ui, win, tab);
            }
            Route::Device(tab) => self.device_page(ui, win, tab),
            Route::DeviceSettings => self.device_settings(ui, win),
        }
        ui.painter().rect_stroke(win, CornerRadius::ZERO, Stroke::new(1.0, if pal().dark { pal().n(0x30) } else { pal().outline_hi }), StrokeKind::Inside);
        self.caption(ui, win);
        self.resize_edges(ui, win);
        self.draw_overlay(&ctx);
        self.draw_toast(ui, win);
        self.autosave();
        self.shot_tick(&ctx);
        ctx.request_repaint_after(Duration::from_millis(200));
    }

    fn raw_input_hook(&mut self, _ctx: &egui::Context, raw: &mut egui::RawInput) {
        // Screenshots must not depend on where the real cursor is or on the clock.
        if self.shot.is_some() {
            raw.events.clear();
            let after_ready = self.shot.as_ref().and_then(|s| s.ready_at.map(|r| s.frames.saturating_sub(r)));
            raw.time = Some(1000.0 + after_ready.unwrap_or(0) as f64 * 0.05);
        }
    }

    fn on_exit(&mut self) {
        self.exit.run();
    }
}
