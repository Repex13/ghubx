//! Host-side engine: owns the "who controls the mouse" policy (G HUB vs
//! GHUB-X), resolves the active profile (persistent / per-app / selected),
//! and turns 0x8110 button bitmaps into actions.

use crate::catalog::mod_vks;
use crate::device::{DeviceState, RATES_MS, SLOTS};
use crate::profile::*;
use crate::osd::{self, OsdKind};
use crate::win;
use crossbeam_channel::{Receiver, Sender, select, tick};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Where the engine sends synthesized input (the real thing, or a test sink).
pub trait Input: Send + Sync {
    /// Keyboard keys and mouse-button VKs (0x01, 0x02, 0x04, 0x05, 0x06).
    fn key(&self, vk: u16, down: bool);
    fn mouse(&self, btn: u8, down: bool);
    fn mouse_move(&self, dx: i32, dy: i32);
    fn wheel(&self, delta: i32);
    fn hwheel(&self, delta: i32);
}

pub struct RealInput;

impl Input for RealInput {
    fn key(&self, vk: u16, down: bool) {
        win::key(vk, down);
    }
    fn mouse(&self, btn: u8, down: bool) {
        win::mouse(btn, down);
    }
    fn mouse_move(&self, dx: i32, dy: i32) {
        win::mouse_move(dx, dy);
    }
    fn wheel(&self, delta: i32) {
        win::wheel(delta);
    }
    fn hwheel(&self, delta: i32) {
        win::hwheel(delta);
    }
}

static OBSERVER: AtomicBool = AtomicBool::new(false);

/// Observer mode (`--dev`, `--shot`): never take control or touch G HUB.
pub fn set_observer(on: bool) {
    OBSERVER.store(on, Ordering::Relaxed);
    win::set_passive(on);
}

fn observer() -> bool {
    OBSERVER.load(Ordering::Relaxed)
}

/// What GHUB-X pushes to the mouse while it holds host mode.
#[derive(Clone, Debug, PartialEq)]
pub struct Applied {
    pub map: [u8; 16],
    pub dpi: u16,
    pub rate_ms: u8,
    pub leds: [[u8; 11]; 2],
}

/// Commands for the device IO thread.
#[derive(Debug)]
pub enum DevCmd {
    TakeControl(Applied),
    Release,
    SetDpi(u16),
    SetRate(u8),
    SetMap([u8; 16]),
    SetZone { zone: u8, led: [u8; 11] },
    SetOnboardProfile(u8),
    Refresh,
    /// Read the mouse's on-board profile memory (read-only), one sector per IO pass.
    ReadOnboard,
    Quit,
}

/// Messages into the engine (from IO and UI).
#[derive(Debug)]
pub enum EngineMsg {
    Spy(u16),
    Connected(bool),
    ControlResult(bool),
    /// The mouse left host mode behind our back (G HUB or a power cycle).
    ControlLost,
    ConfigChanged,
    CloseGhub,
    StartGhub,
    /// Inject a test turn of exactly this many counts.
    TestPerfect180 { counts: i32 },
    /// Play these macro steps once (macro editor TEST button).
    TestMacro(Vec<MacroStep>),
    /// Pick the active on-board slot (only while not in host control).
    OnboardSlot(u8),
    /// Ask the device thread to read the on-board profiles.
    ReadOnboard,
    Quit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub connected: bool,
    pub controlled: bool,
    pub ghub_running: bool,
    pub active: (usize, usize),
    /// File name of the foreground process (for the UI).
    pub foreground: Option<String>,
    /// Full path of the foreground process (for profile matching).
    pub foreground_path: Option<String>,
    pub gshift: bool,
}

#[derive(Clone, Debug)]
pub enum UiEvent {
    Device(DeviceState),
    Buttons(u16),
    Status(Status),
    Log(String),
    Onboard(Vec<OnboardSlot>),
}

struct Held {
    a: Assignment,
    passthrough: bool,
    stop: Option<Arc<AtomicBool>>,
}

pub struct Engine {
    cfg: Arc<Mutex<Config>>,
    dev: Sender<DevCmd>,
    ui: Sender<UiEvent>,
    st: Status,
    applied: Option<Applied>,
    retry_at: Option<Instant>,
    mask: u16,
    dpi_shift: bool,
    held: Vec<Option<Held>>,
    toggles: HashMap<String, Arc<AtomicBool>>,
    asleep: bool,
    pub(crate) osd: Box<dyn Fn(OsdKind) + Send>,
    pub(crate) persist: bool,
    pub(crate) flash_until: Option<Instant>,
    pub(crate) input: Arc<dyn Input>,
}

impl Engine {
    pub fn new(cfg: Arc<Mutex<Config>>, dev: Sender<DevCmd>, ui: Sender<UiEvent>) -> Self {
        Self {
            cfg,
            dev,
            ui,
            st: Status::default(),
            applied: None,
            retry_at: None,
            mask: 0,
            dpi_shift: false,
            held: (0..SLOTS).map(|_| None).collect(),
            toggles: HashMap::new(),
            asleep: false,
            osd: Box::new(osd::show),
            persist: true,
            flash_until: None,
            input: Arc::new(RealInput),
        }
    }

