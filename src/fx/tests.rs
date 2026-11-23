use super::*;
use crate::profile::{AudioBand, HostFx};
use std::sync::atomic::{AtomicUsize, Ordering};

fn sine_bands(freq: f32) -> (f32, f32, f32) {
    let mut b = Bands::new(48000);
    let sample = |i: usize| (2.0 * std::f32::consts::PI * freq * i as f32 / 48000.0).sin();
    for i in 0..24000 {
        b.feed(sample(i));
    }
    b.take();
    for i in 24000..36000 {
        b.feed(sample(i));
    }
    b.take()
}

#[test]
fn bands_follow_the_frequency() {
    let (peak, bass, _) = sine_bands(60.0);
    assert!((bass - 0.862).abs() < 0.02 && (peak - 1.0).abs() < 0.01, "{peak} {bass}");
    assert!((sine_bands(150.0).1 - 0.50).abs() < 0.02);
    assert!(sine_bands(4000.0).1 < 0.01);
    let mut b = Bands::new(48000);
    for _ in 0..1000 {
        b.feed(0.0);
    }
    assert_eq!(b.take(), (0.0, 0.0, 0.0));
    let mut b = Bands::new(48000);
    for i in 0..4800 {
        b.feed((2.0 * std::f32::consts::PI * 60.0 * i as f32 / 48000.0).sin());
    }
    assert!(b.take().0 > 0.5);
    assert_eq!(b.take(), (0.0, 0.0, 0.0));
}

#[test]
fn bands_split_bass_mids_and_all() {
    // a 60 Hz sine is nearly all bass; 1 kHz is nearly all mids; 8 kHz passes neither band
    let (_, bass60, mids60) = sine_bands(60.0);
    assert!(bass60 > 0.8 && mids60 < 0.1, "60 Hz: bass {bass60} mids {mids60}");
    let (peak1k, bass1k, mids1k) = sine_bands(1000.0);
    assert!(mids1k > 0.6 && bass1k < 0.05, "1 kHz: peak {peak1k} bass {bass1k} mids {mids1k}");
    let (_, bass8k, mids8k) = sine_bands(8000.0);
    assert!(bass8k < 0.01 && mids8k < 0.25, "8 kHz: bass {bass8k} mids {mids8k}");
}

fn close(a: [u8; 3], b: [u8; 3], tol: i32) -> bool {
    (0..3).all(|i| (a[i] as i32 - b[i] as i32).abs() <= tol)
}

fn screen_light(host: HostFx) -> Lighting {
    Lighting { effect: Effect::Screen, brightness: 100, host: HostFx { boost: 0, ..host }, ..Default::default() }
}

fn audio_light(host: HostFx) -> Lighting {
    Lighting { effect: Effect::Audio, color: [0, 184, 252], brightness: 100, host: HostFx { color2: [16, 16, 48], intensity: 50, auto_sens: false, sensitivity: 100, ..host }, ..Default::default() }
}

#[test]
fn screen_area_rects() {
    assert_eq!(ScreenArea::Full.rect(), (0.0, 0.0, 1.0, 1.0));
    let t = 1.0 / 3.0;
    assert_eq!(ScreenArea::TopLeft.rect(), (0.0, 0.0, t, t));
    assert_eq!(ScreenArea::Center.rect(), (t, t, 2.0 * t, 2.0 * t));
    let (x0, y0, x1, y1) = ScreenArea::BottomRight.rect();
    assert!((x0 - 2.0 * t).abs() < 1e-6 && (y0 - 2.0 * t).abs() < 1e-6 && x1 == 1.0 && y1 == 1.0);
    let set: std::collections::HashSet<_> = ScreenArea::GRID.iter().collect();
    assert_eq!(set.len(), 9);
    assert!(!set.contains(&ScreenArea::Full));
}

#[test]
fn boost_color_values() {
    for (inp, boost, want) in [
        ([64, 64, 64], 0, [64, 64, 64]),
        ([64, 64, 64], 100, [101, 101, 101]),
        ([128, 100, 100], 100, [161, 91, 91]),
        ([128, 100, 100], 50, [147, 99, 99]),
        ([10, 40, 200], 50, [0, 33, 210]),
        ([30, 60, 90], 50, [0, 55, 111]),
        ([255, 255, 255], 100, [255, 255, 255]),
        ([0, 0, 0], 100, [0, 0, 0]),
    ] {
        let got = boost_color(inp, boost);
        assert!(close(got, want, 1), "{inp:?} boost {boost}: {got:?} vs {want:?}");
    }
}

