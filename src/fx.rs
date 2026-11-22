//! Host-driven lighting effects (SCREEN SAMPLER, AUDIO VISUALIZER): a thread computes a colour
//! from the screen or from the audio the PC plays and hands it to the IO thread through a
//! `Mailbox`. The IO thread writes it as a RAM-only fixed colour (never to flash).

use crate::profile::{Effect, Lighting, ScreenArea};
use std::collections::HashMap;
use std::sync::{Arc, Condvar, Mutex};
use std::time::{Duration, Instant};

// ---------------------------------------------------------------- colour maths

/// Standard (gamma-space) HSV to sRGB bytes; `h`, `s`, `v` in 0..=1.
pub fn hsv_to_rgb(h: f32, s: f32, v: f32) -> [u8; 3] {
    let h6 = h.rem_euclid(1.0) * 6.0;
    let (i, f) = (h6.floor() as i32, h6 - h6.floor());
    let (p, q, t) = (v * (1.0 - s), v * (1.0 - s * f), v * (1.0 - s * (1.0 - f)));
    let (r, g, b) = match i % 6 {
        0 => (v, t, p),
        1 => (q, v, p),
        2 => (p, v, t),
        3 => (p, q, v),
        4 => (t, p, v),
        _ => (v, p, q),
    };
    [r, g, b].map(|c| (c * 255.0).round().clamp(0.0, 255.0) as u8)
}

/// sRGB bytes to standard HSV (hue 0..1).
pub fn rgb_to_hsv(c: [u8; 3]) -> (f32, f32, f32) {
    let [r, g, b] = c.map(|x| x as f32 / 255.0);
    let (mx, mn) = (r.max(g).max(b), r.min(g).min(b));
    let d = mx - mn;
    let h = if d == 0.0 {
        0.0
    } else if mx == r {
        ((g - b) / d).rem_euclid(6.0) / 6.0
    } else if mx == g {
        ((b - r) / d + 2.0) / 6.0
    } else {
        ((r - g) / d + 4.0) / 6.0
    };
    (h, if mx == 0.0 { 0.0 } else { d / mx }, mx)
}

/// More saturated and brighter, hue unchanged. `boost` 0..=100.
pub fn boost_color(rgb: [u8; 3], boost: u8) -> [u8; 3] {
    let k = boost.min(100) as f32 / 100.0;
    let (h, s, v) = rgb_to_hsv(rgb);
    hsv_to_rgb(h, (s * (1.0 + k)).min(1.0), v.powf(1.0 / (1.0 + k / 2.0)))
}

/// Level 0..=1 for one tick of audio. `reference` is the running maximum kept between ticks.
pub fn audio_level(x: f32, reference: &mut f32, auto_sens: bool, sensitivity: u8, intensity: u8, dt: f32) -> f32 {
    let level = if auto_sens {
        *reference = (*reference * 0.5f32.powf(dt / 8.0)).max(x).max(0.02);
        x / *reference
    } else {
        x / (sensitivity.max(1) as f32 / 100.0)
    };
    (level * (0.5 + intensity.min(100) as f32 / 100.0 * 1.5)).min(1.0)
}

/// Per-zone smoothing / envelope state. Pure: no clock, no I/O.
#[derive(Default)]
pub struct ZoneState {
    cur: Option<[f32; 3]>,
    env: f32,
    reference: f32,
}