    pub fn run(mut self, rx: Receiver<EngineMsg>) {
        let _ = win::start_scroll_hook();
        let ticker = tick(Duration::from_millis(250));
        let mut n: u32 = 0;
        self.st.ghub_running = win::ghub_running();
        self.reconcile();
        loop {
            select! {
                recv(rx) -> m => match m {
                    Ok(EngineMsg::Spy(mask)) => self.on_mask(mask),
                    Ok(EngineMsg::Connected(c)) => {
                        self.st.connected = c;
                        self.drop_control_state();
                        self.reconcile();
                    }
                    Ok(EngineMsg::ControlResult(ok)) => {
                        if !ok && self.applied.is_some() {
                            self.drop_control_state();
                            self.retry_at = Some(Instant::now() + Duration::from_secs(5));
                            self.send_status();
                        }
                    }
                    Ok(EngineMsg::ControlLost) => self.on_control_lost(),
                    Ok(EngineMsg::ConfigChanged) => self.reconcile(),
                    Ok(EngineMsg::CloseGhub | EngineMsg::StartGhub) if observer() => self.log("G HUB control ignored in dev mode".into()),
                    Ok(EngineMsg::CloseGhub) => {
                        let n = win::close_ghub();
                        self.log(format!("Closed {n} G HUB process(es)"));
                        std::thread::sleep(Duration::from_millis(400));
                        self.st.ghub_running = win::ghub_running();
                        // G HUB leaves the mouse in host mode with its buttons muted:
                        // take over now, or hand the mouse back to its onboard profile.
                        let take = self.cfg.lock().unwrap().general.take_control;
                        if !take && self.st.connected && !self.st.ghub_running {
                            let _ = self.dev.send(DevCmd::Release);
                        }
                        self.reconcile();
                    }
                    Ok(EngineMsg::StartGhub) => {
                        self.release_device();
                        win::start_ghub();
                        self.st.ghub_running = true;
                        self.send_status();
                    }
                    Ok(EngineMsg::OnboardSlot(slot)) => {
                        if !observer() && !self.st.controlled && self.st.connected && !self.st.ghub_running {
                            let _ = self.dev.send(DevCmd::SetOnboardProfile(slot));
                            let _ = self.dev.send(DevCmd::Refresh);
                        }
                    }
                    Ok(EngineMsg::ReadOnboard) => {
                        if self.st.connected {
                            let _ = self.dev.send(DevCmd::ReadOnboard);
                        }
                    }
                    Ok(EngineMsg::TestMacro(steps)) => self.test_macro(steps),
                    Ok(EngineMsg::TestPerfect180 { counts }) => {
                        let e = self.cfg.lock().unwrap().extras.clone();
                        let input = self.input.clone();
                        std::thread::spawn(move || perfect_180(&*input, counts, e.p180_ms, e.p180_steps));
                    }
                    Ok(EngineMsg::Quit) | Err(_) => {
                        self.release_device();
                        let _ = self.dev.send(DevCmd::Quit);
                        return;
                    }
                },
                recv(ticker) -> _ => {
                    n = n.wrapping_add(1);
                    let path = win::foreground_path();
                    let mut dirty = false;
                    if path != self.st.foreground_path {
                        self.st.foreground = path.as_deref().and_then(|p| p.rsplit(['\\', '/']).next()).map(str::to_string);
                        self.st.foreground_path = path;
                        dirty = true;
                    }
                    if n.is_multiple_of(8) {
                        let g = win::ghub_running();
                        dirty |= g != self.st.ghub_running;
                        self.st.ghub_running = g;
                    }
                    dirty |= self.check_sleep();
                    dirty |= self.expire_flash();
                    if self.retry_at.is_some_and(|t| Instant::now() >= t) {
                        self.retry_at = None;
                        dirty = true;
                    }
                    if dirty {
                        self.reconcile();
                    }
                }
            }
        }
    }

    fn test_macro(&self, steps: Vec<MacroStep>) {
        let input = self.input.clone();
        std::thread::spawn(move || play(&*input, &steps, None));
    }

    fn on_control_lost(&mut self) {
        if self.applied.is_some() {
            self.st.ghub_running = win::ghub_running();
            self.drop_control_state();
            self.retry_at = Some(Instant::now() + Duration::from_secs(1));
            self.send_status();
        }
    }

    fn log(&self, s: String) {
        let _ = self.ui.send(UiEvent::Log(s));
    }

    fn send_status(&self) {
        let _ = self.ui.send(UiEvent::Status(self.st.clone()));
    }

    fn drop_control_state(&mut self) {
        self.release_all();
        self.applied = None;
        self.st.controlled = false;
        self.mask = 0;
        self.flash_until = None;
    }

