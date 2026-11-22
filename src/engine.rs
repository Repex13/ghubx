//! Host-side engine: owns the "who controls the mouse" policy (G HUB vs
//! GHUB-X), resolves the active profile (persistent / per-app / selected),
//! and turns 0x8110 button bitmaps into actions.

use crate::catalog::mod_vks;
use crate::device::{DeviceState, RATES_MS, SLOTS};
use crate::profile::*;
use crate::win;
use crossbeam_channel::{Receiver, Sender, select, tick};
use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

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
    TestPerfect180,
    /// Pick the active on-board slot (only while not in host control).
    OnboardSlot(u8),
    Quit,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Status {
    pub connected: bool,
    pub controlled: bool,
    pub ghub_running: bool,
    pub active: (usize, usize),
    pub foreground: Option<String>,
    pub gshift: bool,
}

#[derive(Clone, Debug)]
pub enum UiEvent {
    Device(DeviceState),
    Buttons(u16),
    Status(Status),
    Log(String),
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
        }
    }

    pub fn run(mut self, rx: Receiver<EngineMsg>) {
        win::start_scroll_hook();
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
                    Ok(EngineMsg::ControlLost) => {
                        if self.applied.is_some() {
                            self.st.ghub_running = win::ghub_running();
                            self.drop_control_state();
                            self.retry_at = Some(Instant::now() + Duration::from_secs(1));
                            self.send_status();
                        }
                    }
                    Ok(EngineMsg::ConfigChanged) => self.reconcile(),
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
                        if !self.st.controlled && self.st.connected && !self.st.ghub_running {
                            let _ = self.dev.send(DevCmd::SetOnboardProfile(slot));
                            let _ = self.dev.send(DevCmd::Refresh);
                        }
                    }
                    Ok(EngineMsg::TestPerfect180) => {
                        let e = self.cfg.lock().unwrap().extras.clone();
                        std::thread::spawn(move || perfect_180(e.p180_counts, e.p180_ms, e.p180_steps));
                    }
                    Ok(EngineMsg::Quit) | Err(_) => {
                        self.release_device();
                        let _ = self.dev.send(DevCmd::Quit);
                        return;
                    }
                },
                recv(ticker) -> _ => {
                    n = n.wrapping_add(1);
                    let fg = win::foreground_exe();
                    let mut dirty = false;
                    if fg != self.st.foreground {
                        self.st.foreground = fg;
                        dirty = true;
                    }
                    if n.is_multiple_of(8) {
                        let g = win::ghub_running();
                        dirty |= g != self.st.ghub_running;
                        self.st.ghub_running = g;
                    }
                    dirty |= self.check_sleep();
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
    }

    fn release_device(&mut self) {
        if self.applied.is_some() {
            self.release_all();
            let _ = self.dev.send(DevCmd::Release);
        }
        self.applied = None;
        self.st.controlled = false;
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
        let active = cfg.resolve(self.st.foreground.as_deref());
        if active != self.st.active {
            self.release_all();
            self.st.active = active;
        }
        let want = self.st.connected && cfg.general.take_control && !self.st.ghub_running && self.retry_at.is_none();
        if want {
            let Some(p) = cfg.profile_at(active) else { return };
            let leds = if self.asleep {
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
        let stop = self.start(&a, passthrough, &macros, &extras);
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
                    win::mouse(b, false);
                }
            }
            Assignment::Key(k) | Assignment::Shortcut { combo: k, .. } => {
                win::key(k.vk, false);
                for vk in mod_vks(k.mods).into_iter().rev() {
                    win::key(vk, false);
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
    fn start(&mut self, a: &Assignment, passthrough: bool, macros: &[Macro], extras: &Extras) -> Option<Arc<AtomicBool>> {
        match a {
            Assignment::Default | Assignment::Disabled => None,
            Assignment::Mouse(m) => match m {
                _ if passthrough => None,
                MouseFn::DoubleClick => {
                    std::thread::spawn(|| {
                        for _ in 0..2 {
                            win::mouse(0, true);
                            win::mouse(0, false);
                            std::thread::sleep(Duration::from_millis(30));
                        }
                    });
                    None
                }
                MouseFn::WheelUp | MouseFn::WheelDown | MouseFn::WheelLeft | MouseFn::WheelRight => {
                    let m = *m;
                    let stop = Arc::new(AtomicBool::new(false));
                    let s = stop.clone();
                    std::thread::spawn(move || {
                        let notch = || match m {
                            MouseFn::WheelUp => win::wheel(120),
                            MouseFn::WheelDown => win::wheel(-120),
                            MouseFn::WheelLeft => win::hwheel(-120),
                            _ => win::hwheel(120),
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
                        win::mouse(b, true);
                    }
                    None
                }
            },
            Assignment::Key(k) | Assignment::Shortcut { combo: k, .. } => {
                for vk in mod_vks(k.mods) {
                    win::key(vk, true);
                }
                win::key(k.vk, true);
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
                        std::thread::spawn(move || play(&m.steps, None));
                        None
                    }
                    MacroKind::RepeatWhileHolding => {
                        let stop = Arc::new(AtomicBool::new(false));
                        let s = stop.clone();
                        std::thread::spawn(move || play(&m.steps, Some(&s)));
                        Some(stop)
                    }
                    MacroKind::Toggle => {
                        if let Some(f) = self.toggles.remove(&m.id) {
                            f.store(true, Ordering::Relaxed);
                        } else {
                            let stop = Arc::new(AtomicBool::new(false));
                            self.toggles.insert(m.id.clone(), stop.clone());
                            std::thread::spawn(move || play(&m.steps, Some(&stop)));
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
                let c = if *left { -extras.p180_counts } else { extras.p180_counts };
                let (ms, steps) = (extras.p180_ms, extras.p180_steps);
                std::thread::spawn(move || perfect_180(c, ms, steps));
                None
            }
            Assignment::ToggleScrollAccel => {
                let on = {
                    let mut cfg = self.cfg.lock().unwrap();
                    cfg.extras.scroll_accel = !cfg.extras.scroll_accel;
                    let _ = cfg.save();
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
                    let _ = cfg.save();
                }
                self.reconcile();
            }
        }
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
            let _ = cfg.save();
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
fn play(steps: &[MacroStep], stop: Option<&AtomicBool>) {
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
            win::key(s.vk, s.down);
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
        win::key(vk, false);
    }
}

/// Basilisk V4 "Perfect 180": inject `counts` of horizontal motion spread
/// evenly over `steps` reports across `ms` milliseconds.
pub fn perfect_180(counts: i32, ms: u16, steps: u8) {
    // keep each report under ~1000 counts so games that clamp per-report motion keep up
    let steps = (steps.max(1) as i64).max((counts.unsigned_abs() as u64).div_ceil(1000) as i64).min(200);
    let mut sent = 0i64;
    let gap = ms as f64 / steps as f64;
    for i in 1..=steps {
        let target = counts as i64 * i / steps;
        win::mouse_move((target - sent) as i32, 0);
        sent = target;
        if gap >= 1.0 && i < steps {
            std::thread::sleep(Duration::from_secs_f64(gap / 1000.0));
        }
    }
}