impl ZoneState {
    pub fn step(&mut self, l: &Lighting, screen: Option<[u8; 3]>, audio: Option<(f32, f32)>, dt: f32) -> [u8; 3] {
        let k = l.brightness.clamp(1, 100) as f32 / 100.0;
        let rgb = match l.effect {
            Effect::Screen => {
                if let Some(s) = screen {
                    let target = boost_color(s, l.host.boost).map(|c| c as f32);
                    let tau = l.host.smoothing.min(100) as f32 / 100.0 * 0.6;
                    let alpha = if tau == 0.0 { 1.0 } else { 1.0 - (-dt / tau).exp() };
                    self.cur = Some(match self.cur {
                        None => target,
                        Some(c) => [0, 1, 2].map(|i| c[i] + alpha * (target[i] - c[i])),
                    });
                }
                self.cur.unwrap_or([0.0; 3])
            }
            Effect::Audio => {
                let x = audio.map(|(peak, bass)| if l.host.bass_only { bass } else { peak }).unwrap_or(0.0);
                let level = audio_level(x, &mut self.reference, l.host.auto_sens, l.host.sensitivity, l.host.intensity, dt);
                self.env = level.max(self.env * (-dt / 0.25).exp());
                [0, 1, 2].map(|i| l.host.color2[i] as f32 + (l.color[i] as f32 - l.host.color2[i] as f32) * self.env)
            }
            _ => [0.0; 3],
        };
        rgb.map(|c| (c * k).round().clamp(0.0, 255.0) as u8)
    }
}

/// Peak and bass (two cascaded one-pole low-passes at 150 Hz) of a mono stream.
pub struct Bands {
    coef: f32,
    lp1: f32,
    lp2: f32,
    peak: f32,
    bass: f32,
}

impl Bands {
    pub fn new(sample_rate: u32) -> Self {
        let coef = 1.0 - (-2.0 * std::f32::consts::PI * 150.0 / sample_rate.max(1) as f32).exp();
        Self { coef, lp1: 0.0, lp2: 0.0, peak: 0.0, bass: 0.0 }
    }

    pub fn feed(&mut self, mono: f32) {
        self.peak = self.peak.max(mono.abs());
        self.lp1 += self.coef * (mono - self.lp1);
        self.lp2 += self.coef * (self.lp1 - self.lp2);
        self.bass = self.bass.max(self.lp2.abs());
    }

    /// (peak, bass) since the last call, each 0..=1. Resets the maxima, keeps the filter state.
    pub fn take(&mut self) -> (f32, f32) {
        let out = (self.peak.min(1.0), self.bass.min(1.0));
        self.peak = 0.0;
        self.bass = 0.0;
        out
    }
}

// ---------------------------------------------------------------- mailbox

/// Colours the fx thread wants on the mouse. The IO thread takes them while it holds host control.
#[derive(Clone, Default)]
pub struct Mailbox(Arc<Mutex<[Option<[u8; 3]>; 2]>>);

impl Mailbox {
    pub fn take(&self) -> [Option<[u8; 3]>; 2] {
        std::mem::take(&mut *self.0.lock().unwrap_or_else(|e| e.into_inner()))
    }

    fn put(&self, zone: usize, rgb: [u8; 3]) {
        self.0.lock().unwrap_or_else(|e| e.into_inner())[zone] = Some(rgb);
    }

    fn clear(&self, zone: usize) {
        self.0.lock().unwrap_or_else(|e| e.into_inner())[zone] = None;
    }
}

// ---------------------------------------------------------------- sources

pub trait Source {
    /// Average colour of that part of the primary screen, averaged in linear light. None if the capture failed.
    fn screen(&mut self, area: ScreenArea) -> Option<[u8; 3]>;
    /// (peak, bass) of what the PC played since the last call, each 0..=1. None if there is no audio source.
    fn audio(&mut self) -> Option<(f32, f32)>;
    /// Called once when the thread stops needing audio or the whole source.
    fn release(&mut self) {}
    /// What the source is (for the probe).
    fn describe(&self) -> String {
        String::new()
    }
    /// Notes about opened / closed streams since the last call (for the probe).
    fn events(&mut self) -> Vec<String> {
        Vec::new()
    }
}

// ---------------------------------------------------------------- thread

#[derive(Clone, Copy)]
pub struct Timing {
    pub tick: Duration,
    pub capture: Duration,
    pub keepalive: Duration,
}

impl Default for Timing {
    fn default() -> Self {
        Self { tick: Duration::from_millis(50), capture: Duration::from_millis(100), keepalive: Duration::from_secs(2) }
    }
}