    fn release_device(&mut self) {
        if self.applied.is_some() {
            self.release_all();
            let _ = self.dev.send(DevCmd::Release);
        }
        self.applied = None;
        self.st.controlled = false;
        self.flash_until = None;
    }

    fn save_cfg(&self, cfg: &Config) {
        if self.persist {
            let _ = cfg.save();
        }
    }

    fn check_sleep(&mut self) -> bool {
        let mins = self.cfg.lock().unwrap().general.lighting_sleep_min;
        let idle = win::idle_ms();
        let sleep = mins > 0 && idle > mins * 60_000;
        let changed = sleep != self.asleep;
        self.asleep = sleep;
        changed
    }

    /// Bring the device in line with config + environment.
    fn reconcile(&mut self) {
        let cfg = self.cfg.lock().unwrap().clone();
        win::set_scroll_accel(cfg.extras.scroll_accel, cfg.extras.scroll_boost);
        let active = cfg.resolve(self.st.foreground_path.as_deref());
        if active != self.st.active {
            let switched_in_control = self.applied.is_some();
            self.release_all();
            self.st.active = active;
            if switched_in_control
                && cfg.general.osd_profile
                && let (Some(app), Some(p)) = (cfg.apps.get(active.0), cfg.profile_at(active))
            {
                (self.osd)(OsdKind::Profile { name: format!("{}: {}", app.name, p.name) });
            }
        }
        let want = !observer() && self.st.connected && cfg.general.take_control && !self.st.ghub_running && self.retry_at.is_none();
        if want {
            let Some(p) = cfg.profile_at(active) else { return };
            let leds = if self.flash_until.is_some() && self.applied.is_some() {
                self.applied.as_ref().map_or([[0u8; 11]; 2], |a| a.leds)
            } else if self.asleep {
                [[0u8; 11]; 2]
            } else {
                [p.lighting[0].to_led(), p.lighting[1].to_led()]
            };
            let target = Applied {
                map: p.spy_map(),
                dpi: if self.dpi_shift { p.dpi.shift } else { p.dpi.active },
                rate_ms: p.rate_ms,
                leds,
            };
            match &self.applied {
                None => {
                    let _ = self.dev.send(DevCmd::TakeControl(target.clone()));
                }
                Some(old) => {
                    if old.map != target.map {
                        let _ = self.dev.send(DevCmd::SetMap(target.map));
                    }
                    if old.dpi != target.dpi {
                        let _ = self.dev.send(DevCmd::SetDpi(target.dpi));
                    }
                    if old.rate_ms != target.rate_ms {
                        let _ = self.dev.send(DevCmd::SetRate(target.rate_ms));
                    }
                    for z in 0..2 {
                        if old.leds[z] != target.leds[z] {
                            let _ = self.dev.send(DevCmd::SetZone { zone: z as u8, led: target.leds[z] });
                        }
                    }
                }
            }
            self.applied = Some(target);
            self.st.controlled = true;
        } else if self.applied.is_some() {
            self.release_all();
            // If G HUB just started it is initialising the mouse itself; don't fight it.
            if self.st.connected && !self.st.ghub_running {
                let _ = self.dev.send(DevCmd::Release);
            }
            self.applied = None;
            self.st.controlled = false;
        }
        self.send_status();
    }

    fn profile(&self) -> Option<(Profile, Vec<Macro>, Extras)> {
        let cfg = self.cfg.lock().unwrap();
        let p = cfg.profile_at(self.st.active)?.clone();
        Some((p, cfg.apps[self.st.active.0].macros.clone(), cfg.extras.clone()))
    }

    fn on_mask(&mut self, mask: u16) {
        let changed = mask ^ self.mask;
        self.mask = mask;
        let _ = self.ui.send(UiEvent::Buttons(mask));
        if !self.st.controlled {
            return;
        }
        for slot in 0..SLOTS {
            let bit = 1u16 << slot;
            if changed & bit != 0 {
                if mask & bit != 0 { self.press(slot) } else { self.release(slot) }
            }
        }
    }

    fn press(&mut self, slot: usize) {
        let Some((p, macros, extras)) = self.profile() else { return };
        let base = p.effective(slot, false);
        let a = if base == Assignment::System(SystemFn::GShift) || !self.st.gshift { base } else { p.effective(slot, true) };
        let passthrough = self.applied.as_ref().is_some_and(|ap| ap.map[slot] != 0) && a == native(slot);
        let turn = turn_for(&p, &extras);
        let stop = self.start(&a, passthrough, &macros, &extras, &turn);
        self.held[slot] = Some(Held { a, passthrough, stop });
    }