#[test]
fn screen_zone_smoothing() {
    let dt = 0.05;
    let mut z = ZoneState::default();
    let l = screen_light(HostFx { smoothing: 0, ..Default::default() });
    for c in [[10, 200, 30], [0, 0, 0], [255, 128, 7]] {
        assert_eq!(z.step(&l, Some(c), None, false, dt), c);
    }
    let l = screen_light(HostFx { smoothing: 100, ..Default::default() });
    let mut z = ZoneState::default();
    assert_eq!(z.step(&l, Some([0, 0, 0]), None, false, dt), [0, 0, 0]);
    let mut reds = Vec::new();
    for _ in 0..12 {
        reds.push(z.step(&l, Some([255, 0, 0]), None, false, dt)[0]);
    }
    assert!((reds[0] as i32 - 20).abs() <= 1 && (reds[1] as i32 - 39).abs() <= 1 && (reds[11] as i32 - 161).abs() <= 1, "{reds:?}");
    let prev = z.step(&l, Some([255, 0, 0]), None, false, dt);
    assert_eq!(z.step(&l, None, None, false, dt), prev);
    let half = Lighting { brightness: 50, ..screen_light(HostFx { smoothing: 0, ..Default::default() }) };
    let mut z = ZoneState::default();
    assert!(close(z.step(&half, Some([200, 100, 50]), None, false, dt), [100, 50, 25], 1));
}

#[test]
fn audio_zone_envelope() {
    let dt = 0.05;
    let l = audio_light(HostFx::default());
    let mut z = ZoneState::default();
    assert_eq!(z.step(&l, None, Some((0.0, 0.0, 0.0)), false, dt), [16, 16, 48]);
    assert_eq!(z.step(&l, None, Some((1.0, 0.0, 0.0)), false, dt), [0, 184, 252]);
    assert!(close(z.step(&l, None, Some((0.0, 0.0, 0.0)), false, dt), [3, 154, 215], 1));
    let mut z = ZoneState::default();
    assert_eq!(z.step(&l, None, None, false, dt), [16, 16, 48]);
    let b = audio_light(HostFx { band: AudioBand::Bass, ..Default::default() });
    let mut z = ZoneState::default();
    assert_eq!(z.step(&b, None, Some((1.0, 0.0, 0.0)), false, dt), [16, 16, 48]);
    assert_eq!(z.step(&b, None, Some((0.0, 1.0, 0.0)), false, dt), [0, 184, 252]);
    // MIDS follows only the third component
    let m = audio_light(HostFx { band: AudioBand::Mids, ..Default::default() });
    let mut z = ZoneState::default();
    assert_eq!(z.step(&m, None, Some((1.0, 1.0, 0.0)), false, dt), [16, 16, 48]);
    assert_eq!(z.step(&m, None, Some((0.0, 0.0, 1.0)), false, dt), [0, 184, 252]);
}

#[test]
fn reactive_zone_holds_then_fades() {
    let dt = 0.05;
    let l = Lighting { effect: Effect::Reactive, color: [200, 100, 50], brightness: 100, host: HostFx { hold_ms: 200, ..Default::default() }, ..Default::default() };
    let mut z = ZoneState::default();
    assert_eq!(z.step(&l, None, None, false, dt), [0, 0, 0]);
    // a held button fades the colour in fast
    let c1 = z.step(&l, None, None, true, dt);
    let c2 = z.step(&l, None, None, true, dt);
    let c3 = z.step(&l, None, None, true, dt);
    assert!(c1[0] > 100 && c2[0] > c1[0] && c3[0] > c2[0], "{c1:?} {c2:?} {c3:?}");
    for _ in 0..20 {
        z.step(&l, None, None, true, dt);
    }
    assert_eq!(z.step(&l, None, None, true, dt), [200, 100, 50]);
    // release: hold_ms keeps it lit, then it fades out
    assert!(z.step(&l, None, None, false, dt)[0] > 195);
    assert!(z.step(&l, None, None, false, dt)[0] > 195);
    let mut last = 255i32;
    let mut went_out = false;
    for _ in 0..40 {
        let c = z.step(&l, None, None, false, dt)[0] as i32;
        assert!(c <= last, "fade must be monotonic: {last} -> {c}");
        last = c;
        if c == 0 {
            went_out = true;
            break;
        }
    }
    assert!(went_out, "reactive zone never faded out");
    // brightness scales the colour
    let half = Lighting { brightness: 50, ..l };
    let mut z = ZoneState::default();
    for _ in 0..20 {
        z.step(&half, None, None, true, dt);
    }
    assert!(close(z.step(&half, None, None, true, dt), [100, 50, 25], 1));
}

