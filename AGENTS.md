# GHUB-X (crate `ghubx`)

G HUB-style control app for the Logitech G502 HERO (USB `046D:C08B`), Rust + egui 0.37.

## Commands

- Build: `cargo build` (debug) / `cargo build --release` (no console window, thin LTO)
- Tests: `cargo test` (CRC vs a real onboard sector, spy decoding, firmware string, DPI axis, G HUB import)
- Lint: `cargo clippy --all-targets` (keep it warning-free)
- Device probe (read-only): `cargo run --bin probe`
- Probe subcommands: `probe ghub-status` (is `lghub_agent.exe` running), `probe ghub-close` / `probe ghub-start` (same `win.rs` calls as the UI buttons), `probe takeover-test` (WRITES to the mouse: host mode, spy map, DPI, rate, LEDs, then releases to onboard; exit 2 if G HUB's agent or any `ghubx.exe` is running, 0 only if every step passes), `probe release` (WRITES: stop spy, identity map, onboard mode; exit 2 if G HUB's agent is running)
- `probe listen [secs]` (read-only): prints 0x8110 button events, also next to a running GHUB-X or G HUB
- Env `GHUBX_CONFIG_DIR`: base dir instead of `%APPDATA%` (config is `<dir>\ghubx\config.toml`); point `--shot` runs at a frozen copy so live config changes can't alter screenshots
- Shot flags (in memory only): `--theme <id>`, `--accent <RRGGBB>`, `--follow-lighting`, `--switch-to <id>` (switches theme on frame 10 via the same path as the Appearance tiles)
- Flags: `--minimized` (start hidden in the tray), `--shot <page> <out.png>` (no mutex, no tray)
- Page screenshots without touching the mouse: `target/debug/ghubx.exe --shot <page> <out.png>`
  pages: home, dropdown, assign, assign2, gshift, select, keys, actions, macro, macroedit, sens, light, fixed, devset, profiles, appdetail, settings, sleep (settings with lighting sleep on), appearance, diag
- Kill a running instance before rebuilding: `taskkill //IM ghubx.exe //F`

## Layout

- `src/hidpp.rs` HID++ 2.0 transport (short 0x10 on usage 0x0001, long 0x11 + all input on usage 0x0002)
- `src/device.rs` G502 operations (DPI 0x2201, report rate 0x8060 in ms, onboard 0x8100, LEDs 0x8070, button spy 0x8110)
- `src/profile.rs` data model (apps -> profiles -> base/G-Shift layers, DPI table, lighting, macros) + onboard sector CRC
- `src/engine.rs` takeover policy (only when G HUB's `lghub_agent.exe` isn't running), spy bitmap -> actions
- `src/io.rs` HID owner thread; `src/win.rs` SendInput, foreground app, G HUB process control, autostart, wheel hook
- Tray / background (`win.rs`, pure Win32): hidden top-level window (`WS_EX_TOOLWINDOW | WS_POPUP`, never shown, so it receives `TaskbarCreated` and `WM_ENDSESSION` broadcasts) of class `GHUBX_TRAY` on its own thread; `WM_ENDSESSION` (wParam != 0) runs the quit path so a shutdown/logoff releases the mouse; mutex `Local\GHUBX-single-instance` (a second launch finds the tray window with `FindWindowW` (not message-only), posts `WM_APP+2` and exits). Messages: `WM_APP+1` tray callback (left click/double click show, right click menu Open/Quit), `WM_APP+2` show main window, `WM_APP+3` quit. Show/hide use raw `ShowWindow` (eframe runs no frames while hidden, so the engine/IO threads must never depend on the UI loop; `ui::spawn_stash` keeps the UiEvent channel drained while hidden). Shutdown is one function, `ui::Exit::run` (save config, engine Quit, wait IO thread, remove icon), used by `on_exit` and tray Quit. `win::tray_ready()` gates close-to-tray and `--minimized` hiding (no tray = window stays reachable). `--minimized` starts the window at (-32000,-32000), hides it after eframe's first frame and re-centres it on the work area. One icon: `win::app_icon_rgba(size)` (tray 32, window 64). Control watchdog: the IO thread's periodic refresh sends `EngineMsg::ControlLost` if the mouse left host mode while controlled; the engine retries after 1 s. Config: `general.close_to_tray` (default on), `general.start_minimized` (adds ` --minimized` to the autostart Run entry)
- `src/ghub_import.rs` imports `%LOCALAPPDATA%\LGHUB\settings.db` (copied to temp first)
- Themes: `src/ui/palette.rs` holds `Palette` (global `pal()` / `set_palette`), 5 presets (ids `ghub-dark`, `ghub-light`, `razer-green`, `midnight`, `ember`; `ghub-dark` is the original G HUB look verbatim), `n(level)` neutral ramp, `ink(alpha)`, `with_accent`, `resolve(&Appearance, lighting_accent)`. No `Color32` literals in `src/ui` except images/renders, the colour picker, LEDs, posters, shadows and the close-button red. `App::ui` recomputes the palette every frame (config `[appearance] theme/accent/follow_lighting`; follow-lighting uses the logo zone) and restyles when it changes. Settings > Appearance is tab 1 (Diagnostics is now tab 2, About 3)
- `src/ui/` G HUB look: fonts from `C:\Program Files\LGHUB\resources\app.asar`, renders + hotspots from `C:\ProgramData\LGHUB\depots\*\g502_hero`

## Rules

- Never write spy map / host mode while G HUB is running (it owns the mouse); `--shot` forces `take_control = false`.
- Don't ship Logitech assets; they are read from the local G HUB install at runtime.
- `write_sector` (flash) exists but is not exposed in the UI; verify on hardware before wiring it up.
- Config: `%APPDATA%\ghubx\config.toml`.
- Live check with G HUB closed: `probe ghub-close`, then `probe takeover-test` (all 8 steps passed on 2026-11-21). `probe release` is the rescue command (mouse back to onboard mode). Onboard mode on this mouse is 3200 DPI, so the pointer speeds up whenever no software controls it.