    fn release(&mut self, slot: usize) {
        let Some(h) = self.held[slot].take() else { return };
        if let Some(s) = &h.stop {
            s.store(true, Ordering::Relaxed);
        }
        match h.a {
            Assignment::Mouse(m) if !h.passthrough => {
                if let Some(b) = click_button(m) {
                    self.input.mouse(b, false);
                }
            }
            Assignment::Key(k) | Assignment::Shortcut { combo: k, .. } => {
                self.input.key(k.vk, false);
                for vk in mod_vks(k.mods).into_iter().rev() {
                    self.input.key(vk, false);
                }
            }
            Assignment::System(SystemFn::DpiShift) => {
                self.dpi_shift = false;
                self.push_dpi();
            }
            Assignment::System(SystemFn::GShift) => {
                self.st.gshift = false;
                self.send_status();
            }
            _ => {}
        }
    }

    fn release_all(&mut self) {
        for slot in 0..SLOTS {
            self.release(slot);
        }
        for (_, f) in self.toggles.drain() {
            f.store(true, Ordering::Relaxed);
        }
        self.st.gshift = false;
        self.dpi_shift = false;
    }

    /// Start an action; returns a stop flag for actions that run while held.
    fn start(&mut self, a: &Assignment, passthrough: bool, macros: &[Macro], extras: &Extras, turn: &TurnCal) -> Option<Arc<AtomicBool>> {
        match a {
            Assignment::Default | Assignment::Disabled => None,
            Assignment::Mouse(m) => match m {
                _ if passthrough => None,
                MouseFn::DoubleClick => {
                    let input = self.input.clone();
                    std::thread::spawn(move || {
                        for _ in 0..2 {
                            input.mouse(0, true);
                            input.mouse(0, false);
                            std::thread::sleep(Duration::from_millis(30));
                        }
                    });
                    None
                }
                MouseFn::WheelUp | MouseFn::WheelDown | MouseFn::WheelLeft | MouseFn::WheelRight => {
                    let m = *m;
                    let stop = Arc::new(AtomicBool::new(false));
                    let s = stop.clone();
                    let input = self.input.clone();
                    std::thread::spawn(move || {
                        let notch = || match m {
                            MouseFn::WheelUp => input.wheel(120),
                            MouseFn::WheelDown => input.wheel(-120),
                            MouseFn::WheelLeft => input.hwheel(-120),
                            _ => input.hwheel(120),
                        };
                        notch();
                        std::thread::sleep(Duration::from_millis(350));
                        while !s.load(Ordering::Relaxed) {
                            notch();
                            std::thread::sleep(Duration::from_millis(60));
                        }
                    });
                    Some(stop)
                }
                _ => {
                    if let Some(b) = click_button(*m) {
                        self.input.mouse(b, true);
                    }
                    None
                }
            },
            Assignment::Key(k) | Assignment::Shortcut { combo: k, .. } => {
                for vk in mod_vks(k.mods) {
                    self.input.key(vk, true);
                }
                self.input.key(k.vk, true);
                None
            }
            Assignment::System(s) => {
                self.system(*s);
                None
            }
            Assignment::Macro(id) => {
                let m = macros.iter().find(|m| &m.id == id)?.clone();
                match m.kind {
                    MacroKind::NoRepeat => {
                        let input = self.input.clone();
                        std::thread::spawn(move || play(&*input, &m.steps, None));
                        None
                    }
                    MacroKind::RepeatWhileHolding => {
                        let stop = Arc::new(AtomicBool::new(false));
                        let s = stop.clone();
                        let input = self.input.clone();
                        std::thread::spawn(move || play(&*input, &m.steps, Some(&s)));
                        Some(stop)
                    }
                    MacroKind::Toggle => {
                        if let Some(f) = self.toggles.remove(&m.id) {
                            f.store(true, Ordering::Relaxed);
                        } else {
                            let stop = Arc::new(AtomicBool::new(false));
                            self.toggles.insert(m.id.clone(), stop.clone());
                            let input = self.input.clone();
                            std::thread::spawn(move || play(&*input, &m.steps, Some(&stop)));
                        }
                        None
                    }
                }
            }
            Assignment::Launch { path, .. } => {
                let _ = std::process::Command::new("cmd").args(["/C", "start", "", path]).spawn();
                None
            }
            Assignment::Perfect180 { left } => {
                let counts = turn_counts(turn.counts, turn.angle);
                let c = if *left { -counts } else { counts };
                let (ms, steps) = (extras.p180_ms, extras.p180_steps);
                let input = self.input.clone();
                std::thread::spawn(move || perfect_180(&*input, c, ms, steps));
                None
            }
            Assignment::ToggleScrollAccel => {
                let on = {
                    let mut cfg = self.cfg.lock().unwrap();
                    cfg.extras.scroll_accel = !cfg.extras.scroll_accel;
                    self.save_cfg(&cfg);
                    cfg.extras.scroll_accel
                };
                self.log(format!("Scroll acceleration {}", if on { "on" } else { "off" }));
                self.reconcile();
                None
            }
        }
    }

