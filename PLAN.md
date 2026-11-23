# GHUB-X roadmap

Everything discussed but **not started** yet. Status tags: `[proposed]`
(idea on the table), `[decided]` (approved by Repex, ready to build),
`[blocked]` (needs something external first), `[won't]` (out of scope,
with the reason).

## 1. Auto-update + notifications `[done 73ff037]`

Now that the repo is public (`https://github.com/Repex13/ghubx`):

- **GitHub Actions CI** — `.github/workflows/ci.yml` runs `cargo test` +
  `clippy` on every push; `release.yml` builds `ghubx.exe` on a `v*` tag
  and attaches the exe + `sha256.txt` to a GitHub Release. Free on public
  repos; makes the updater possible.
- **Update checker** — on startup and ~daily, GET
  `api.github.com/repos/Repex13/ghubx/releases/latest`, compare with a
  `GHUBX_VERSION` baked into the build. One HTTPS request (WinINet via the
  `windows` crate, or `ureq` if we take a dep). 60 req/h unauthenticated
  is plenty.
- **Self-update flow** — download `ghubx.exe` to
  `%LOCALAPPDATA%\ghubx\updates\ghubx.new.exe`, verify sha256, rename the
  running exe (Windows allows it), move the new one in, restart. ~0.5 s
  with the mouse on its on-board profile. Same swap we already do by hand.
- **Notification feed** — G HUB's bell, locally: unread dot on a header
  icon + a dropdown list. Feeds "update available", "G HUB started —
  GHUB-X now observing", "OBS disconnected", "profile switched to X".
  Optional Windows toast later; in-app feed first.

## 2. Perfect 180 compatibility `[decided with boundary]`

- **Input-sender fallback chain** `[done bdae9ae]` — the injection path is
  pluggable behind `engine::Input`: `SendInput` → legacy `mouse_event`
  → `PostMessage`, picked in the P180 panel (`Extras.p180_sender`) and
  tried with the TEST button. For games with *flaky* handling of one
  API only — the move path is user-mode senders, nothing else.
- **Hard boundary** `[won't bypass]` — anti-cheat-blocked games
  (Valorant/Vanguard and peers) drop *all* user-mode injection; no
  sender in the chain changes that. The only past-the-wall options are
  kernel/interception drivers or firmware spoofing: anti-cheat
  circumvention, ban-risk, out of scope. No work on this.
- **Honest answers for blocked games** `[done bdae9ae]` — (a) the Sensitivity
  Clutch + a physical flick: real hardware movement, works everywhere,
  it's what the clutch is for; (b) on-board can't help — G502 on-board
  macros emit key/button events only, never movement deltas; (c) the
  per-game "GAME BLOCKS IT" marker in the P180 panel
  (`Extras.p180_blocked`) so you don't calibrate a game that can't see
  the input — a reminder, not a lock: TEST still fires.
- **FLICK 180 — the on-board hardware version** `[decided]` — assign the
  mouse's *native* on-board DPI Shift function to a button + set the
  stored shift DPI so a ~2–3 cm physical swipe = 180° at the calibrated
  game sens. Written into a slot: zero software running, works in every
  game incl. Vanguard-blocked ones, impossible to detect because it IS
  the sensor. The G502 can't emit movement on its own, so the flick
  stays physical — this is the legal ceiling for "one-button turns".
- **READ FROM GAME** `[done TBD]` — the P180 panel's IN-GAME SENS fills
  from the game's own settings file: CS2's `cs2_user_convars` vcfg,
  Valorant's `RiotUserSettings.ini`, and now Fortnite — whose sens lives
  in the UE `ClientSettings.Sav` GVAS blob (`MouseSensitivityX`/`Y`
  FloatProperties; `GameUserSettings.ini` holds none, legacy
  `MouseSensitivity` is -1). `games::preset_sensitivity` maps the
  selected preset to the parser, and the file's decimal runs through the
  same preset-percentage maths as typing it.
- **Live Lua sender** `[done TBD]` — `engine::P180_SENDER`, a
  process-wide atomic `SetSender` overwrites; the Lua worker's
  `RealInput` reads it for every `MoveMouse` dispatch, so a sender pick
  changed after spawn applies without rebuilding the worker.

## 3. Remaining G HUB parity gaps

