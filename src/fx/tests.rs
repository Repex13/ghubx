use super::*;
use crate::profile::HostFx;
use std::sync::atomic::{AtomicUsize, Ordering};

fn sine_bands(freq: f32) -> (f32, f32) {
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
    let (peak, bass) = sine_bands(60.0);
    assert!((bass - 0.862).abs() < 0.02 && (peak - 1.0).abs() < 0.01, "{peak} {bass}");
    assert!((sine_bands(150.0).1 - 0.50).abs() < 0.02);
    assert!(sine_bands(4000.0).1 < 0.01);
    let mut b = Bands::new(48000);
    for _ in 0..1000 {
        b.feed(0.0);
    }
    assert_eq!(b.take(), (0.0, 0.0));
    let mut b = Bands::new(48000);
    for i in 0..4800 {
        b.feed((2.0 * std::f32::consts::PI * 60.0 * i as f32 / 48000.0).sin());
    }
    assert!(b.take().0 > 0.5);
    assert_eq!(b.take(), (0.0, 0.0));
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
        assert_eq!(z.step(&l, Some(c), None, dt), c);
    }
    let l = screen_light(HostFx { smoothing: 100, ..Default::default() });
    let mut z = ZoneState::default();
    assert_eq!(z.step(&l, Some([0, 0, 0]), None, dt), [0, 0, 0]);
    let mut reds = Vec::new();
    for _ in 0..12 {
        reds.push(z.step(&l, Some([255, 0, 0]), None, dt)[0]);
    }
    assert!((reds[0] as i32 - 20).abs() <= 1 && (reds[1] as i32 - 39).abs() <= 1 && (reds[11] as i32 - 161).abs() <= 1, "{reds:?}");
    let prev = z.step(&l, Some([255, 0, 0]), None, dt);
    assert_eq!(z.step(&l, None, None, dt), prev);
    let half = Lighting { brightness: 50, ..screen_light(HostFx { smoothing: 0, ..Default::default() }) };
    let mut z = ZoneState::default();
    assert!(close(z.step(&half, Some([200, 100, 50]), None, dt), [100, 50, 25], 1));
}

#[test]
fn audio_zone_envelope() {
    let dt = 0.05;
    let l = audio_light(HostFx::default());
    let mut z = ZoneState::default();
    assert_eq!(z.step(&l, None, Some((0.0, 0.0)), dt), [16, 16, 48]);
    assert_eq!(z.step(&l, None, Some((1.0, 0.0)), dt), [0, 184, 252]);
    assert!(close(z.step(&l, None, Some((0.0, 0.0)), dt), [3, 154, 215], 1));
    let mut z = ZoneState::default();
    assert_eq!(z.step(&l, None, None, dt), [16, 16, 48]);
    let b = audio_light(HostFx { bass_only: true, ..Default::default() });
    let mut z = ZoneState::default();
    assert_eq!(z.step(&b, None, Some((1.0, 0.0)), dt), [16, 16, 48]);
    assert_eq!(z.step(&b, None, Some((0.0, 1.0)), dt), [0, 184, 252]);
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
}

impl Source for Fake {
    fn screen(&mut self, _: ScreenArea) -> Option<[u8; 3]> {
        self.screens.fetch_add(1, Ordering::SeqCst);
        Some([200, 10, 10])
    }

    fn audio(&mut self) -> Option<(f32, f32)> {
        Some((0.0, 0.0))
    }

    fn release(&mut self) {
        self.releases.fetch_add(1, Ordering::SeqCst);
    }
}

fn rig() -> (Handle, Mailbox, Arc<AtomicUsize>, Arc<AtomicUsize>) {
    let mb = Mailbox::default();
    let (screens, releases) = (Arc::new(AtomicUsize::new(0)), Arc::new(AtomicUsize::new(0)));
    let src = Fake { screens: screens.clone(), releases: releases.clone() };
    let timing = Timing { tick: Duration::from_millis(10), capture: Duration::from_millis(10), keepalive: Duration::from_millis(200) };
    (Handle::spawn_with(mb.clone(), move || Box::new(src), timing), mb, screens, releases)
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
    let (h, mb, _, _) = rig();
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
    let (h, mb, _, releases) = rig();
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
    let (h, mb, _, _) = rig();
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
    let (h, _mb, screens, _) = rig();
    h.set_device([Some(plain_screen()), Some(plain_screen())]);
    std::thread::sleep(Duration::from_millis(60));
    h.set_device([None, None]);
    std::thread::sleep(Duration::from_millis(60));
    let n = screens.load(Ordering::SeqCst);
    assert!(n >= 1);
    let ticks_upper_bound = 60 / 10 + 3;
    assert!(n <= ticks_upper_bound, "{n} captures for two zones on one area");
}
