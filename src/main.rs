//! GHUB-X — a G HUB-style control centre for the Logitech G502 HERO with
//! Razer Basilisk V4–inspired extras (Perfect 180, sensitivity clutch,
//! scroll acceleration).
//!
//! Flags: `--minimized` (autostart), `--shot <page> <out.png>` (render a
//! page to PNG and exit; never takes control of the mouse).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use crossbeam_channel::unbounded;
use ghubx::engine::{DevCmd, Engine, EngineMsg, UiEvent};
use ghubx::profile::Config;
use ghubx::ui::{Exit, GhubApp, Shot};
use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use std::sync::{Arc, Mutex};

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let shot = args
        .iter()
        .position(|a| a == "--shot")
        .and_then(|i| Some(Shot::new(args.get(i + 1)?.clone(), args.get(i + 2)?.into())));
    let minimized = args.iter().any(|a| a == "--minimized") && shot.is_none();
    if shot.is_none() && !ghubx::win::single_instance() {
        return Ok(());
    }

    // First run (or a config from an older version): bring over the G HUB setup.
    let mut welcome = None;
    let mut config = match Config::load_existing() {
        Some(c) => c,
        None => {
            let mut c = Config::default();
            if let Ok(s) = ghubx::ghub_import::import(&mut c) {
                welcome = Some(format!("Imported your G HUB setup ({} macros)", s.macros));
                if shot.is_none() {
                    let _ = c.save();
                }
            }
            c
        }
    };
    if shot.is_some() {
        config.general.take_control = false;
    }
    let cfg = Arc::new(Mutex::new(config));

    let (dev_tx, dev_rx) = unbounded::<DevCmd>();
    let (eng_tx, eng_rx) = unbounded::<EngineMsg>();
    let (ui_tx, ui_rx) = unbounded::<UiEvent>();
    let (done_tx, done_rx) = crossbeam_channel::bounded::<()>(1);
    {
        let (eng_tx, ui_tx) = (eng_tx.clone(), ui_tx.clone());
        std::thread::Builder::new()
            .name("hid-io".into())
            .spawn(move || {
                ghubx::io::run(dev_rx, eng_tx, ui_tx);
                let _ = done_tx.send(());
            })
            .expect("spawn io thread");
    }
    {
        let engine = Engine::new(cfg.clone(), dev_tx, ui_tx);
        std::thread::Builder::new().name("engine".into()).spawn(move || engine.run(eng_rx)).expect("spawn engine thread");
    }

    let mut viewport = egui::ViewportBuilder::default()
        .with_title("GHUB-X")
        .with_app_id("ghubx")
        .with_inner_size([1024.0, 768.0])
        .with_min_inner_size([1024.0, 768.0])
        .with_decorations(false)
        .with_resizable(true)
        .with_icon(egui::IconData { rgba: ghubx::win::app_icon_rgba(64), width: 64, height: 64 });
    if minimized {
        viewport = viewport.with_position([-32000.0, -32000.0]);
    }
    let options = eframe::NativeOptions { viewport, ..Default::default() };
    let exit = Arc::new(Exit::new(cfg.clone(), eng_tx.clone(), done_rx, shot.is_none()));
    eframe::run_native(
        "GHUB-X",
        options,
        Box::new(move |cc| {
            if shot.is_none()
                && let Ok(RawWindowHandle::Win32(h)) = cc.window_handle().map(|h| h.as_raw())
            {
                let (ctx, exit) = (cc.egui_ctx.clone(), exit.clone());
                ghubx::win::start_tray(
                    h.hwnd.get(),
                    minimized,
                    move || ctx.request_repaint(),
                    move || {
                        exit.run();
                        std::process::exit(0);
                    },
                );
            }
            let mut app = GhubApp::new(cc, cfg, eng_tx, ui_rx, exit, shot);
            if let Some(w) = welcome {
                app.notify(w);
            }
            Ok(Box::new(app))
        }),
    )
}