- **On-board slot manager** `[done a4842c0]` — map any of your profiles to
  slots 1-5 and write them in one pass; slot 1 already proven.
  Device Settings > ASSIGN PROFILES TO SLOTS opens an overlay with five
  dropdowns pre-filled from the read directory; one pending action feeds
  the same gated WRITE TO MOUSE bar.
- **Settings watcher** `[done a4842c0]` — notice `settings.db` changing
  (you tweaked G HUB) and offer a one-click re-import diff. The watcher
  (`ghub_watch::DbWatch`, ~5 s poll, 2 s quiet debounce) sets a CHANGED
  hint beside IMPORT and drops a feed note.
- **Import preview diff** `[done TBD]` — the first IMPORT click now runs
  `ghub_import::preview` (the db copy + convert against a throwaway copy
  of the config, nothing written) and lists `import_diff`'s lines
  (+/- apps, profiles per app, macro and retune counts) with a DISMISS
  link; the confirming click installs exactly what the diff showed.
- **Lua parity** `[done a4842c0]` — syntax highlighting, unsaved-changes
  prompt, remaining API (`SetMKeyState`, `GetMKeyState`, `AbortMacro`,
  `OutputDebugMessage`, `IsModifierPressed`, `MoveMouseTo` done; anything
  still impossible sits on the explicit unsupported list). The editor
  colours source and shows an UNSAVED CHANGES bar.
- **Macro polish** `[done a4842c0]` — recording indicator, loop-count
  limits, random delay ranges (a GHUB-X extra G HUB lacks). Done as
  `Macro.repeat_count` (UNTIL RELEASED / N TIMES) plus per-step `jitter`
  edited next to each delay chip.
- `[won't]` Community profiles, Logitech account sync, firmware updates,
  other devices — need Logitech's servers or different hardware. Keep
  G HUB installed-but-closed for firmware checks.

## 4. Quality of life `[proposed]`

- **Tray quick-menu** `[done 5e67bc3]` — right-click tray icon: switch profile,
  current DPI, who controls the mouse (GHUB-X / G HUB / on-board).
- **Profile hotkey** `[done 5e67bc3]` — a combo that cycles profiles without
  opening.
- **Auto-backup** `[done 5e67bc3]` — snapshot `config.toml` + on-board backup
  into `backups/` on every write/import, keep last 10.
- **Diagnostics page** `[done 5e67bc3]` — firmware version, HID++ feature table,
  one-click "copy report" for debugging.

## 5. Lighting extras `[done 92f14ea]`

- **Multi-monitor Screen Sampler** `[done 92f14ea]` — `win::monitors()` lists every
  monitor in virtual-screen coordinates (primary first, negative origins kept),
  `HostFx.monitor` picks which one a zone samples, and the panel gets a MONITOR
  pill row. `probe fx-screen` prints each monitor's rect + a capture sanity line.
- **Audio Visualizer bands** `[done 92f14ea]` — `AudioBand::{Bass, Mids, All}`; the
  loopback `Bands` splitter now also isolates ~300 Hz-3 kHz. The old
  "PULSE ON BASS ONLY" checkbox became three BAND pills; `bass_only` configs
  migrate to `band = "Bass"` on load.
- **Reactive lighting** `[done 92f14ea]` — `Effect::Reactive` (host-driven, RAM
  only): the zone's colour fades in while any captured button is held and for
  `host.hold_ms` (default 300 ms) after release; the engine feeds
  `fx::Handle::set_reactive_down` from every spy-mask change.
- **Monitor-aware sample preview** `[done TBD]` —
  `fx::screen_picture(monitor, ..)` captures whichever monitor the zone's
  `HostFx.monitor` picks (virtual coords, negative origins fine,
  out-of-range falls back to primary) and the Edit Samples overlay draws
  that monitor behind the rectangles. `probe fx-sample` stays
  primary-only.

## 6. Bigger items `[proposed / uncertain]`

- **Discord / Streamlabs actions** — each is a separate local-API
  integration; sizeable.
- **Lift-off distance** — may exist in HERO firmware, needs reversing
  first; surface tuning is not exposed on this sensor.
- `[won't]` Kernel-level input transforms (sens curves, angle snapping,
  smoothing) — needs a driver.

## 7. Open validation `[blocked on user]`

- Physical button checklist (G4/G5/G6, DPI steps, tilt, replug).
- Swap the live app to the current build + decide G HUB vs GHUB-X at
  startup.
- Live OBS test — needs OBS opened once with WebSocket enabled.
- Perfect 180 per-game calibration in real matches.
