//! GHUB-X user interface, laid out after G HUB 2026.7 (measured from the
//! live app over CDP): borderless window, 88px header, device pages with
//! a left rail + 310px panel, all text in Brown Pro.

pub mod assets;
mod assign;
mod device;
mod home;
mod light;
mod lua_editor;
pub mod palette;
mod profiles;
mod sens;
mod macros;
mod macros_model;
mod onboard;
mod samples;
mod settings;
mod themes;
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
    /// Bell dropdown; holds the bell's anchor rect.
    Notes(Rect),
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
    /// The General hotkey field is armed: the next key event is captured.
    pub(crate) hotkey_capture: bool,
    pub(crate) appear_hex: String,
    pub(crate) ghub_autostart: Option<Option<bool>>,
    pub(crate) onboard: Vec<crate::profile::OnboardSlot>,
    pub(crate) onboard_asked: bool,
    pub(crate) fx: Option<crate::fx::Handle>,
    pub(crate) obs: crate::obs::Handle,
    /// Lua worker handle for the script editor's CONSOLE pane (fixed in dev/shot).
    pub(crate) lua: crate::lua::Handle,
    /// Session notification feed behind the header bell; the update worker
    /// pushes into a clone of it.
    pub(crate) feed: crate::notify::Feed,
    /// Update worker (`Handle::fixed` in observer/test modes).
    pub(crate) update: crate::update::Handle,
    /// The first `Status` event is a baseline, not a transition worth noting.
    status_seen: bool,
    /// Scroll offset inside the bell dropdown.
    notes_scroll: f32,
    /// The updater swapped the exe and spawned the new build; quitting now.
    applying: bool,
    /// Full-window Lua editor opened from an app detail's SCRIPTING block.
    pub(crate) script_editor: Option<lua_editor::ScriptEditor>,
    pub(crate) slot_saved_msg: Option<String>,
    pub(crate) wdl_cache: Option<(Instant, Option<bool>)>,
    pub(crate) slot_menu: Option<onboard::SlotMenu>,
    pub(crate) slot_details: Option<usize>,
    /// ASSIGN PROFILES overlay state (Device Settings ON-BOARD column).
    pub(crate) slot_assign: Option<onboard::SlotAssign>,
    pub(crate) slot_pending: Option<onboard::SlotAction>,
    /// Debounced watcher of G HUB's settings.db mtime.
    ghub_db: crate::ghub_watch::DbWatch,
    /// The next time the settings.db watcher may poll.
    ghub_db_check: Instant,
    /// settings.db changed on disk since the last successful import.
    pub(crate) ghub_db_changed: bool,
    pub(crate) theme_edit: Option<themes::ThemeEditor>,
    pub(crate) sample_edit: Option<samples::SampleEditor>,
    pub(crate) theme_page: usize,
    theme_io: Option<themes::ThemeIoRx>,
    script: Option<crate::script::Runner>,
    script_shot_pending: Arc<std::sync::atomic::AtomicBool>,
    script_png: Option<(PathBuf, bool)>,
    script_quit: bool,
}