    fn system(&mut self, s: SystemFn) {
        match s {
            SystemFn::GShift => {
                self.st.gshift = true;
                self.send_status();
            }
            SystemFn::DpiShift => {
                self.dpi_shift = true;
                self.push_dpi();
            }
            SystemFn::DpiUp | SystemFn::DpiDown | SystemFn::DpiCycle | SystemFn::DpiDefault => {
                self.edit_active(|p| {
                    match s {
                        SystemFn::DpiUp => p.dpi.step(true),
                        SystemFn::DpiDown => p.dpi.step(false),
                        SystemFn::DpiCycle => p.dpi.cycle(),
                        _ => {
                            p.dpi.active = p.dpi.default;
                            p.dpi.active
                        }
                    };
                });
                self.dpi_feedback();
            }
            SystemFn::ReportRateCycle => self.edit_active(|p| {
                let i = RATES_MS.iter().position(|&r| r == p.rate_ms).unwrap_or(0);
                p.rate_ms = RATES_MS[(i + 1) % RATES_MS.len()];
            }),
            SystemFn::ProfileCycle | SystemFn::OnboardProfileCycle => {
                {
                    let mut cfg = self.cfg.lock().unwrap();
                    let a = &mut cfg.apps[self.st.active.0];
                    a.active_profile = (a.active_profile + 1) % a.profiles.len();
                    self.save_cfg(&cfg);
                }
                self.reconcile();
            }
        }
    }

    /// OSD and lighting flash after a DPI button.
    fn dpi_feedback(&mut self) {
        let (info, osd_on, flash_on) = {
            let cfg = self.cfg.lock().unwrap();
            let info = cfg.profile_at(self.st.active).map(|p| (p.dpi.active, p.dpi.levels.len().min(255) as u8, p.dpi.active_index()));
            (info, cfg.general.osd_dpi, cfg.general.dpi_flash)
        };
        let Some((value, levels, index)) = info else { return };
        if osd_on {
            (self.osd)(OsdKind::Dpi { value, levels, active: index as u8 });
        }
        if flash_on && !self.asleep && self.applied.is_some() {
            let led = Lighting { effect: Effect::Fixed, color: dpi_flash_color(index), ..Default::default() }.to_led();
            for zone in 0..2u8 {
                let _ = self.dev.send(DevCmd::SetZone { zone, led });
            }
            if let Some(ap) = &mut self.applied {
                ap.leds = [led; 2];
            }
            self.flash_until = Some(Instant::now() + Duration::from_millis(1200));
        }
    }

    /// True when a DPI lighting flash just ended (the profile's LEDs must be re-sent).
    fn expire_flash(&mut self) -> bool {
        if self.flash_until.is_some_and(|t| Instant::now() >= t) {
            self.flash_until = None;
            return true;
        }
        false
    }

    /// Send the current (shifted or active) DPI without a full reconcile.
    fn push_dpi(&mut self) {
        let dpi = {
            let cfg = self.cfg.lock().unwrap();
            let Some(p) = cfg.profile_at(self.st.active) else { return };
            if self.dpi_shift { p.dpi.shift } else { p.dpi.active }
        };
        if let Some(ap) = &mut self.applied
            && ap.dpi != dpi
        {
            ap.dpi = dpi;
            let _ = self.dev.send(DevCmd::SetDpi(dpi));
        }
    }

    fn edit_active(&mut self, f: impl FnOnce(&mut Profile)) {
        {
            let mut cfg = self.cfg.lock().unwrap();
            let (a, p) = self.st.active;
            if let Some(prof) = cfg.apps.get_mut(a).and_then(|x| x.profiles.get_mut(p)) {
                f(prof);
            }
            self.save_cfg(&cfg);
        }
        self.reconcile();
    }
}

fn click_button(m: MouseFn) -> Option<u8> {
    Some(match m {
        MouseFn::Primary => 0,
        MouseFn::Secondary => 1,
        MouseFn::Middle => 2,
        MouseFn::Back => 3,
        MouseFn::Forward => 4,
        _ => return None,
    })
}

/// Play macro steps once, or loop until `stop` is set. Keys still held when
/// stopped are released.
fn play(input: &dyn Input, steps: &[MacroStep], stop: Option<&AtomicBool>) {
    let mut down: Vec<u16> = Vec::new();
    loop {
        let mut slept = 0;
        for s in steps {
            if s.delay_ms > 0 {
                std::thread::sleep(Duration::from_millis(s.delay_ms as u64));
                slept += s.delay_ms;
            }
            if s.vk == 0 {
                continue; // delay-only step
            }
            input.key(s.vk, s.down);
            if s.down {
                down.push(s.vk);
            } else {
                down.retain(|&v| v != s.vk);
            }
        }
        match stop {
            Some(f) if !f.load(Ordering::Relaxed) => {
                if slept == 0 {
                    std::thread::sleep(Duration::from_millis(10));
                }
            }
            _ => break,
        }
    }
    for vk in down {
        input.key(vk, false);
    }
}

