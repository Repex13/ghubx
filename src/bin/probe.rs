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
        Some("export-theme") => std::process::exit(export_theme_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("import-theme") => std::process::exit(import_theme_cmd(&std::env::args().skip(2).collect::<Vec<_>>())),
        Some("game-sens") => {
            for (name, id) in [("Counter-Strike 2", Some("ca572e6a-c76d-4e34-bd64-3665c0d0b0c1")), ("Valorant", Some("48574816-5d5d-4e5b-b616-9c3af2c15bbf"))] {
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
        Some(other) => {
            eprintln!("unknown subcommand {other:?}; use ghub-status, ghub-close, ghub-start, takeover-test, release or listen");
            std::process::exit(64);
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