impl GhubApp {
    #[allow(clippy::too_many_arguments)]
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        cfg: Arc<Mutex<Config>>,
        eng: Sender<EngineMsg>,
        rx: Receiver<UiEvent>,
        exit: Arc<Exit>,
        shot: Option<Shot>,
        fx: Option<crate::fx::Handle>,
        obs: crate::obs::Handle,
        lua: crate::lua::Handle,
        feed: crate::notify::Feed,
        update: crate::update::Handle,
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
            asg: {
                let mut a = assign::AsgState::default();
                a.collapsed.insert("OBS".into());
                a
            },
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
            hotkey_capture: false,
            appear_hex: String::new(),
            ghub_autostart: None,
            onboard: Vec::new(),
            onboard_asked: false,
            fx,
            obs,
            lua,
            feed,
            update,
            status_seen: false,
            notes_scroll: 0.0,
            applying: false,
            script_editor: None,
            slot_saved_msg: None,
            wdl_cache: None,
            slot_menu: None,
            slot_details: None,
            slot_assign: None,
            slot_pending: None,
            ghub_db: Default::default(),
            ghub_db_check: Instant::now(),
            ghub_db_changed: false,
            theme_edit: None,
            sample_edit: None,
            theme_page: 0,
            theme_io: None,
            script: None,
            script_shot_pending: Default::default(),
            script_png: None,
            script_quit: false,
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
            "assign" | "assign2" | "gshift" | "macro" | "macroname" | "macrotype" | "macroseq" | "macroedit" | "macrorec" | "macromenu" | "macrocustom" | "macrofold" | "macroconfirm" | "macroaction" | "select" | "keys" | "actions" | "obsactions" | "obsnew" | "p180warn" | "gamecmds" | "p180game" => Route::Device(DevTab::Assignments),
            "sens" => Route::Device(DevTab::Sensitivity),
            "light" | "fixed" | "wdl" | "effectmenu" | "screenfx" | "screenfxmine" | "sampleedit" | "audiofx" | "audiofxadv" => Route::Device(DevTab::Lighting),
            "devset" | "onboard" | "slotmenu" | "slotconfirm" | "slotdetails" | "slotassign" => Route::DeviceSettings,
            "profiles" | "appdetail" | "addgame" | "scriptedit" => Route::Profiles,
            "settings" | "sleep" => Route::Settings(0),
            "appearance" | "themeedit" | "mythemes" => Route::Settings(1),
            "ghub" => Route::Settings(2),
            "diag" => Route::Settings(3),
            _ => Route::Home,
        };
        self.asg.side = page == "assign2";
        self.asg.gshift = page == "gshift";
        self.asg.tab = match page {
            "macro" | "macroname" | "macrotype" | "macroseq" | "macroedit" | "macrorec" | "macromenu" | "macrocustom" | "macrofold" | "macroconfirm" | "macroaction" => 3,
            "keys" => 1,
            "actions" | "p180game" | "p180warn" | "obsactions" | "obsnew" => 2,
            _ => 0,
        };
        if page == "select" {
            self.asg.selected = Some(3);
        }
        if page.starts_with("macro") && page != "macro" {
            self.asg.editor = Some(self.macro_shot(page));
        }
        if page == "p180warn" {
            let mut c = self.cfg.lock().unwrap();
            c.extras.p180_game = 0;
            c.extras.p180_sens = 0.01;
            c.extras.p180_counts = 818_182;
            drop(c);
            self.asg.scroll_to = Some(200.0);
        }
        if page == "obsactions" || page == "obsnew" {
            self.asg.collapsed.remove("OBS");
            self.cfg.lock().unwrap().obs_actions = vec![crate::profile::ObsAction::Scene("Live".into()), crate::profile::ObsAction::MixerToggle("Mic/Aux".into())];
            if page == "obsnew" {
                self.asg.obs_form = Some(assign::ObsForm::default());
                self.asg.scroll_to = Some(380.0);
            }
        }
        if page == "fixed" {
            let mut c = self.cfg.lock().unwrap();
            let p = c.current_profile_mut();
            p.lighting[1] = crate::profile::Lighting { effect: crate::profile::Effect::Fixed, color: [0x00, 0xB8, 0xFC], period_ms: 4000, brightness: 80, ..Default::default() };
            drop(c);
            self.light.zone = 1;
        }
        if matches!(page, "effectmenu" | "screenfx" | "audiofx" | "audiofxadv") {
            use crate::profile::{Effect, HostFx, Lighting, ScreenArea};
            let mut c = self.cfg.lock().unwrap();
            c.current_profile_mut().lighting[1] = match page {
                "effectmenu" => Lighting { effect: Effect::Fixed, color: [0x00, 0xB8, 0xFC], period_ms: 4000, brightness: 80, ..Default::default() },
                "screenfx" => Lighting { effect: Effect::Screen, host: HostFx { area: ScreenArea::Center, boost: 50, smoothing: 40, ..Default::default() }, ..Default::default() },
                _ => Lighting {
                    effect: Effect::Audio,
                    color: [0x00, 0xB8, 0xFC],
                    host: HostFx { color2: [0x10, 0x10, 0x30], auto_sens: page != "audiofxadv", bass_only: page == "audiofxadv", ..Default::default() },
                    ..Default::default()
                },
            };
            drop(c);
            self.light.zone = 1;
            self.light.set_advanced(page == "audiofxadv");
            if page == "effectmenu" {
                self.overlay = Some(Overlay::Effect(Rect::from_min_size(pos2(88.0, 289.0), vec2(70.0, 26.0))));
            }
        }
        if matches!(page, "screenfxmine" | "sampleedit") {
            use crate::profile::{Effect, HostFx, Lighting, Sample, ScreenArea};
            let mut c = self.cfg.lock().unwrap();
            // three own samples; the second is the one the zone samples (selected in the editor)
            c.samples = vec![
                Sample { name: "Middle band".into(), rect: [40, 350, 960, 650] },
                Sample { name: "Centre block".into(), rect: [333, 333, 667, 667] },
                Sample { name: "Game corner".into(), rect: [700, 550, 980, 960] },
            ];
            c.current_profile_mut().lighting[1] = Lighting { effect: Effect::Screen, host: HostFx { area: ScreenArea::Rect([333, 333, 667, 667]), boost: 50, smoothing: 40, ..Default::default() }, ..Default::default() };
            drop(c);
            self.light.zone = 1;
            if page == "sampleedit" {
                self.sample_edit = Some(samples::SampleEditor::new(Some(1)));
            }
        }
        if page == "wdl" {
            crate::win::set_wdl(true);
        }
        if page == "themeedit" || page == "mythemes" {
            use crate::profile::CustomTheme;
            let sunset = CustomTheme { id: "custom-sunset".into(), name: "Sunset".into(), bg: [0x1A, 0x10, 0x23], surface: [0x2A, 0x1B, 0x3D], text: [0xF6, 0xEE, 0xFC], accent: [0xFF, 0x7A, 0x59] };
            if page == "themeedit" {
                self.open_theme_editor(CustomTheme { id: String::new(), ..sunset }, None, 3);
            } else {
                let paper = CustomTheme { id: "custom-paper".into(), name: "Paper".into(), bg: [0xF4, 0xF1, 0xEA], surface: [0xFF, 0xFF, 0xFF], text: [0x1D, 0x1C, 0x1A], accent: [0xC2, 0x41, 0x0C] };
                let mut c = self.cfg.lock().unwrap();
                c.appearance.custom = vec![sunset, paper];
                c.appearance.theme = "custom-sunset".into();
            }
        }
        if matches!(page, "onboard" | "slotmenu" | "slotconfirm" | "slotdetails" | "slotassign") {
            use crate::profile::{OnboardProfile, OnboardSlot, sample_sector1};
            let base = OnboardProfile::parse(&sample_sector1());
            let with = |dpi: &[u16]| {
                base.clone().map(|mut p| {
                    p.dpi = dpi.to_vec();
                    p
                })
            };
            self.onboard = [base.clone(), with(&[400, 800, 1600, 3200, 6400]), with(&[800, 1600]), with(&[1600]), None]
                .into_iter()
                .enumerate()
                .map(|(i, profile)| OnboardSlot { sector: i as u16 + 1, enabled: true, profile })
                .collect();
        }
        match page {
            "slotmenu" => self.slot_menu = Some(onboard::SlotMenu { slot: 5, opened_frame: u64::MAX }),
            "slotconfirm" => self.replace_slot(1, 0, 0),
            "slotdetails" => self.slot_details = Some(1),
            "slotassign" => {
                self.open_slot_assign(u64::MAX);
                if let Some(sa) = self.slot_assign.as_mut() {
                    // one picked row so the dropdown contents are visible
                    sa.sel[0] = onboard::Sel::Pick(0, 0);
                }
            }
            _ => {}
        }
        if page == "addgame" {
            self.prof.open_add();
        }
        if page == "gamecmds" || page == "p180game" {
            let ghub_id = crate::games::db_blocking().and_then(|d| d.by_name("Counter-Strike 2")).map(|g| g.id.clone());
            let mut c = self.cfg.lock().unwrap();
            c.apps.push(crate::profile::AppEntry { name: "Counter-Strike 2".into(), ghub_id, ..crate::profile::AppEntry::desktop() });
            c.current_app = c.apps.len() - 1;
        }
        if page == "sleep" {
            self.cfg.lock().unwrap().general.lighting_sleep_min = 10;
        }
        if page == "appdetail" {
            self.prof.open_detail(0);
        }
        if page == "scriptedit" {
            self.prof.open_detail(0);
            self.script_editor = Some(lua_editor::ScriptEditor::new((0, 0), lua_editor::TEMPLATE.into()));
        }
        if page == "dropdown" {
            self.overlay = Some(Overlay::Profiles(Rect::from_min_size(pos2(670.0, 38.0), vec2(208.0, 42.0))));
        }
        if page == "notify" {
            self.seed_notes();
            self.overlay = Some(Overlay::Notes(Rect::from_center_size(pos2(964.0, 59.0), vec2(36.0, 36.0))));
        }
    }

    /// Deterministic feed for the `notify` shot page and `GHUBX_SEED_NOTES=1`
    /// scripted runs (debug/test only — the live feed is event-driven).
    pub(crate) fn seed_notes(&mut self) {
        use crate::notify::NoteKind;
        // push oldest first so the update note sits on top
        for (text, kind) in [
            ("G HUB closed", NoteKind::Info),
            ("GHUB-X controls the mouse", NoteKind::Info),
            ("Update available: v9.9.9", NoteKind::Update),
        ] {
            self.feed.push(text, kind);
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
        self.push_tray();
    }

    /// Refresh the snapshot the tray quick-menu is built from (control state,
    /// DPI, the current app's profile list). No-op without a tray.
    fn push_tray(&self) {
        if !crate::win::tray_ready() {
            return;
        }
        let cfg = self.cfg();
        let a = cfg.current().0;
        crate::win::set_tray_info(crate::win::TrayInfo {
            control: if !self.dev.connected {
                crate::win::TrayControl::NoDevice
            } else if self.status.ghub_running {
                crate::win::TrayControl::GHub
            } else if self.status.controlled {
                crate::win::TrayControl::Ghubx
            } else {
                crate::win::TrayControl::Onboard
            },
            dpi: self.dev.dpi,
            profiles: a.profiles.iter().enumerate().map(|(i, p)| (i, p.name.clone())).collect(),
            active_profile: a.active_profile,
        });
    }

    pub(crate) fn cfg(&self) -> Config {
        self.cfg.lock().unwrap().clone()
    }

    pub(crate) fn scripted(&self) -> bool {
        self.script.is_some()
    }

    /// In-memory macro editor states for the `macro*` shot pages.
    fn macro_shot(&self, page: &str) -> macros::MacroEditor {
        use crate::profile::{Assignment, KeyCombo, Macro, MacroKind, MacroStep};
        let k = MacroStep::key;
        let name = "zz ghubx probe";
        let cfg = self.cfg();
        let find = |n: &str| cfg.current().0.macros.iter().find(|m| m.name == n).cloned();
        let custom = |std: bool| {
            let d = |v: u32| if std { 50 } else { v };
            Macro {
                id: "c".into(),
                name: "custom".into(),
                steps: vec![
                    k(0xA2, true, 0),
                    k(0x43, true, d(35)),
                    k(0x43, false, d(120)),
                    k(0xA2, false, d(40)),
                    MacroStep::text("gg wp", 0),
                    MacroStep::run(Assignment::Shortcut { name: "Play/Pause".into(), combo: KeyCombo { vk: 0xB3, mods: 0 } }, d(0)),
                    MacroStep::run(Assignment::Launch { name: "Notepad".into(), path: "notepad.exe".into() }, d(0)),
                    k(0x01, true, d(0)),
                    k(0x01, false, d(1500)),
                    MacroStep::pause(if std { 50 } else { 250 }),
                ],
                std_delay: std.then_some(50),
                show_up_down: !std,
                color: Some([0, 184, 252]),
                ..Default::default()
            }
        };
        match page {
            "macroname" => macros::MacroEditor::new_named(),
            "macrotype" => {
                let mut e = macros::MacroEditor::new_named();
                e.m.name = name.into();
                e.stage = macros::Stage::Type;
                e
            }
            "macroseq" => macros::MacroEditor::new_of(name, MacroKind::Sequence),
            "macrorec" => {
                let mut e = macros::MacroEditor::new_of("rec", MacroKind::NoRepeat);
                e.m.lists[0] = vec![k(0x48, true, 0), k(0x48, false, 50), k(0x49, true, 50), k(0x49, false, 50)];
                e.rec = Some(0);
                e.rec_at = Some(std::time::Instant::now());
                e
            }
            "macromenu" => {
                let mut e = macros::MacroEditor::new_of("menu", MacroKind::Sequence);
                e.m.lists[0] = vec![k(0x57, true, 0)];
                e.menu = Some(1);
                e
            }
            "macrocustom" | "macrofold" => {
                let mut e = macros::MacroEditor::open(&custom(page == "macrofold"));
                if page == "macrocustom" {
                    e.m.sel = vec![macros::ItemId { sec: 0, idx: 2, delay: false }];
                }
                e
            }
            "macroaction" => {
                let mut e = macros::MacroEditor::new_of("action", MacroKind::NoRepeat);
                e.m.lists[0] = vec![MacroStep::run(Assignment::Obs(crate::profile::ObsAction::ToggleRecord), 0), MacroStep::run(Assignment::Default, 50)];
                e.mode = Some(macros::Mode::Choose { sec: 0, idx: 1, launch: false, obs: true, cat: Some(0) });
                e
            }
            "macroconfirm" => {
                let m = find("jump").unwrap_or_default();
                let mut e = macros::MacroEditor::open(&m);
                if !e.m.lists[0].is_empty() {
                    e.m.lists[0].remove(0);
                }
                e.confirm = Some(macros::Confirm::Leave);
                e
            }
            _ => macros::MacroEditor::open(&find("cl").unwrap_or_default()),
        }
    }

    pub(crate) fn say(&mut self, s: impl Into<String>) {
        self.toast = Some((s.into(), Instant::now()));
    }

    /// Show a message once the window is up (e.g. after the first-run import).
    pub fn set_script(&mut self, r: crate::script::Runner) {
        if std::env::var_os("GHUBX_SEED_NOTES").is_some() {
            self.seed_notes();
        }
        self.script = Some(r);
    }

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
                UiEvent::Device(d) => {
                    if !d.connected {
                        self.onboard_asked = false;
                        self.onboard.clear();
                    }
                    self.dev = d;
                    self.push_tray();
                }
                UiEvent::Onboard(v) => self.onboard = v,
                UiEvent::OnboardWritten(r) => {
                    let ok = self.slot_saved_msg.take().unwrap_or_else(|| "Saved to the mouse and checked".into());
                    self.say(match r {
                        Ok(_) => ok,
                        Err(e) => e,
                    });
                }
                UiEvent::Buttons(m) => {
                    if self.shot.is_none() && self.script.is_none() {
                        self.mask = m;
                    }
                }
                UiEvent::Status(s) => {
                    if let Some(fg) = &s.foreground
                        && self.shot.is_none()
                        && self.script.is_none()
                        && !self.seen_exes.contains(fg)
                        && !fg.eq_ignore_ascii_case("ghubx.exe")
                    {
                        self.seen_exes.insert(0, fg.clone());
                        self.seen_exes.truncate(12);
                    }
                    self.notify_status(&s);
                    self.status = s;
                    self.push_tray();
                }
                UiEvent::TrayProfile(i) => {
                    self.edit(|c| {
                        let ca = c.current_app;
                        if let Some(a) = c.apps.get_mut(ca) {
                            a.active_profile = i.min(a.profiles.len().saturating_sub(1));
                        }
                    });
                }
                UiEvent::TrayCycleProfile => {
                    self.edit(|c| {
                        let ca = c.current_app;
                        if let Some(a) = c.apps.get_mut(ca)
                            && !a.profiles.is_empty()
                        {
                            a.active_profile = (a.active_profile + 1) % a.profiles.len();
                        }
                    });
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

    /// Feed entries for the engine transitions `Status` events carry. The
    /// first event is the startup baseline — only later diffs become notes.
    /// Scripted and shot runs push nothing (the feed stays deterministic).
    fn notify_status(&mut self, s: &Status) {
        use crate::notify::NoteKind;
        if self.shot.is_some() || self.script.is_some() {
            return;
        }
        if !self.status_seen {
            self.status_seen = true;
            return;
        }
        let old = &self.status;
        if s.ghub_running != old.ghub_running {
            self.feed.push(if s.ghub_running { "G HUB started — GHUB-X is watching" } else { "G HUB closed" }, NoteKind::Info);
        }
        if s.controlled != old.controlled {
            self.feed.push(if s.controlled { "GHUB-X controls the mouse" } else { "Mouse back on its on-board profile" }, NoteKind::Info);
        }
        if s.active != old.active
            && let Some(name) = self.cfg.lock().unwrap().profile_at(s.active).map(|p| p.name.clone())
        {
            self.feed.push(format!("Profile: {name}"), NoteKind::Info);
        }
    }

    /// Preset + accent from the config. The lighting accent follows the logo zone
    /// of the profile the UI is showing.
    fn wanted_palette(&self, ctx: &egui::Context) -> Palette {
        if let Some(ed) = &self.theme_edit {
            return palette::custom_palette(&ed.draft);
        }
        let cfg = self.cfg.lock().unwrap();
        let lighting = if cfg.appearance.follow_lighting {
            let l = cfg.current().1.lighting[1];
            match l.effect {
                Effect::Fixed | Effect::Breathing if l.color.iter().max().copied().unwrap_or(0) >= 40 => Some(l.color),
                Effect::Screen | Effect::Audio => {
                    ctx.request_repaint_after(Duration::from_millis(100));
                    self.fx.as_ref().and_then(|h| {
                        h.preview_zone(1, l);
                        h.preview(1)
                    })
                }
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

    /// Every ~5 s: has G HUB's settings.db been rewritten? A settled change
    /// puts a note on the bell and a CHANGED hint on the IMPORT button.
    /// Never watched in shot/script mode (screenshots must stay deterministic).
    fn watch_ghub_db(&mut self) {
        if self.shot.is_some() || self.scripted() || self.ghub_db_check.elapsed() < Duration::from_secs(5) {
            return;
        }
        self.ghub_db_check = Instant::now();
        let mtime = crate::ghub_import::settings_db().and_then(|p| std::fs::metadata(p).ok()).and_then(|m| m.modified().ok());
        if self.ghub_db.poll(mtime, std::time::SystemTime::now(), Duration::from_secs(2)) {
            self.feed.push("G HUB settings changed — re-import to keep GHUB-X in sync", crate::notify::NoteKind::Info);
            self.ghub_db_changed = true;
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
        // Notification bell. Shot renders skip it (except the `notify` page,
        // which exists to show it) so the header keeps its baseline pixels.
        if self.shot.as_ref().is_none_or(|s| s.page == "notify") {
            let bell_c = pos2(win.right() - 60.0, win.top() + 59.0);
            let bell_r = Rect::from_center_size(bell_c, vec2(36.0, 36.0));
            let br = click(ui, bell_r, "bell");
            let open = matches!(self.overlay, Some(Overlay::Notes(_)));
            if open || br.hovered() {
                ui.painter().circle_filled(bell_c, 17.0, if open { pal().primary } else { pal().surface_hi });
            }
            bell(ui.painter(), bell_c, 19.0, Stroke::new(1.6, if open { pal().on_primary } else { pal().text }));
            if self.feed.unread() > 0 {
                ui.painter().circle_filled(bell_c + vec2(7.5, -8.5), 3.6, pal().amber);
            }
            if br.on_hover_text("Notifications").clicked() {
                if open {
                    self.overlay = None;
                } else {
                    self.notes_scroll = 0.0;
                    self.open_overlay(ui.ctx(), Overlay::Notes(bell_r));
                }
            }
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
        if let Overlay::Notes(anchor) = *kind {
            self.draw_notes_panel(ctx, anchor);
            return;
        }
        let (anchor, size) = match kind {
            Overlay::Profiles(r) => (r.left_bottom() + vec2(0.0, 4.0), vec2(r.width().max(240.0), 0.0)),
            Overlay::Effect(r) => (r.left_bottom() + vec2(0.0, 2.0), vec2(240.0, 0.0)),
            Overlay::Slot { pos, .. } | Overlay::Dpi { pos, .. } => (*pos, vec2(200.0, 0.0)),
            Overlay::Persistent(r) => (r.left_bottom() + vec2(0.0, 4.0), vec2(r.width(), 0.0)),
            Overlay::Notes(r) => (r.left_bottom() + vec2(0.0, 4.0), vec2(360.0, 0.0)), // handled above
        };
        let mut chosen: Option<usize> = None;
        let items = self.overlay_items();
        let resp = egui::Area::new(egui::Id::unique("ghubx-overlay"))
            .order(egui::Order::Foreground)
            .fixed_pos(anchor)
            .constrain(true)
            .show(ctx, |ui| {
                let flat = matches!(kind, Overlay::Effect(_));
                let row_h = if flat { 24.0 } else { 36.0 };
                let pad = if flat { 0.0 } else { 6.0 };
                let h = items.len() as f32 * row_h + pad * 2.0;
                let (rect, _) = ui.allocate_exact_size(vec2(size.x, h), Sense::hover());
                if !flat {
                    soft_shadow(ui, rect, 6);
                }
                let th = pal();
                if flat {
                    ui.painter().rect_filled(rect, CornerRadius::same(2), th.n(0x3a));
                } else {
                    ui.painter().rect_filled(rect, CornerRadius::same(6), if th.dark { th.surface_hi } else { th.surface });
                }
                if !th.dark && !flat {
                    ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, th.outline_hi), StrokeKind::Inside);
                }
                for (i, (label, active, enabled)) in items.iter().enumerate() {
                    let rr = Rect::from_min_size(pos2(rect.left(), rect.top() + pad + i as f32 * row_h), vec2(rect.width(), row_h));
                    if label.is_empty() {
                        ui.painter().line_segment([pos2(rr.left() + 12.0, rr.center().y), pos2(rr.right() - 12.0, rr.center().y)], Stroke::new(1.0, pal().outline_hi));
                        continue;
                    }
                    let r = if *enabled { click(ui, rr, ("ov", i)) } else { hit(ui, rr, ("ov", i), Sense::hover()) };
                    if r.hovered() && *enabled {
                        ui.painter().rect_filled(rr.shrink2(vec2(6.0, 1.0)), CornerRadius::same(4), pal().ink(14));
                    }
                    let color = if !enabled {
                        pal().disabled
                    } else if flat {
                        if *active { pal().text } else { pal().n(0x88) }
                    } else if *active {
                        pal().primary
                    } else {
                        pal().text
                    };
                    let bold = label.chars().all(|c| !c.is_lowercase());
                    let ts = if bold { Ts::new(Fam::Bold, 12.0, color) } else { Ts::new(Fam::Pan, 14.0, color).ls(-0.2) };
                    let pos = if bold && matches!(kind, Overlay::Profiles(_)) {
                        text(ui, rr.center(), Align2::CENTER_CENTER, label, ts)
                    } else {
                        text(ui, pos2(rr.left() + 18.0, rr.center().y), Align2::LEFT_CENTER, label, ts)
                    };
                    let _ = pos;
                    if *active && !flat {
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
            Some(Overlay::Notes(_)) | None => vec![],
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
                            self.asg.editor = Some(macros::MacroEditor::open(m));
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
            Overlay::Notes(_) => {}
        }
        let _ = ctx;
    }

    /// Bell dropdown: the notification feed. 360 px wide, header +
    /// "MARK ALL READ", up to 8 rows of dot + two lines + "Nm ago"; scrolls
    /// past that. Closes on outside click, Escape or the bell again — the
    /// same dismissal rule as the other dropdowns.
    fn draw_notes_panel(&mut self, ctx: &egui::Context, anchor: Rect) {
        use crate::notify::{NoteKind, ago};
        const W: f32 = 360.0;
        const ROW: f32 = 46.0;
        const HEAD: f32 = 40.0;
        let notes = self.feed.notes();
        let body = if notes.is_empty() { 72.0 } else { notes.len().min(8) as f32 * ROW };
        let h = HEAD + body + 6.0;
        let mut mark_read = false;
        let resp = egui::Area::new(egui::Id::unique("ghubx-notes"))
            .order(egui::Order::Foreground)
            .fixed_pos(pos2(anchor.right() - W, anchor.bottom() + 4.0))
            .constrain(true)
            .show(ctx, |ui| {
                let (rect, _) = ui.allocate_exact_size(vec2(W, h), Sense::hover());
                soft_shadow(ui, rect, 6);
                let th = pal();
                ui.painter().rect_filled(rect, CornerRadius::same(6), if th.dark { th.surface_hi } else { th.surface });
                if !th.dark {
                    ui.painter().rect_stroke(rect, CornerRadius::same(6), Stroke::new(1.0, th.outline_hi), StrokeKind::Inside);
                }
                text(ui, pos2(rect.left() + 16.0, rect.top() + HEAD / 2.0), Align2::LEFT_CENTER, "NOTIFICATIONS", Ts::new(Fam::Bold, 11.0, pal().label).ls(1.0));
                let mar = Rect::from_min_max(pos2(rect.right() - 136.0, rect.top()), pos2(rect.right() - 12.0, rect.top() + HEAD));
                let mr = click(ui, mar, "notes-mark-all");
                let mc = if self.feed.unread() == 0 { pal().muted } else if mr.hovered() { pal().text } else { pal().primary };
                text(ui, mar.center(), Align2::CENTER_CENTER, "MARK ALL READ", Ts::new(Fam::Bold, 10.0, mc).ls(0.6));
                if mr.clicked() {
                    mark_read = true;
                }
                ui.painter().line_segment([pos2(rect.left(), rect.top() + HEAD), pos2(rect.right(), rect.top() + HEAD)], Stroke::new(1.0, pal().divider));
                if notes.is_empty() {
                    text(ui, rect.center() + vec2(0.0, HEAD / 2.0), Align2::CENTER_CENTER, "No notifications", Ts::new(Fam::Reg, 13.0, pal().muted));
                } else {
                    let view = Rect::from_min_max(pos2(rect.left(), rect.top() + HEAD + 2.0), pos2(rect.right(), rect.bottom() - 4.0));
                    let content = notes.len() as f32 * ROW;
                    let max_scroll = (content - view.height()).max(0.0);
                    if max_scroll > 0.0 {
                        let dy = ctx.input(|i| if i.pointer.hover_pos().is_some_and(|p| view.contains(p)) { i.smooth_scroll_delta.y } else { 0.0 });
                        if dy != 0.0 {
                            self.notes_scroll = (self.notes_scroll + dy).clamp(0.0, max_scroll);
                        }
                    }
                    let p = ui.painter().with_clip_rect(view);
                    for (i, n) in notes.iter().enumerate() {
                        let top = view.top() + i as f32 * ROW - self.notes_scroll;
                        let rr = Rect::from_min_size(pos2(view.left(), top), vec2(view.width(), ROW));
                        if rr.bottom() < view.top() || rr.top() > view.bottom() {
                            continue;
                        }
                        let dot = match n.kind {
                            NoteKind::Update => pal().primary,
                            NoteKind::Warn => pal().amber,
                            NoteKind::Info => pal().n(0xaa),
                        };
                        p.circle_filled(pos2(rr.left() + 20.0, top + 15.0), 3.0, dot);
                        let g = galley(ui, &n.text, Ts::new(Fam::Reg, 13.0, pal().text).ls(0.0), rr.width() - 56.0);
                        p.galley(pos2(rr.left() + 36.0, top + 7.0), g.clone(), pal().text);
                        p.galley(pos2(rr.left() + 36.0, top + 7.0 + g.size().y + 1.0), galley(ui, &ago(n.at), Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.3), f32::INFINITY), pal().muted);
                        if i + 1 < notes.len() {
                            p.line_segment([pos2(rr.left() + 14.0, rr.bottom()), pos2(rr.right() - 14.0, rr.bottom())], Stroke::new(1.0, pal().divider));
                        }
                    }
                }
                rect
            });
        let area_rect = resp.inner;
        if mark_read {
            self.feed.mark_read();
        }
        let pressed_outside = ctx.input(|i| {
            i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !area_rect.contains(p) && !anchor.contains(p))
        });
        if (pressed_outside && ctx.cumulative_frame_nr() > self.overlay_frame) || ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
            self.overlay = None;
        }
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
        if self.script.is_some() {
            ctx.request_repaint();
            if let Some((path, true)) = self.script_png.take() {
                self.script_shot_pending.store(true, std::sync::atomic::Ordering::Relaxed);
                let flag = self.script_shot_pending.clone();
                ctx.request_screenshot(move |img| {
                    let [w, h] = img.size;
                    let bytes: Vec<u8> = img.pixels.iter().flat_map(|c| [c.r(), c.g(), c.b(), 255]).collect();
                    let _ = image::save_buffer(&path, &bytes, w as u32, h as u32, image::ExtendedColorType::Rgba8);
                    flag.store(false, std::sync::atomic::Ordering::Relaxed);
                });
            }
            if self.script_quit && !self.script_shot_pending.load(std::sync::atomic::Ordering::Relaxed) {
                ctx.send_viewport_cmd(ViewportCommand::Close);
            }
        }
        if !matches!(self.route, Route::Settings(1)) {
            self.theme_edit = None;
        }
        let want = self.wanted_palette(&ctx);
        if want != pal() {
            set_palette(want);
            style(&ctx);
        }
        if !self.applying && self.update.state() == crate::update::UpdateState::Applied {
            // The worker swapped the exe and spawned the new build; the tray's
            // quit usually beats this frame, but without a tray the window
            // still has to close so the engine releases the mouse.
            self.applying = true;
            ctx.send_viewport_cmd(ViewportCommand::Close);
        }
        if !self.applying && self.shot.is_none() && crate::win::tray_ready() && self.cfg.lock().unwrap().general.close_to_tray && ctx.input(|i| i.viewport().close_requested()) {
            ctx.send_viewport_cmd(ViewportCommand::CancelClose);
            self.hide_to_tray();
        }
        self.pump(&ctx);
        self.watch_ghub_db();
        if let Some(e) = self.obs.take_error() {
            self.say(e);
        }
        let win = ui.max_rect();
        ui.painter().rect_filled(win, CornerRadius::ZERO, pal().bg);
        match self.route {
            _ if self.script_editor.is_some() => self.script_overlay(ui, win),
            _ if self.sample_edit.is_some() => self.samples_overlay(ui, win),
            _ if self.asg.editor.is_some() => self.macro_overlay(ui, win),
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

    fn raw_input_hook(&mut self, ctx: &egui::Context, raw: &mut egui::RawInput) {
        if let Some(r) = &mut self.script {
            raw.events.clear();
            // The off-screen script window has no reliable OS focus; widget focus must not flap with it.
            raw.focused = true;
            ctx.request_repaint();
            if !self.script_shot_pending.load(std::sync::atomic::Ordering::Relaxed) {
                let f = r.next_frame();
                raw.events.extend(f.events);
                self.script_png = f.shot.map(|p| (p, true)).or(self.script_png.take());
                self.script_quit |= f.quit;
            }
        }
        // Screenshots must not depend on where the real cursor is or on the clock.
        if self.shot.is_some() {
            raw.events.clear();
            let after_ready = self.shot.as_ref().and_then(|s| s.ready_at.map(|r| s.frames.saturating_sub(r)));
            raw.time = Some(1000.0 + after_ready.unwrap_or(0) as f64 * 0.05);
        }
    }

    fn on_exit(&mut self) {
        self.update.quit();
        self.exit.run();
    }
}
