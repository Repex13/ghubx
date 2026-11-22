# GHUB-X

> [!WARNING]
> **Still in testing.** This is a personal project under active development. It
> talks to real hardware: it can remap your mouse buttons, write to the mouse's
> on-board memory, and inject keyboard/mouse input. It has been tested on exactly
> one PC and one mouse. Use it at your own risk and read the safety notes below.

GHUB-X is a desktop control app for the **Logitech G502 HERO** (USB `046D:C08B`),
written in Rust with egui. It reproduces the everyday feature set of Logitech
G HUB — profiles, button assignments, macros, DPI, report rate, LIGHTSYNC
lighting, per-game detection — and adds extras inspired by the Razer Basilisk
line, plus OBS Studio actions and per-profile Lua scripting.

It is not affiliated with Logitech or Razer.

## Status

- Works on this machine, in daily use by its author.
- Device takeover, DPI/rate/LED writes and one on-board slot write have been
  exercised on the real mouse and verified by automated probes.
- Macros, remaps, Lua scripts and Perfect 180 have **not** all been exercised
  on the physical buttons yet.
- On-board writes are backed up, read back, and restored on mismatch, but they
  are still writes to firmware-visible memory.
- If something kills GHUB-X while it controls the mouse, run
  `probe release` to put the mouse back in on-board mode.

## Features

- **Assignments:** remap all 11 buttons + wheel tilt, a G-SHIFT layer, key
  combos, text typing, system commands (media keys, volume...), launches,
  macros and OBS actions.
- **Macros:** all four G HUB types (no repeat, repeat while holding, toggle,
  ON PRESS / WHILE HOLDING / ON RELEASE sequence) in a G HUB-style full-screen
  editor with recording, delays and per-step editing.
- **Sensitivity:** up to 5 DPI stages, DPI shift, report rate, optional DPI
  flash and on-screen indicator.
- **LIGHTSYNC:** fixed colour, cycle, breathing, plus host-driven SCREEN
  SAMPLER (including your own sample rectangles, G HUB "Edit Samples" style)
  and AUDIO VISUALIZER. Host-driven colours are written to RAM only, never
  flashed. Honors Windows Dynamic Lighting.
- **Profiles:** per-app profiles with automatic switching on the focused .exe,
  persistent profile, import from G HUB's `settings.db`, `.ghubx` export/import.
- **On-board memory:** read the slot directory, and write profiles into slots
  1-5 (byte-exact encoding, verified against G HUB's own writes).
- **Extras:** Perfect 180 quick turn with per-game calibration (type the
  sensitivity value the game displays), sensitivity clutch, scroll
  acceleration, OBS WebSocket actions (stream/record/mute/scene...), and
  per-profile Lua scripts using G HUB's `OnEvent` API.

## Requirements

- Windows 10/11.
- A wired Logitech G502 HERO.
- Optional: Logitech G HUB installed — GHUB-X can import your existing setup
  and reads G HUB's fonts and device images from the install at runtime
  (no Logitech assets are shipped in this repo).

## Build & run

```
cargo build --release
target\release\ghubx.exe
```

While G HUB is running, GHUB-X only observes the mouse. CLOSE G HUB inside the
app to hand control to GHUB-X; Settings > G HUB can start G HUB again.

Useful probes: `cargo run --bin probe` lists them (`probe takeover-test`,
`probe onboard`, `probe obs-status`, `probe listen`, ...).

## Honest limitations

- Only the G502 HERO. Other mice would need their own protocol tables.
- No Logitech online services: accounts, community profiles, game library,
  firmware updates, notifications.
- Perfect 180 sends input through `SendInput`; games with kernel anti-cheat
  (e.g. Valorant) will ignore it. It will never try to bypass that.
- Lua covers the useful `OnEvent` API surface, not every G HUB helper; no
  syntax highlighting.
- On-board writes cover what fits in a slot; macros/Lua/OBS stay host-side.

## License

No license file yet — treat as all-rights-reserved personal code for now.