#[test]
fn monitor_rect_maps_area_fractions() {
    // primary at origin
    assert_eq!(area_rect_on((0, 0, 1920, 1080), ScreenArea::Full), (0, 0, 1920, 1080));
    assert_eq!(area_rect_on((0, 0, 1920, 1080), ScreenArea::Center), (640, 360, 640, 360));
    // a second monitor to the right of a 1920-wide primary
    assert_eq!(area_rect_on((1920, 0, 2560, 1440), ScreenArea::Center), (1920 + 853, 480, 853, 480));
    assert_eq!(area_rect_on((1920, 0, 2560, 1440), ScreenArea::TopLeft), (1920, 0, 853, 480));
    // a monitor LEFT of the primary has negative virtual coordinates
    assert_eq!(area_rect_on((-1280, 0, 1280, 1024), ScreenArea::BottomRight), (-1280 + 853, 682, 426, 341));
    // own rectangle (thousandths of the monitor)
    assert_eq!(area_rect_on((1920, 0, 2560, 1440), ScreenArea::Rect([500, 0, 1000, 500])), (1920 + 1280, 0, 1280, 720));
    // degenerate picks still produce at least 1 px
    let (_, _, cw, ch) = area_rect_on((0, 0, 1, 1), ScreenArea::Center);
    assert!(cw >= 1 && ch >= 1);
    // out-of-range picks and an empty list fall back to the primary rect
    let mons = [(0, 0, 1920, 1080), (1920, 0, 2560, 1440)];
    assert_eq!(monitor_rect(&mons, 0, (0, 0, 1, 1)), (0, 0, 1920, 1080));
    assert_eq!(monitor_rect(&mons, 1, (0, 0, 1, 1)), (1920, 0, 2560, 1440));
    assert_eq!(monitor_rect(&mons, 9, (0, 0, 1, 1)), (0, 0, 1920, 1080));
    assert_eq!(monitor_rect(&[], 3, (0, 0, 1, 1)), (0, 0, 1, 1));
}

#[test]
fn audio_level_rules() {
    let mut r = 0.0;
    assert!((audio_level(0.1, &mut r, true, 50, 50, 0.05) - 1.0).abs() < 0.01);
    assert!((audio_level(0.05, &mut r, true, 50, 50, 0.05) - 0.628).abs() < 0.01);
    let mut r = 0.0;
    assert!((audio_level(0.25, &mut r, false, 50, 0, 0.05) - 0.25).abs() < 0.01);
    assert!(audio_level(0.25, &mut r, false, 0, 0, 0.05).is_finite());
}

struct Fake {
    screens: Arc<AtomicUsize>,
    releases: Arc<AtomicUsize>,
    areas: Arc<Mutex<Vec<ScreenArea>>>,
}

impl Source for Fake {
    fn screen(&mut self, area: ScreenArea, _monitor: usize) -> Option<[u8; 3]> {
        self.screens.fetch_add(1, Ordering::SeqCst);
        self.areas.lock().unwrap().push(area);
        Some([200, 10, 10])
    }

    fn audio(&mut self) -> Option<(f32, f32, f32)> {
        Some((0.0, 0.0, 0.0))
    }

    fn release(&mut self) {
        self.releases.fetch_add(1, Ordering::SeqCst);
    }
}

#[allow(clippy::type_complexity)]
fn rig() -> (Handle, Mailbox, Arc<AtomicUsize>, Arc<AtomicUsize>, Arc<Mutex<Vec<ScreenArea>>>) {
    let mb = Mailbox::default();
    let (screens, releases, areas) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)), Arc::new(Mutex::new(Vec::new())));
    let src = Fake { screens: screens.clone(), releases: releases.clone(), areas: areas.clone() };
    let timing = Timing { tick: Duration::from_millis(10), capture: Duration::from_millis(10), keepalive: Duration::from_millis(200) };
    (Handle::spawn_with(mb.clone(), move || Box::new(src), timing), mb, screens, releases, areas)
}