const PREVIEW_LIFE: Duration = Duration::from_millis(500);

#[derive(Default)]
struct Shared {
    device: [Option<Lighting>; 2],
    preview: [Option<(Lighting, Instant)>; 2],
    preview_out: [Option<[u8; 3]>; 2],
}

struct Inner {
    shared: Mutex<Shared>,
    wake: Condvar,
    mailbox: Mailbox,
}

#[derive(Clone)]
pub struct Handle(Arc<Inner>);

impl Handle {
    /// `make_source` runs on the fx thread (COM / GDI objects stay on that thread).
    pub fn spawn(mailbox: Mailbox, make_source: impl FnOnce() -> Box<dyn Source> + Send + 'static) -> Self {
        Self::spawn_with(mailbox, make_source, Timing::default())
    }

    pub fn spawn_with(mailbox: Mailbox, make_source: impl FnOnce() -> Box<dyn Source> + Send + 'static, timing: Timing) -> Self {
        let inner = Arc::new(Inner { shared: Mutex::new(Shared::default()), wake: Condvar::new(), mailbox });
        let t = inner.clone();
        std::thread::Builder::new().name("fx".into()).spawn(move || run(t, make_source(), timing)).ok();
        Handle(inner)
    }

    /// What the mouse should show (None = zone not host-driven). Synchronous: when it returns, nothing for a
    /// removed zone is in the mailbox and nothing more will be put there. An unchanged zone keeps its state.
    pub fn set_device(&self, zones: [Option<Lighting>; 2]) {
        let mut g = self.0.shared.lock().unwrap_or_else(|e| e.into_inner());
        for (z, l) in zones.iter().enumerate() {
            if l.is_none() {
                self.0.mailbox.clear(z);
            }
        }
        if g.device != zones {
            g.device = zones;
            self.0.wake.notify_all();
        }
    }

    /// Keep a live preview for this zone; it is forgotten 500 ms after its last call. The same `Lighting`
    /// again does not reset the zone's state.
    pub fn preview_zone(&self, zone: usize, l: Lighting) {
        let mut g = self.0.shared.lock().unwrap_or_else(|e| e.into_inner());
        let fresh = g.preview[zone].is_none();
        g.preview[zone] = Some((l, Instant::now()));
        if fresh {
            self.0.wake.notify_all();
        }
    }

    pub fn preview(&self, zone: usize) -> Option<[u8; 3]> {
        self.0.shared.lock().unwrap_or_else(|e| e.into_inner()).preview_out[zone]
    }
}

fn differs(a: [u8; 3], b: [u8; 3]) -> bool {
    (0..3).any(|i| (a[i] as i32 - b[i] as i32).abs() >= 2)
}