/// Basilisk V4 "Perfect 180": inject `counts` of horizontal motion spread
/// evenly over `steps` reports across `ms` milliseconds.
pub fn perfect_180(input: &dyn Input, counts: i32, ms: u16, steps: u8) {
    // keep each report under ~1000 counts so games that clamp per-report motion keep up
    let steps = (steps.max(1) as i64).max((counts.unsigned_abs() as u64).div_ceil(1000) as i64).min(200);
    let mut sent = 0i64;
    let gap = ms as f64 / steps as f64;
    for i in 1..=steps {
        let target = counts as i64 * i / steps;
        input.mouse_move((target - sent) as i32, 0);
        sent = target;
        if gap >= 1.0 && i < steps {
            std::thread::sleep(Duration::from_secs_f64(gap / 1000.0));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crossbeam_channel::unbounded;
    use std::sync::MutexGuard;

    /// The observer flag is process-wide, so engine tests run one at a time.
    static SERIAL: Mutex<()> = Mutex::new(());

    #[derive(Clone, Debug, PartialEq)]
    enum Ev {
        Key(u16, bool),
        Mouse(u8, bool),
        Move(i32, i32),
        Wheel(i32),
        HWheel(i32),
    }

    #[derive(Default)]
    struct Rec {
        ev: Mutex<Vec<(Instant, Ev)>>,
    }

    impl Rec {
        fn push(&self, e: Ev) {
            self.ev.lock().unwrap().push((Instant::now(), e));
        }
        fn events(&self) -> Vec<Ev> {
            self.ev.lock().unwrap().iter().map(|(_, e)| e.clone()).collect()
        }
    }

    impl Input for Rec {
        fn key(&self, vk: u16, down: bool) {
            self.push(Ev::Key(vk, down));
        }
        fn mouse(&self, btn: u8, down: bool) {
            self.push(Ev::Mouse(btn, down));
        }
        fn mouse_move(&self, dx: i32, dy: i32) {
            self.push(Ev::Move(dx, dy));
        }
        fn wheel(&self, delta: i32) {
            self.push(Ev::Wheel(delta));
        }
        fn hwheel(&self, delta: i32) {
            self.push(Ev::HWheel(delta));
        }
    }

    struct Rig {
        rec: Arc<Rec>,
        e: Engine,
        dev_rx: Receiver<DevCmd>,
        osd: Arc<Mutex<Vec<OsdKind>>>,
        cfg: Arc<Mutex<Config>>,
        _ui_rx: Receiver<UiEvent>,
        _guard: MutexGuard<'static, ()>,
    }

    impl Drop for Rig {
        fn drop(&mut self) {
            set_observer(false);
        }
    }

    fn rig(edit: impl FnOnce(&mut Config)) -> Rig {
        let guard = SERIAL.lock().unwrap_or_else(|e| e.into_inner());
        set_observer(false);
        let mut cfg = Config::default();
        cfg.apps[0].profiles[0].dpi = DpiTable { levels: vec![600, 1000, 1600], active: 600, default: 600, shift: 400 };
        edit(&mut cfg);
        let cfg = Arc::new(Mutex::new(cfg));
        let (dev_tx, dev_rx) = unbounded();
        let (ui_tx, ui_rx) = unbounded();
        let mut e = Engine::new(cfg.clone(), dev_tx, ui_tx);
        let osd = Arc::new(Mutex::new(Vec::new()));
        let sink = osd.clone();
        e.osd = Box::new(move |k| sink.lock().unwrap().push(k));
        e.persist = false;
        let rec = Arc::new(Rec::default());
        e.input = rec.clone();
        e.st.connected = true;
        e.st.ghub_running = false;
        Rig { rec, e, dev_rx, osd, cfg, _ui_rx: ui_rx, _guard: guard }
    }

    impl Rig {
        fn cmds(&self) -> Vec<DevCmd> {
            self.dev_rx.try_iter().collect()
        }

        fn take_control(&mut self) {
            self.e.reconcile();
            assert!(matches!(self.cmds().first(), Some(DevCmd::TakeControl(_))));
        }

        fn press(&mut self, slot: usize) {
            self.e.on_mask(1 << slot);
            self.e.on_mask(0);
        }
    }

    fn pause(ms: u64) {
        std::thread::sleep(Duration::from_millis(ms));
    }

    fn base(c: &mut Config, slot: usize, a: Assignment) {
        c.apps[0].profiles[0].base[slot] = a;
    }

    fn macro_of(id: &str, kind: MacroKind, steps: &[(u16, bool, u32)]) -> Macro {
        Macro { id: id.into(), name: id.into(), kind, steps: steps.iter().map(|&(vk, down, delay_ms)| MacroStep { vk, down, delay_ms }).collect() }
    }

    fn moves(r: &Rig) -> Vec<(i32, i32)> {
        r.rec.events().into_iter().filter_map(|e| if let Ev::Move(x, y) = e { Some((x, y)) } else { None }).collect()
    }

    #[test]
    fn key_assignment_presses_and_releases() {
        let mut r = rig(|c| base(c, 5, Assignment::Key(KeyCombo { vk: 0x53, mods: 0 })));
        r.take_control();
        r.e.on_mask(1 << 5);
        assert_eq!(r.rec.events(), vec![Ev::Key(0x53, true)]);
        r.e.on_mask(0);
        assert_eq!(r.rec.events(), vec![Ev::Key(0x53, true), Ev::Key(0x53, false)]);
    }

    #[test]
    fn shortcut_orders_modifiers() {
        let combo = KeyCombo { vk: 0x1B, mods: MOD_CTRL | MOD_SHIFT };
        let mut r = rig(|c| base(c, 5, Assignment::Shortcut { name: "x".into(), combo }));
        r.take_control();
        r.e.on_mask(1 << 5);
        assert_eq!(r.rec.events(), vec![Ev::Key(0xA2, true), Ev::Key(0xA0, true), Ev::Key(0x1B, true)]);
        r.e.on_mask(0);
        assert_eq!(r.rec.events()[3..], [Ev::Key(0x1B, false), Ev::Key(0xA0, false), Ev::Key(0xA2, false)]);
    }

    #[test]
    fn native_clicks_pass_through() {
        let mut r = rig(|_| {});
        r.take_control();
        r.press(0);
        assert!(r.rec.events().is_empty());
        assert!(r.cmds().is_empty());
    }

    fn alternates_down_up(ev: &[Ev], vk: u16) -> usize {
        for (i, e) in ev.iter().enumerate() {
            assert_eq!(*e, Ev::Key(vk, i % 2 == 0), "event {i} in {ev:?}");
        }
        ev.len() / 2
    }

    #[test]
    fn repeat_macro_runs_while_held_and_releases_keys() {
        let m = macro_of("jump", MacroKind::RepeatWhileHolding, &[(0x20, true, 0), (0x20, false, 50), (0, false, 50)]);
        let mut r = rig(|c| {
            c.apps[0].macros.push(m);
            base(c, 3, Assignment::Macro("jump".into()));
        });
        r.take_control();
        r.e.on_mask(1 << 3);
        pause(280);
        r.e.on_mask(0);
        pause(250);
        let ev = r.rec.events();
        assert!(alternates_down_up(&ev, 0x20) >= 2, "{ev:?}");
        assert_eq!(ev.last(), Some(&Ev::Key(0x20, false)));
        pause(200);
        assert_eq!(r.rec.events().len(), ev.len());
    }

    #[test]
    fn toggle_macro_starts_and_stops() {
        let m = macro_of("ping", MacroKind::Toggle, &[(0x04, true, 0), (0x04, false, 50), (0, false, 50)]);
        let mut r = rig(|c| {
            c.apps[0].macros.push(m);
            base(c, 3, Assignment::Macro("ping".into()));
        });
        r.take_control();
        r.press(3);
        pause(160);
        r.press(3);
        pause(250);
        let ev = r.rec.events();
        assert!(alternates_down_up(&ev, 0x04) >= 1, "{ev:?}");
        assert_eq!(ev.last(), Some(&Ev::Key(0x04, false)));
        pause(200);
        assert_eq!(r.rec.events().len(), ev.len());
    }

    #[test]
    fn tilt_scroll_repeats_while_held() {
        let mut r = rig(|_| {});
        r.take_control();
        r.e.on_mask(1 << 9);
        pause(20);
        assert_eq!(r.rec.events(), vec![Ev::HWheel(120)]);
        pause(500);
        assert!(r.rec.events().len() >= 3, "{:?}", r.rec.events());
        r.e.on_mask(0);
        pause(150);
        let n = r.rec.events().len();
        pause(200);
        assert_eq!(r.rec.events().len(), n);
    }

    #[test]
    fn gshift_layer_switches_assignment() {
        let mut r = rig(|c| {
            base(c, 5, Assignment::System(SystemFn::GShift));
            base(c, 3, Assignment::Key(KeyCombo { vk: 0x42, mods: 0 }));
            c.apps[0].profiles[0].gshift[3] = Assignment::Key(KeyCombo { vk: 0x41, mods: 0 });
        });
        r.take_control();
        r.e.on_mask(1 << 5);
        r.e.on_mask((1 << 5) | (1 << 3));
        assert_eq!(r.rec.events(), vec![Ev::Key(0x41, true)]);
        r.e.on_mask(1 << 5);
        assert_eq!(r.rec.events().last(), Some(&Ev::Key(0x41, false)));
        r.e.on_mask(0);
        r.e.on_mask(1 << 3);
        assert_eq!(r.rec.events().last(), Some(&Ev::Key(0x42, true)));
    }

    #[test]
    fn perfect_180_injects_exact_counts() {
        for (left, angle, want) in [(false, 180u16, 8182), (true, 180, -8182), (false, 90, 4091)] {
            let mut r = rig(|c| {
                base(c, 5, Assignment::Perfect180 { left });
                c.extras.p180_counts = 8182;
                c.extras.p180_angle = angle;
                c.extras.p180_ms = 20;
                c.extras.p180_steps = 12;
            });
            r.take_control();
            r.press(5);
            pause(200);
            let m = moves(&r);
            assert_eq!(m.iter().map(|m| m.0).sum::<i32>(), want, "{m:?}");
            assert!(m.iter().all(|m| m.0.abs() <= 1000 && m.1 == 0), "{m:?}");
        }
    }

    #[test]
    fn dpi_shift_applies_while_held() {
        let mut r = rig(|_| {});
        r.take_control();
        r.e.on_mask(1 << 5);
        assert!(r.cmds().iter().any(|c| matches!(c, DevCmd::SetDpi(400))));
        r.e.on_mask(0);
        assert!(r.cmds().iter().any(|c| matches!(c, DevCmd::SetDpi(600))));
    }

    #[test]
    fn held_keys_are_released_when_control_is_lost() {
        let mut r = rig(|c| base(c, 5, Assignment::Key(KeyCombo { vk: 0x53, mods: 0 })));
        r.take_control();
        r.e.on_mask(1 << 5);
        r.e.on_control_lost();
        assert_eq!(r.rec.events().last(), Some(&Ev::Key(0x53, false)));
    }

    #[test]
    fn test_macro_plays_once_through_the_sink() {
        let mut r = rig(|_| {});
        r.take_control();
        r.e.test_macro(vec![MacroStep { vk: 0x41, down: true, delay_ms: 0 }, MacroStep { vk: 0x41, down: false, delay_ms: 30 }]);
        pause(150);
        assert_eq!(r.rec.events(), vec![Ev::Key(0x41, true), Ev::Key(0x41, false)]);
    }

    #[test]
    fn observer_leaves_the_wheel_alone() {
        let mut r = rig(|c| c.extras.scroll_accel = true);
        set_observer(true);
        r.e.reconcile();
        assert!(!win::scroll_accel_enabled());
        assert!(!win::start_scroll_hook());
    }

    #[test]
    fn disabled_button_does_nothing() {
        let mut r = rig(|c| base(c, 3, Assignment::Disabled));
        r.take_control();
        r.press(3);
        assert!(r.rec.events().is_empty());
    }

    fn fixed_led(color: [u8; 3]) -> [u8; 11] {
        Lighting { effect: Effect::Fixed, color, ..Default::default() }.to_led()
    }

    #[test]
    fn dpi_up_button_steps_dpi_and_shows_osd() {
        let mut r = rig(|_| {});
        r.take_control();
        r.press(7);
        assert!(r.cmds().iter().any(|c| matches!(c, DevCmd::SetDpi(1000))));
        assert_eq!(r.cfg.lock().unwrap().apps[0].profiles[0].dpi.active, 1000);
        assert_eq!(*r.osd.lock().unwrap(), vec![OsdKind::Dpi { value: 1000, levels: 3, active: 1 }]);
    }

    #[test]
    fn osd_respects_settings() {
        let mut r = rig(|c| c.general.osd_dpi = false);
        r.take_control();
        r.press(7);
        assert!(r.cmds().iter().any(|c| matches!(c, DevCmd::SetDpi(1000))));
        assert!(r.osd.lock().unwrap().is_empty());
    }

    #[test]
    fn profile_cycle_shows_profile_osd() {
        let mut r = rig(|c| c.apps[0].profiles.push(Profile::new("Second")));
        r.take_control();
        r.press(8);
        assert_eq!(r.cfg.lock().unwrap().apps[0].active_profile, 1);
        assert_eq!(*r.osd.lock().unwrap(), vec![OsdKind::Profile { name: "Desktop: Second".into() }]);
    }

    #[test]
    fn dpi_flash_sets_zones_then_restores() {
        let mut r = rig(|c| c.general.dpi_flash = true);
        r.take_control();
        r.press(7);
        let zones: Vec<(u8, [u8; 11])> = r.cmds().into_iter().filter_map(|c| if let DevCmd::SetZone { zone, led } = c { Some((zone, led)) } else { None }).collect();
        assert_eq!(zones, vec![(0, fixed_led([0x34, 0xC7, 0x59])), (1, fixed_led([0x34, 0xC7, 0x59]))]);
        assert!(r.e.flash_until.is_some());
        r.e.flash_until = Some(Instant::now() - Duration::from_millis(1));
        assert!(r.e.expire_flash());
        r.e.reconcile();
        let own: Vec<(u8, [u8; 11])> = r.cmds().into_iter().filter_map(|c| if let DevCmd::SetZone { zone, led } = c { Some((zone, led)) } else { None }).collect();
        let cfg = r.cfg.lock().unwrap();
        let p = &cfg.apps[0].profiles[0];
        assert_eq!(own, vec![(0, p.lighting[0].to_led()), (1, p.lighting[1].to_led())]);
    }

    #[test]
    fn observer_never_takes_control() {
        let mut r = rig(|_| {});
        set_observer(true);
        r.e.reconcile();
        assert!(r.cmds().is_empty());
    }
}