fn wait_for<T>(timeout_ms: u64, mut f: impl FnMut() -> Option<T>) -> Option<T> {
    let end = Instant::now() + Duration::from_millis(timeout_ms);
    while Instant::now() < end {
        if let Some(v) = f() {
            return Some(v);
        }
        std::thread::sleep(Duration::from_millis(5));
    }
    None
}

fn plain_screen() -> Lighting {
    screen_light(HostFx { smoothing: 0, ..Default::default() })
}

#[test]
fn handle_delivers_and_keeps_alive() {
    let (h, mb, _, _, _) = rig();
    h.set_device([None, Some(plain_screen())]);
    let got = wait_for(500, || {
        let t = mb.take();
        t[1].is_some().then_some(t)
    });
    assert_eq!(got, Some([None, Some([200, 10, 10])]));
    assert_eq!(mb.take(), [None, None]);
    assert!(wait_for(400, || mb.take()[1]).is_some(), "no keep-alive");
}

#[test]
fn removing_a_zone_empties_the_mailbox() {
    let (h, mb, _, releases, _) = rig();
    h.set_device([None, Some(plain_screen())]);
    assert!(wait_for(500, || mb.take()[1]).is_some());
    let before = releases.load(Ordering::SeqCst);
    h.set_device([None, None]);
    assert_eq!(mb.take(), [None, None]);
    std::thread::sleep(Duration::from_millis(150));
    assert_eq!(mb.take(), [None, None]);
    assert_eq!(releases.load(Ordering::SeqCst), before + 1);
}

#[test]
fn previews_are_per_zone_and_expire() {
    let (h, mb, _, _, _) = rig();
    let l = plain_screen();
    let start = Instant::now();
    let mut seen = false;
    while start.elapsed() < Duration::from_millis(400) {
        h.preview_zone(1, l);
        if start.elapsed() > Duration::from_millis(200) {
            seen |= h.preview(1).is_some();
            assert!(!seen || h.preview(1).is_some());
        }
        if start.elapsed() > Duration::from_millis(250) && start.elapsed() < Duration::from_millis(270) {
            h.preview_zone(0, l);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    assert!(seen, "preview(1) never became Some");
    assert!(wait_for(1000, || h.preview(0).is_none().then_some(())).is_some());
    assert!(wait_for(1000, || h.preview(1).is_none().then_some(())).is_some());
    assert_eq!(mb.take(), [None, None]);
}

#[test]
fn one_capture_per_area() {
    let (h, _mb, screens, _, _) = rig();
    h.set_device([Some(plain_screen()), Some(plain_screen())]);
    std::thread::sleep(Duration::from_millis(60));
    h.set_device([None, None]);
    std::thread::sleep(Duration::from_millis(60));
    let n = screens.load(Ordering::SeqCst);
    assert!(n >= 1);
    let ticks_upper_bound = 60 / 10 + 3;
    assert!(n <= ticks_upper_bound, "{n} captures for two zones on one area");
}

#[test]
fn rect_areas_are_captured_and_cached_separately() {
    let (h, _mb, screens, _, areas) = rig();
    let (a, b) = (ScreenArea::Rect([100, 200, 500, 800]), ScreenArea::Rect([50, 60, 300, 400]));
    let zone = |area| screen_light(HostFx { area, smoothing: 0, ..Default::default() });
    // two zones on two different own rectangles: each is asked for, once per capture
    h.set_device([Some(zone(a)), Some(zone(b))]);
    std::thread::sleep(Duration::from_millis(60));
    h.set_device([None, None]);
    std::thread::sleep(Duration::from_millis(60));
    let got = areas.lock().unwrap().clone();
    assert!(got.contains(&a) && got.contains(&b), "{got:?}");
    let n = screens.load(Ordering::SeqCst);
    let ticks = 60 / 10 + 3;
    assert!(n >= 2 && n <= ticks * 2, "{n} captures for two zones on two areas");
    // the same rectangle on both zones: the cache keeps them at one capture per tick
    screens.store(0, Ordering::SeqCst);
    areas.lock().unwrap().clear();
    h.set_device([Some(zone(a)), Some(zone(a))]);
    std::thread::sleep(Duration::from_millis(60));
    h.set_device([None, None]);
    std::thread::sleep(Duration::from_millis(60));
    let n = screens.load(Ordering::SeqCst);
    assert!(n >= 1 && n <= ticks, "{n} captures for two zones on one rectangle");
    assert!(areas.lock().unwrap().iter().all(|s| *s == a));
}