fn run(inner: Arc<Inner>, mut src: Box<dyn Source>, timing: Timing) {
    let mut dev_cfg: [Option<Lighting>; 2] = [None; 2];
    let mut dev_state = [ZoneState::default(), ZoneState::default()];
    let mut dev_sent: [Option<([u8; 3], Instant)>; 2] = [None, None];
    let mut pv_cfg: [Option<Lighting>; 2] = [None; 2];
    let mut pv_state = [ZoneState::default(), ZoneState::default()];
    let mut cache: HashMap<ScreenArea, Option<[u8; 3]>> = HashMap::new();
    let mut last_capture: Option<Instant> = None;
    let mut had_any = false;
    let mut had_audio = false;
    let mut last = Instant::now();
    loop {
        let (device, preview) = {
            let mut g = inner.shared.lock().unwrap_or_else(|e| e.into_inner());
            loop {
                let now = Instant::now();
                for z in 0..2 {
                    if g.preview[z].is_some_and(|(_, t)| now.duration_since(t) > PREVIEW_LIFE) {
                        g.preview[z] = None;
                        g.preview_out[z] = None;
                    }
                }
                if g.device.iter().any(Option::is_some) || g.preview.iter().any(Option::is_some) {
                    break;
                }
                if had_any {
                    src.release();
                    had_any = false;
                    had_audio = false;
                    cache.clear();
                    last_capture = None;
                }
                g = inner.wake.wait(g).unwrap_or_else(|e| e.into_inner());
                last = Instant::now();
            }
            (g.device, g.preview.map(|p| p.map(|(l, _)| l)))
        };
        let now = Instant::now();
        let dt = now.duration_since(last).as_secs_f32().min(0.5);
        last = now;

        for z in 0..2 {
            if device[z] != dev_cfg[z] {
                dev_cfg[z] = device[z];
                dev_state[z] = ZoneState::default();
                dev_sent[z] = None;
            }
            if preview[z] != pv_cfg[z] {
                pv_cfg[z] = preview[z];
                pv_state[z] = ZoneState::default();
            }
        }
        let live = || device.iter().chain(preview.iter()).flatten();
        let need_audio = live().any(|l| l.effect == Effect::Audio);
        if had_any && had_audio && !need_audio {
            src.release();
        }
        had_any = true;
        had_audio = need_audio;

        if last_capture.is_none_or(|t| now.duration_since(t) >= timing.capture) {
            cache.clear();
            last_capture = Some(now);
        }
        let mut screen = |area: ScreenArea, src: &mut Box<dyn Source>| *cache.entry(area).or_insert_with(|| src.screen(area));
        let audio = if need_audio { src.audio() } else { None };

        let mut dev_out: [Option<[u8; 3]>; 2] = [None; 2];
        let mut pv_out: [Option<[u8; 3]>; 2] = [None; 2];
        for z in 0..2 {
            if let Some(l) = &device[z] {
                let s = if l.effect == Effect::Screen { screen(l.host.area, &mut src) } else { None };
                dev_out[z] = Some(dev_state[z].step(l, s, audio, dt));
            }
            if let Some(l) = &preview[z] {
                let s = if l.effect == Effect::Screen { screen(l.host.area, &mut src) } else { None };
                pv_out[z] = Some(pv_state[z].step(l, s, audio, dt));
            }
        }

        {
            let mut g = inner.shared.lock().unwrap_or_else(|e| e.into_inner());
            for z in 0..2 {
                if g.preview[z].is_some() && preview[z].is_some() {
                    g.preview_out[z] = pv_out[z];
                }
                if g.device[z] != dev_cfg[z] {
                    continue;
                }
                if let Some(c) = dev_out[z] {
                    let send = match dev_sent[z] {
                        None => true,
                        Some((prev, at)) => differs(prev, c) || now.duration_since(at) >= timing.keepalive,
                    };
                    if send {
                        inner.mailbox.put(z, c);
                        dev_sent[z] = Some((c, now));
                    }
                }
            }
        }
        std::thread::sleep(timing.tick);
    }
}

// ---------------------------------------------------------------- Windows source

#[cfg(windows)]
mod win_src;

#[cfg(windows)]
pub fn win_source() -> Box<dyn Source> {
    Box::new(win_src::WinSource::new())
}

/// Exact (1:1) linear-light average of an area of the primary screen, for checking the sampler.
#[cfg(windows)]
pub fn exact_screen(area: ScreenArea) -> Option<[u8; 3]> {
    win_src::exact_screen(area)
}

/// The primary screen scaled to fit `max_w` x `max_h` (aspect kept), as RGBA bytes — the
/// sample editor's background. Read-only.
#[cfg(windows)]
pub fn screen_picture(max_w: u32, max_h: u32) -> Option<(Vec<u8>, u32, u32)> {
    win_src::screen_picture(max_w, max_h)
}

#[cfg(not(windows))]
pub fn win_source() -> Box<dyn Source> {
    struct None_;
    impl Source for None_ {
        fn screen(&mut self, _: ScreenArea) -> Option<[u8; 3]> {
            None
        }
        fn audio(&mut self) -> Option<(f32, f32)> {
            None
        }
    }
    Box::new(None_)
}

#[cfg(test)]
mod tests;
