//! Device probe. No arguments: read-only state dump (`cargo run --bin probe`).
//! Subcommands: `ghub-status`, `ghub-close`, `ghub-start`, `takeover-test` and
//! `release` (both write to the mouse; they refuse to run while G HUB's agent
//! is running, and `takeover-test` also while any `ghubx.exe` is running).
use ghubx::device::{G502, MODE_HOST, MODE_ONBOARD};
use ghubx::hidpp::Result;
use ghubx::osd::{OsdKind, render};
use ghubx::profile::{Effect, Lighting};
use ghubx::ui::palette::palette_for;
use ghubx::win;
use hidapi::HidApi;
use std::time::{Duration, Instant};

const IDENTITY_MAP: [u8; 16] = [1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 0, 0, 0, 0, 0];
const MUTE_SLOT: usize = 8;

fn main() {
    match std::env::args().nth(1).as_deref() {
        None => dump(),
        Some("ghub-status") => print_status(),
        Some("ghub-close") => {
            let n = win::close_ghub();
            println!("closed {n} G HUB process(es)");
            std::thread::sleep(Duration::from_millis(400));
            print_status();
        }
        Some("ghub-start") => {
            win::start_ghub();
            println!("started G HUB");
            std::thread::sleep(Duration::from_millis(1500));
            print_status();
        }
        Some("takeover-test") => std::process::exit(takeover_test()),
        Some("release") => std::process::exit(release()),
        Some("games") => std::process::exit(games()),
        Some("fx-screen") => std::process::exit(fx_screen(std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(3.0))),
        Some("onboard-backup") => std::process::exit(onboard_backup_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("onboard-save") => std::process::exit(onboard_save_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("onboard-restore") => std::process::exit(onboard_restore_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("onboard-activate") => std::process::exit(onboard_activate_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("dpi-led") => std::process::exit(toggle_cmd("dpi-led", &std::env::args().skip(2).collect::<Vec<_>>())),
        Some("startup-effect") => std::process::exit(toggle_cmd("startup-effect", &std::env::args().skip(2).collect::<Vec<_>>())),
        Some("obs-status") => std::process::exit(obs_status_cmd()),
        Some("obs") => std::process::exit(obs_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("fx-screen-check") => std::process::exit(fx_screen_check()),
        Some("fx-sample") => std::process::exit(fx_sample(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("fx-audio") => std::process::exit(fx_audio(std::env::args().nth(2).and_then(|a| a.parse().ok()).unwrap_or(5.0))),
        Some("export-theme") => std::process::exit(export_theme_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("import-theme") => std::process::exit(import_theme_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("game-sens") => {
            for (name, id) in [("Counter-Strike 2", Some("ca572e6a-c76d-4e34-bd64-3665c0d0b0c1")), ("Valorant", Some("48574816-5d5d-4e5b-b616-9c3af2c15bbf")), ("Fortnite", Some("b2fd8db8-b15c-49f0-91a8-7839ee611aaa")), ("Tom Clancy's Rainbow Six Siege", Some("cebe3aba-815a-486d-94ed-31c20a385ddb")), ("Apex Legends", None)] {
                match ghubx::games::game_sensitivity_file(id, name) {
                    Some((g, file)) => println!("{name}: file {} | sensitivity {} | yaw {} | preset {} | {} counts for 180", file.display(), g.sens, g.yaw, ghubx::profile::P180_GAMES[g.preset].0, g.counts),
                    None => println!("{name}: not found"),
                }
            }
        }
        Some("onboard") => std::process::exit(onboard_cmd()),
        Some("save-dialog") => println!("{:?}", win::save_file(0, "Export a game or app", "test.ghubx", "GHUB-X profile", "*.ghubx")),
        Some("ghub-autostart") => println!("startup_approved(LGHUB) = {:?}", win::startup_approved("LGHUB")),
        Some("export-app") => std::process::exit(export_app_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("import-app") => std::process::exit(import_app_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("cache-sync") => {
            let n = ghubx::ui::assets::sync_cache();
            let dir = ghubx::ui::assets::cache_dir();
            println!("saved copy folder: {} ({n} file(s) written)", dir.display());
            for name in ghubx::ui::assets::CACHE_NAMES {
                match std::fs::metadata(dir.join(name)) {
                    Ok(m) => println!("  {name}: {} bytes", m.len()),
                    Err(_) => println!("  {name}: missing"),
                }
            }
        }
        Some("pick-exe") => {
            println!("{:?}", win::pick_exe(0));
        }
        Some("osd-png") => std::process::exit(osd_png(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("osd") => std::process::exit(osd_show(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("listen") => listen(std::env::args().nth(2).and_then(|s| s.parse().ok()).unwrap_or(15)),
        Some("update-check") => std::process::exit(update_check_cmd()),
        Some("update-fetch") => std::process::exit(update_fetch_cmd()),
        Some(other) => {
            eprintln!("unknown subcommand {other:?}; use ghub-status, ghub-close, ghub-start, takeover-test, release or listen");
            std::process::exit(64);
        }
    }
}

/// `probe update-check` — one real GitHub releases/latest call; prints what it
/// found and whether the tag is newer than this build. Read-only: never
/// downloads or applies.
fn update_check_cmd() -> i32 {
    match ghubx::update::latest_release() {
        Ok(Some(r)) => {
            let (a, b, c) = r.version;
            let local = ghubx::update::current_ver();
            println!("tag: {}", r.tag);
            println!("parsed version: {a}.{b}.{c}");
            println!("local version: {}", env!("CARGO_PKG_VERSION"));
            println!("update available: {}", if ghubx::update::ver_gt(r.version, local) { "yes" } else { "no" });
            0
        }
        Ok(None) => {
            println!("latest release: none — the repo has no releases yet (GitHub returned 404)");
            println!("local version: {}", env!("CARGO_PKG_VERSION"));
            println!("update available: no");
            0
        }
        Err(e) => {
            eprintln!("update check failed: {e}");
            1
        }
    }
}

/// `probe update-fetch` — download the latest release's exe into the normal
/// updates dir and verify its SHA-256. The only probe that downloads; it never
/// touches the mouse.
fn update_fetch_cmd() -> i32 {
    match ghubx::update::latest_release() {
        Ok(Some(r)) => match ghubx::update::fetch_release(&r) {
            Ok(path) => {
                let size = std::fs::metadata(&path).map(|m| m.len()).unwrap_or(0);
                println!("tag: {}", r.tag);
                println!("exe: {size} bytes at {}", path.display());
                println!("sha256 ok");
                0
            }
            Err(e) => {
                eprintln!("fetch failed: {e}");
                1
            }
        },
        Ok(None) => {
            println!("no releases yet");
            0
        }
        Err(e) => {
            eprintln!("update check failed: {e}");
            1
        }
    }
}

fn export_app_cmd(args: &[String]) -> i32 {
    let (Some(idx), Some(file)) = (args.first().and_then(|a| a.parse::<usize>().ok()), args.get(1)) else {
        eprintln!("usage: probe export-app <app index> <file>");
        return 64;
    };
    let Some(cfg) = ghubx::profile::Config::load_existing() else {
        eprintln!("no config at {}", ghubx::profile::Config::path().display());
        return 1;
    };
    let Some(app) = cfg.apps.get(idx) else {
        eprintln!("no app {idx} (config has {})", cfg.apps.len());
        return 1;
    };
    let text = ghubx::profile::export_app(app);
    match std::fs::write(file, &text) {
        Ok(()) => {
            println!("exported {:?} ({} profile(s), {} macro(s)) to {file} ({} bytes)", app.name, app.profiles.len(), app.macros.len(), text.len());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn import_app_cmd(args: &[String]) -> i32 {
    let Some(file) = args.first() else {
        eprintln!("usage: probe import-app <file>");
        return 64;
    };
    let app = match std::fs::read_to_string(file).map_err(|e| e.to_string()).and_then(|t| ghubx::profile::import_app(&t)) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("{e}");
            return 1;
        }
    };
    let mut cfg = ghubx::profile::Config::load_existing().unwrap_or_default();
    let (profiles, macros, exe, dir, ghub_id) = (app.profiles.len(), app.macros.len(), app.exe.clone(), app.dir.clone(), app.ghub_id.clone());
    let (idx, linked) = cfg.add_imported(app);
    println!(
        "would add as {:?} (index {idx}): {profiles} profile(s), {macros} macro(s), exe {exe:?}, dir {dir:?}, G HUB id {ghub_id:?}; linked to its game: {linked} (nothing was written)",
        cfg.apps[idx].name
    );
    0
}

/// Read-only dump of the mouse's on-board profile directory and profiles.
fn onboard_cmd() -> i32 {
    use ghubx::profile::{Lighting, OnboardProfile, onboard_summary, parse_directory, sector_crc_ok};
    if win::ghub_running() {
        eprintln!("lghub_agent.exe is running: not reading the mouse's memory while G HUB talks to it.");
        return 2;
    }
    let api = HidApi::new().unwrap();
    let Ok(dev) = G502::connect(&api) else {
        eprintln!("G502 HERO not found");
        return 1;
    };
    let dir = match dev.read_sector(0) {
        Ok(s) => parse_directory(&s),
        Err(e) => {
            eprintln!("directory read failed: {e}");
            return 1;
        }
    };
    println!("directory: {dir:?}");
    let effect = |l: &[u8; 11]| Lighting::from_led(l).map_or_else(|| format!("{l:02X?}"), |l| format!("{:?}", l.effect));
    for (i, (sector, enabled)) in dir.iter().enumerate() {
        match dev.read_sector(*sector) {
            Ok(s) => {
                let p = OnboardProfile::parse(&s);
                println!(
                    "slot {}: sector {sector} enabled {enabled} crc_ok {} | {} | default dpi index {} | zone0 {} | zone1 {}",
                    i + 1,
                    sector_crc_ok(&s),
                    p.as_ref().map_or("unreadable".to_string(), onboard_summary),
                    p.as_ref().map_or("-".to_string(), |p| p.default_dpi.to_string()),
                    p.as_ref().map_or("-".to_string(), |p| effect(&p.leds[0])),
                    p.as_ref().map_or("-".to_string(), |p| effect(&p.leds[1])),
                );
            }
            Err(e) => println!("slot {}: sector {sector} read failed: {e}", i + 1),
        }
    }
    0
}

fn fx_screen(secs: f32) -> i32 {
    use ghubx::profile::ScreenArea;
    use std::time::{Duration, Instant};
    let mut src = ghubx::fx::win_source();
    let (mut total, mut n) = (0.0f64, 0u32);
    let end = Instant::now() + Duration::from_secs_f32(secs);
    let hex = |c: Option<[u8; 3]>| c.map(|c| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])).unwrap_or_else(|| "failed".into());
    // per-monitor sanity: rect in virtual-screen coordinates + one FULL capture each
    let mons = win::monitors();
    for (i, m) in mons.iter().enumerate() {
        let name = if i == 0 { "primary".to_string() } else { format!("monitor {}", i + 1) };
        println!("{name}: {}x{} at ({},{})  full {}", m.2, m.3, m.0, m.1, hex(src.screen(ScreenArea::Full, i)));
    }
    while Instant::now() < end {
        let t = Instant::now();
        let (full, center) = (src.screen(ScreenArea::Full, 0), src.screen(ScreenArea::Center, 0));
        let ms = t.elapsed().as_secs_f64() * 1000.0;
        total += ms / 2.0;
        n += 1;
        println!("capture {ms:.1} ms  full {}  center {}", hex(full), hex(center));
        std::thread::sleep(Duration::from_millis(250));
    }
    let (w, h) = unsafe { (windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CXSCREEN), windows::Win32::UI::WindowsAndMessaging::GetSystemMetrics(windows::Win32::UI::WindowsAndMessaging::SM_CYSCREEN)) };
    println!("avg capture {:.1} ms over {n} samples (screen {w}x{h}, {} monitor(s))", total / n.max(1) as f64, mons.len());
    0
}

/// Exit 2 when something else could own the mouse.
fn refuse_writes() -> bool {
    if win::ghub_running() {
        eprintln!("lghub_agent.exe is running: G HUB owns the mouse. Nothing was written.");
        return true;
    }
    if win::processes().iter().any(|(_, n)| n.eq_ignore_ascii_case("ghubx.exe")) {
        eprintln!("ghubx.exe is running: two controllers would fight over the mouse. Quit it first. Nothing was written.");
        return true;
    }
    false
}

fn open_device() -> Option<G502> {
    let api = HidApi::new().ok()?;
    match G502::connect(&api) {
        Ok(d) => Some(d),
        Err(e) => {
            eprintln!("no G502 HERO: {e}");
            None
        }
    }
}

fn hex_dump(b: &[u8]) -> String {
    b.chunks(16).enumerate().map(|(i, c)| format!("{:02X}: {}", i * 16, c.iter().map(|x| format!("{x:02x}")).collect::<Vec<_>>().join(" "))).collect::<Vec<_>>().join("
")
}

fn onboard_backup_cmd(args: &[String]) -> i32 {
    let Some(dir) = args.first() else {
        eprintln!("usage: probe onboard-backup <dir>");
        return 64;
    };
    let Some(dev) = open_device() else { return 1 };
    match ghubx::onboard_write::backup(&dev, std::path::Path::new(dir), true) {
        Ok((d, _)) => {
            println!("backed up sectors 0..=5 and a manifest to {}", d.display());
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn onboard_save_cmd(args: &[String]) -> i32 {
    use ghubx::profile::{Config, encode_onboard};
    let nums: Vec<usize> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let (dry, activate) = (args.iter().any(|a| a == "--dry-run"), args.iter().any(|a| a == "--activate"));
    let [slot, app, prof] = nums[..] else {
        eprintln!("usage: probe onboard-save <slot 1..5> <app index> <profile index> [--dry-run] [--activate]");
        return 64;
    };
    if !(1..=5).contains(&slot) {
        eprintln!("slot must be 1..=5");
        return 64;
    }
    let cfg = Config::load();
    let Some(a) = cfg.apps.get(app) else {
        eprintln!("no app {app}");
        return 1;
    };
    let Some(p) = a.profiles.get(prof) else {
        eprintln!("no profile {prof} in {}", a.name);
        return 1;
    };
    let name = format!("{}: {}", a.name, p.name);
    let enc = encode_onboard(p, &name, &a.macros);
    println!("slot {slot} <- \"{name}\"
{}", hex_dump(&enc.sector));
    for (s, g, l) in &enc.not_stored {
        if *s == ghubx::profile::NOT_A_SLOT {
            println!("not stored: {l}");
        } else {
            println!("not stored: G{} {} ({l})", s + 1, if *g { "G-Shift" } else { "base" });
        }
    }
    if dry {
        println!("dry run: the device was not opened");
        return 0;
    }
    if refuse_writes() {
        return 2;
    }
    let Some(dev) = open_device() else { return 1 };
    let mut log = |m: String| println!("{m}");
    match ghubx::onboard_write::write_sectors(&dev, &[(slot as u16, enc.sector)], activate.then_some(slot as u8), &mut log) {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn onboard_restore_cmd(args: &[String]) -> i32 {
    let slots: Vec<u16> = match args.first().map(String::as_str) {
        Some("all") => (1..=5).collect(),
        Some(s) => match s.parse::<u16>() {
            Ok(n) if (1..=5).contains(&n) => vec![n],
            _ => {
                eprintln!("usage: probe onboard-restore <slot 1..5 | all>");
                return 64;
            }
        },
        None => {
            eprintln!("usage: probe onboard-restore <slot 1..5 | all>");
            return 64;
        }
    };
    if refuse_writes() {
        return 2;
    }
    let Some(dev) = open_device() else { return 1 };
    let mut sectors = Vec::new();
    for n in slots {
        match ghubx::onboard_write::read_stable(&dev, 0x0100 + n) {
            Ok(rom) => sectors.push((n, ghubx::profile::restore_from_rom(&rom))),
            Err(e) => {
                eprintln!("{e}");
                return 1;
            }
        }
    }
    let mut log = |m: String| println!("{m}");
    match ghubx::onboard_write::write_sectors(&dev, &sectors, None, &mut log) {
        Ok(m) => {
            println!("{m}");
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn onboard_activate_cmd(args: &[String]) -> i32 {
    let Some(slot) = args.first().and_then(|a| a.parse::<u8>().ok()).filter(|n| (1..=5).contains(n)) else {
        eprintln!("usage: probe onboard-activate <slot 1..5>");
        return 64;
    };
    if refuse_writes() {
        return 2;
    }
    let Some(dev) = open_device() else { return 1 };
    let r = dev.set_mode(ghubx::device::MODE_ONBOARD).and_then(|_| dev.set_onboard_profile(slot)).and_then(|_| dev.get_onboard_profile());
    match r {
        Ok(now) => {
            println!("active slot reads {now}");
            i32::from(now != slot)
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn toggle_cmd(which: &str, args: &[String]) -> i32 {
    let on = match args.first().map(String::as_str) {
        Some("on") => true,
        Some("off") => false,
        _ => {
            eprintln!("usage: probe {which} <on|off>");
            return 64;
        }
    };
    if refuse_writes() {
        return 2;
    }
    let Some(dev) = open_device() else { return 1 };
    let r = if which == "dpi-led" {
        dev.set_dpi_led_always_on(on).and_then(|_| dev.get_dpi_led_always_on())
    } else {
        dev.set_startup_effect(on).and_then(|_| dev.get_startup_effect())
    };
    match r {
        Ok(now) => {
            println!("{which} now reads {}", if now { "on" } else { "off" });
            i32::from(now != on)
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn obs_wait(mut done: impl FnMut() -> bool) {
    let end = std::time::Instant::now() + std::time::Duration::from_secs(8);
    while std::time::Instant::now() < end && !done() {
        std::thread::sleep(std::time::Duration::from_millis(50));
    }
}

fn obs_status_cmd() -> i32 {
    use ghubx::obs::ObsStatus;
    let h = ghubx::obs::Handle::spawn();
    h.refresh();
    obs_wait(|| h.status() != ObsStatus::Unknown);
    std::thread::sleep(std::time::Duration::from_millis(300));
    let st = h.status();
    println!("status: {st:?}");
    if let ObsStatus::Connected { version } = &st {
        println!("OBS version: {version}");
        let l = h.lists();
        println!("scenes: {:?}
collections: {:?}
sources: {:?}
mixers: {:?}", l.scenes, l.collections, l.sources, l.mixers);
    }
    i32::from(!matches!(st, ObsStatus::Connected { .. }))
}

fn obs_cmd(args: &[String]) -> i32 {
    use ghubx::profile::ObsAction::*;
    let Some(name) = args.first() else {
        eprintln!("usage: probe obs <action> [target]");
        return 64;
    };
    let target = args.get(1).cloned();
    let need = |f: fn(String) -> ghubx::profile::ObsAction| target.clone().map(f);
    let action = match name.as_str() {
        "start-stream" => Some(StartStream),
        "stop-stream" => Some(StopStream),
        "toggle-stream" => Some(ToggleStream),
        "start-record" => Some(StartRecord),
        "stop-record" => Some(StopRecord),
        "toggle-record" => Some(ToggleRecord),
        "start-buffer" => Some(StartBuffer),
        "stop-buffer" => Some(StopBuffer),
        "toggle-buffer" => Some(ToggleBuffer),
        "save-buffer" => Some(SaveBuffer),
        "mute-desktop" => Some(MuteDesktop),
        "unmute-desktop" => Some(UnmuteDesktop),
        "toggle-desktop" => Some(ToggleDesktopMute),
        "mute-mic" => Some(MuteMic),
        "unmute-mic" => Some(UnmuteMic),
        "toggle-mic" => Some(ToggleMicMute),
        "scene" => need(Scene),
        "collection" => need(Collection),
        "source-on" => need(SourceOn),
        "source-off" => need(SourceOff),
        "source-toggle" => need(SourceToggle),
        "mixer-mute" => need(MixerMute),
        "mixer-unmute" => need(MixerUnmute),
        "mixer-toggle" => need(MixerToggle),
        _ => None,
    };
    let Some(a) = action else {
        eprintln!("unknown action or missing target: {name}");
        return 64;
    };
    let h = ghubx::obs::Handle::spawn();
    h.run(a);
    obs_wait(|| h.last_error().is_some() || matches!(h.status(), ghubx::obs::ObsStatus::Connected { .. }));
    std::thread::sleep(std::time::Duration::from_millis(500));
    match h.last_error() {
        Some(e) => {
            eprintln!("{e}");
            1
        }
        None if matches!(h.status(), ghubx::obs::ObsStatus::Connected { .. }) => {
            println!("done");
            0
        }
        None => {
            eprintln!("OBS is not reachable: {:?}", h.status());
            1
        }
    }
}

fn fx_screen_check() -> i32 {
    use ghubx::profile::ScreenArea;
    let mut src = ghubx::fx::win_source();
    let hex = |c: [u8; 3]| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let diff = |a: [u8; 3], b: [u8; 3]| (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).max().unwrap_or(0);
    let mut best = i32::MAX;
    for round in 1..=3 {
        let (sf, sc) = (src.screen(ScreenArea::Full, 0), src.screen(ScreenArea::Center, 0));
        let (ef, ec) = (ghubx::fx::exact_screen(ScreenArea::Full, 0), ghubx::fx::exact_screen(ScreenArea::Center, 0));
        let (Some(sf), Some(sc), Some(ef), Some(ec)) = (sf, sc, ef, ec) else {
            eprintln!("round {round}: capture failed");
            return 1;
        };
        let (df, dc) = (diff(sf, ef), diff(sc, ec));
        println!("round {round}  full {} vs {} (max diff {df})  center {} vs {} (max diff {dc})", hex(sf), hex(ef), hex(sc), hex(ec));
        best = best.min(df.max(dc));
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    println!("best max diff: {best}");
    i32::from(best > 6)
}

/// Linear-light average of a rectangle of a `screen_picture` (RGBA bytes), same math as `exact_screen`.
fn pic_average(px: &[u8], w: u32, h: u32, area: ghubx::profile::ScreenArea) -> Option<[u8; 3]> {
    let lut: Vec<f32> = (0..=255u16).map(|v| ((v as f32) / 255.0).powf(2.2)).collect();
    let (x0, y0, x1, y1) = area.rect();
    let (sx, sy) = ((x0 * w as f32) as usize, (y0 * h as f32) as usize);
    let (cw, ch) = ((((x1 - x0) * w as f32) as usize).max(1), (((y1 - y0) * h as f32) as usize).max(1));
    let mut sum = [0.0f64; 3];
    let mut n = 0u64;
    for y in sy..(sy + ch).min(h as usize) {
        for x in sx..(sx + cw).min(w as usize) {
            let p = &px[(y * w as usize + x) * 4..][..4];
            sum[0] += lut[p[0] as usize] as f64;
            sum[1] += lut[p[1] as usize] as f64;
            sum[2] += lut[p[2] as usize] as f64;
            n += 1;
        }
    }
    (n > 0).then(|| {
        sum.map(|s| {
            let l = (s / n as f64) as f32;
            let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
            (c * 255.0).round().clamp(0.0, 255.0) as u8
        })
    })
}

fn fx_sample(args: &[String]) -> i32 {
    use ghubx::profile::ScreenArea;
    let nums: Vec<u16> = args.iter().filter_map(|a| a.parse().ok()).collect();
    let [x0, y0, x1, y1] = nums[..] else {
        eprintln!("usage: probe fx-sample <x0> <y0> <x1> <y1> (thousandths of the screen)");
        return 64;
    };
    let area = ScreenArea::Rect([x0, y0, x1, y1]);
    let mut src = ghubx::fx::win_source();
    let hex = |c: [u8; 3]| format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
    let diff = |a: [u8; 3], b: [u8; 3]| (0..3).map(|i| (a[i] as i32 - b[i] as i32).abs()).max().unwrap_or(0);
    let mut best = i32::MAX;
    for round in 1..=3 {
        let (s, e) = (src.screen(area, 0), ghubx::fx::exact_screen(area, 0));
        let pic = ghubx::fx::screen_picture(0, 640, 640).and_then(|(px, w, h)| pic_average(&px, w, h, area));
        let (Some(s), Some(e)) = (s, e) else {
            eprintln!("round {round}: capture failed");
            return 1;
        };
        let d = diff(s, e);
        let p = pic.map_or("no picture".into(), hex);
        println!("round {round}  rect [{x0} {y0} {x1} {y1}]  {} vs exact {} (max diff {d})  picture {p}", hex(s), hex(e));
        best = best.min(d);
        std::thread::sleep(std::time::Duration::from_millis(300));
    }
    println!("best max diff: {best}");
    i32::from(best > 6)
}

fn fx_audio(secs: f32) -> i32 {
    use std::time::{Duration, Instant};
    let mut src = ghubx::fx::win_source();
    println!("source: {}", src.describe());
    let end = Instant::now() + Duration::from_secs_f32(secs);
    while Instant::now() < end {
        let a = src.audio();
        for e in src.events() {
            println!("{e}");
        }
        match a {
            Some((p, b, m)) => println!("peak {p:.3} bass {b:.3} mids {m:.3}"),
            None => println!("no audio device"),
        }
        std::thread::sleep(Duration::from_millis(100));
    }
    src.release();
    for e in src.events() {
        println!("{e}");
    }
    0
}

fn export_theme_cmd(args: &[String]) -> i32 {
    let (Some(idx), Some(file)) = (args.first().and_then(|a| a.parse::<usize>().ok()), args.get(1)) else {
        eprintln!("usage: probe export-theme <custom theme index> <file>");
        return 64;
    };
    let Some(cfg) = ghubx::profile::Config::load_existing() else {
        eprintln!("no config at {}", ghubx::profile::Config::path().display());
        return 1;
    };
    let Some(t) = cfg.appearance.custom.get(idx) else {
        eprintln!("no custom theme {idx} (config has {})", cfg.appearance.custom.len());
        return 1;
    };
    match std::fs::write(file, ghubx::profile::export_theme(t)) {
        Ok(()) => {
            println!("exported theme {:?} to {file}", t.name);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn import_theme_cmd(args: &[String]) -> i32 {
    let Some(file) = args.first() else {
        eprintln!("usage: probe import-theme <file>");
        return 64;
    };
    match std::fs::read_to_string(file).map_err(|e| e.to_string()).and_then(|t| ghubx::profile::import_theme(&t)) {
        Ok(t) => {
            println!("theme {:?}: bg {:?} surface {:?} text {:?} accent {:?} (nothing was written)", t.name, t.bg, t.surface, t.text, t.accent);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

fn games() -> i32 {
    let db = ghubx::games::GameDb::load();
    println!("G HUB game database: {} games", db.as_ref().map_or(0, |d| d.games.len()));
    println!("name | source | folder | G HUB id | commands with a key combo");
    for g in ghubx::games::scan(db.as_ref()) {
        let mapped = g
            .ghub_id
            .as_deref()
            .and_then(|id| db.as_ref()?.by_id(id))
            .map_or(0, |game| game.commands.iter().filter(|c| ghubx::games::key_combo(&c.keys).is_some()).count());
        println!("{} | {} | {} | {} | {}", g.name, g.source, g.dir, g.ghub_id.as_deref().unwrap_or("-"), mapped);
    }
    0
}

fn parse_osd(args: &[String]) -> Option<OsdKind> {
    match args.first()?.as_str() {
        "dpi" => Some(OsdKind::Dpi { value: args.get(1)?.parse().ok()?, levels: args.get(2)?.parse().ok()?, active: args.get(3)?.parse().ok()? }),
        "profile" => Some(OsdKind::Profile { name: args.get(1)?.clone() }),
        _ => None,
    }
}

fn flag_value(args: &[String], name: &str) -> Option<String> {
    args.iter().position(|a| a == name).and_then(|i| args.get(i + 1).cloned())
}

/// `probe osd-png <out.png> dpi <value> <levels> <active>|profile <name> [--theme id] [--scale f]`
fn osd_png(args: &[String]) -> i32 {
    let (Some(out), Some(kind)) = (args.first(), parse_osd(&args[1.min(args.len())..])) else {
        eprintln!("usage: probe osd-png <out.png> dpi <value> <levels> <active> | profile <name> [--theme <id>] [--scale <f>]");
        return 64;
    };
    let theme = flag_value(args, "--theme").unwrap_or_else(|| "ghub-dark".into());
    let scale: f32 = flag_value(args, "--scale").and_then(|s| s.parse().ok()).unwrap_or(1.0);
    let img = render(&kind, &palette_for(&theme), scale);
    let mut rgba = Vec::with_capacity(img.bgra.len());
    for p in img.bgra.as_chunks::<4>().0 {
        let a = p[3] as u32;
        let un = |v: u8| ((v as u32 * 255 + a / 2).checked_div(a).unwrap_or(0)).min(255) as u8;
        rgba.extend_from_slice(&[un(p[2]), un(p[1]), un(p[0]), p[3]]);
    }
    match image::save_buffer(out, &rgba, img.w, img.h, image::ExtendedColorType::Rgba8) {
        Ok(()) => {
            println!("wrote {out} ({}x{})", img.w, img.h);
            0
        }
        Err(e) => {
            eprintln!("{e}");
            1
        }
    }
}

/// `probe osd dpi <value> <levels> <active>|profile <name>`: show on screen for ~2 s.
fn osd_show(args: &[String]) -> i32 {
    let Some(kind) = parse_osd(args) else {
        eprintln!("usage: probe osd dpi <value> <levels> <active> | profile <name>");
        return 64;
    };
    ghubx::osd::show(kind);
    std::thread::sleep(Duration::from_millis(2200));
    0
}

fn print_status() {
    println!("lghub_agent.exe running: {}", win::ghub_running());
}

fn dump() {
    let api = HidApi::new().unwrap();
    let dev = G502::connect(&api).expect("G502 HERO not found");
    let i = &dev.info;
    println!("{} fw {} (bootloader {}), HID++ {}.{}", i.name, i.fw, i.bootloader, i.protocol.0, i.protocol.1);
    println!("features: {:04X?}", i.features.iter().map(|f| f.1).collect::<Vec<_>>());
    println!("mode={:?} (1 onboard, 2 host)  dpi={:?}  rate_ms={:?}", dev.get_mode(), dev.get_dpi(), dev.get_rate_ms());
    for z in 0..2 {
        println!("zone{z} = {:02X?}", dev.get_zone_effect(z));
    }
    println!("0x8110 map = {:02X?}", dev.spy_get_map());
}

struct Snapshot {
    mode: u8,
    dpi: u16,
    default_dpi: u16,
    rate_ms: u8,
    map: [u8; 16],
    zones: [[u8; 11]; 2],
}

fn snapshot(dev: &G502) -> Result<Snapshot> {
    let (dpi, default_dpi) = dev.get_dpi()?;
    Ok(Snapshot {
        mode: dev.get_mode()?,
        dpi,
        default_dpi,
        rate_ms: dev.get_rate_ms()?,
        map: dev.spy_get_map()?,
        zones: [dev.get_zone_effect(0)?, dev.get_zone_effect(1)?],
    })
}

fn print_snapshot(label: &str, s: &Snapshot) {
    println!(
        "{label}: mode={} dpi={} (default {}) rate_ms={} map={:02X?}\n    zone0={:02X?}\n    zone1={:02X?}",
        s.mode, s.dpi, s.default_dpi, s.rate_ms, s.map, s.zones[0], s.zones[1]
    );
}

#[derive(Default)]
struct Report {
    failed: bool,
}

impl Report {
    fn step(&mut self, label: &str, r: Result<(bool, String)>) {
        match r {
            Ok((true, v)) => println!("PASS {label}: {v}"),
            Ok((false, v)) => {
                self.failed = true;
                println!("FAIL {label}: {v}");
            }
            Err(e) => {
                self.failed = true;
                println!("FAIL {label}: error {e}");
            }
        }
    }
}

fn set_map_checked(dev: &G502, map: &[u8; 16]) -> Result<(bool, String)> {
    dev.spy_set_map(map)?;
    let got = dev.spy_get_map()?;
    Ok((&got == map, format!("wrote {map:02X?} read {got:02X?}")))
}

/// Print every 0x8110 button bitmap seen for `dur`; returns how many arrived.
fn print_spy_events(dev: &mut G502, dur: Duration) -> Result<usize> {
    let start = Instant::now();
    let mut count = 0;
    let mut frames = Vec::new();
    while start.elapsed() < dur {
        frames.clear();
        dev.poll_events(&mut frames)?;
        for f in &frames {
            if let Some(mask) = dev.parse_spy(f) {
                count += 1;
                println!("  +{:>5} ms  buttons={mask:011b} (0x{mask:03X})", start.elapsed().as_millis());
            }
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    Ok(count)
}

/// Read-only: print button events for `secs` seconds. Works next to a running
/// GHUB-X or G HUB (it never starts or stops the spy itself).
fn listen(secs: u64) {
    let api = HidApi::new().unwrap();
    let mut dev = G502::connect(&api).expect("G502 HERO not found");
    println!("listening {secs} s (mode={:?}, map={:02X?})", dev.get_mode(), dev.spy_get_map());
    match print_spy_events(&mut dev, Duration::from_secs(secs)) {
        Ok(n) => println!("{n} event(s)"),
        Err(e) => println!("device read failed: {e}"),
    }
}

fn release_sequence(dev: &G502) -> Result<(bool, String)> {
    let _ = dev.spy_stop();
    let _ = dev.spy_set_map(&IDENTITY_MAP);
    dev.set_mode(MODE_ONBOARD)?;
    let m = dev.get_mode()?;
    Ok((m == MODE_ONBOARD, format!("read mode {m}")))
}

fn release() -> i32 {
    if win::ghub_running() {
        eprintln!("lghub_agent.exe is running: G HUB owns the mouse. Nothing was written.");
        return 2;
    }
    let api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("HID init failed: {e}");
            return 1;
        }
    };
    let dev = match G502::connect(&api) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("G502 HERO not found: {e}");
            return 1;
        }
    };
    let mut rep = Report::default();
    match snapshot(&dev) {
        Ok(s) => print_snapshot("before", &s),
        Err(e) => println!("snapshot failed: {e}"),
    }
    rep.step("release", release_sequence(&dev));
    match snapshot(&dev) {
        Ok(s) => print_snapshot("after", &s),
        Err(e) => {
            rep.failed = true;
            println!("FAIL after snapshot: error {e}");
        }
    }
    i32::from(rep.failed)
}

fn takeover_test() -> i32 {
    if win::ghub_running() {
        eprintln!("lghub_agent.exe is running: G HUB owns the mouse. Close it first (probe ghub-close). Nothing was written.");
        return 2;
    }
    if win::processes().iter().any(|(_, n)| n.eq_ignore_ascii_case("ghubx.exe")) {
        eprintln!("ghubx.exe is running: two controllers would fight over the mouse. Quit it first. Nothing was written.");
        return 2;
    }
    let api = match HidApi::new() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("HID init failed: {e}");
            return 1;
        }
    };
    let mut dev = match G502::connect(&api) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("G502 HERO not found: {e}");
            return 1;
        }
    };
    let mut rep = Report::default();

    println!("== 0. snapshot");
    let snap = match snapshot(&dev) {
        Ok(s) => s,
        Err(e) => {
            println!("FAIL 0 snapshot: error {e}");
            return 1;
        }
    };
    print_snapshot("snapshot", &snap);

    println!("== 1. host mode");
    rep.step("1 host mode", (|| {
        dev.set_mode(MODE_HOST)?;
        let m = dev.get_mode()?;
        Ok((m == MODE_HOST, format!("read mode {m}")))
    })());

    println!("== 2. identity spy map");
    rep.step("2 identity map", set_map_checked(&dev, &IDENTITY_MAP));

    println!("== 3. spy events (3000 ms, press buttons)");
    rep.step("3 spy", (|| {
        dev.spy_start()?;
        let count = print_spy_events(&mut dev, Duration::from_millis(3000))?;
        Ok((true, format!("{count} event(s)")))
    })());

    println!("== 4. mute round trip (slot {MUTE_SLOT} = G9)");
    rep.step("4 mute round trip", (|| {
        let mut muted = IDENTITY_MAP;
        muted[MUTE_SLOT] = 0;
        let a = set_map_checked(&dev, &muted)?;
        let b = set_map_checked(&dev, &IDENTITY_MAP)?;
        Ok((a.0 && b.0, format!("muted: {} | restored: {}", a.1, b.1)))
    })());

    println!("== 5. dpi");
    rep.step("5 dpi", (|| {
        let mut all = true;
        let mut log = Vec::new();
        for target in [snap.dpi, snap.dpi.saturating_add(50), snap.dpi] {
            dev.set_dpi(target)?;
            let got = dev.get_dpi()?.0;
            all &= got == target;
            log.push(format!("set {target} read {got}"));
        }
        Ok((all, log.join(" | ")))
    })());

    println!("== 6. report rate");
    rep.step("6 rate", (|| {
        dev.set_rate_ms(snap.rate_ms)?;
        let got = dev.get_rate_ms()?;
        Ok((got == snap.rate_ms, format!("set {} read {got}", snap.rate_ms)))
    })());

    println!("== 7. LEDs");
    rep.step("7 leds", (|| {
        let led = Lighting { effect: Effect::Cycle, period_ms: 8000, brightness: 100, ..Default::default() }.to_led();
        let mut all = true;
        let mut log = Vec::new();
        for z in 0..2u8 {
            dev.set_zone_effect(z, &led, false)?;
            let got = dev.get_zone_effect(z)?;
            let effect = Lighting::from_led(&got).map(|l| l.effect);
            all &= effect == Some(Effect::Cycle);
            log.push(format!("zone{z} wrote {led:02X?} read {got:02X?} effect {effect:?}"));
        }
        Ok((all, log.join(" | ")))
    })());

    println!("== 8. release");
    rep.step("8 release", release_sequence(&dev));
    match snapshot(&dev) {
        Ok(s) => print_snapshot("final", &s),
        Err(e) => {
            rep.failed = true;
            println!("FAIL final snapshot: error {e}");
        }
    }

    println!("{}", if rep.failed { "RESULT: FAIL" } else { "RESULT: PASS" });
    i32::from(rep.failed)
}
