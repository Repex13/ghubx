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

- **Input-sender fallback chain** `[proposed]` — make the injection path
  pluggable behind `engine::Input`: `SendInput` → legacy `mouse_event`
  → `PostMessage`, chosen per-game in the P180 panel with a "test which
  your game accepts" button. This helps games with *flaky* handling of
  one API.
- **Hard boundary** `[won't bypass]` — anti-cheat-blocked games
  (Valorant/Vanguard and peers) drop *all* user-mode injection; no
  sender in the chain changes that. The only past-the-wall options are
  kernel/interception drivers or firmware spoofing: anti-cheat
  circumvention, ban-risk, out of scope. No work on this.
- **Honest answers for blocked games** — (a) the Sensitivity Clutch +
  a physical flick: real hardware movement, works everywhere, it's what
  the clutch is for; (b) on-board can't help — G502 on-board macros emit
  key/button events only, never movement deltas; (c) per-game "blocked"
  marker in the P180 panel so you don't calibrate a game that can't see
  the input.
- **FLICK 180 — the on-board hardware version** `[decided]` — assign the
  mouse's *native* on-board DPI Shift function to a button + set the
  stored shift DPI so a ~2–3 cm physical swipe = 180° at the calibrated
  game sens. Written into a slot: zero software running, works in every
  game incl. Vanguard-blocked ones, impossible to detect because it IS
  the sensor. The G502 can't emit movement on its own, so the flick
  stays physical — this is the legal ceiling for "one-button turns".

## 3. Remaining G HUB parity gaps

- **On-board slot manager** `[proposed]` — map any of your profiles to
  slots 1-5 and write them in one pass; slot 1 already proven.
- **Settings watcher** `[proposed]` — notice `settings.db` changing
  (you tweaked G HUB) and offer a one-click re-import diff.
- **Lua parity** `[proposed]` — syntax highlighting, unsaved-changes
  prompt, remaining API (`SetMKeyState`, `GetMKeyState`, `AbortMacro`,
  ...). Editor currently plaintext; documented gap.
- **Macro polish** `[proposed]` — recording indicator, loop-count
  limits, random delay ranges (a GHUB-X extra G HUB lacks).
- `[won't]` Community profiles, Logitech account sync, firmware updates,
  other devices — need Logitech's servers or different hardware. Keep
  G HUB installed-but-closed for firmware checks.

## 4. Quality of life `[proposed]`

- **Tray quick-menu** `[done TBD]` — right-click tray icon: switch profile,
  current DPI, who controls the mouse (GHUB-X / G HUB / on-board).
- **Profile hotkey** `[done TBD]` — a combo that cycles profiles without
  opening.
- **Auto-backup** `[done TBD]` — snapshot `config.toml` + on-board backup
  into `backups/` on every write/import, keep last 10.
- **Diagnostics page** `[done TBD]` — firmware version, HID++ feature table,
  one-click "copy report" for debugging.

## 5. Lighting extras `[proposed]`

- **Multi-monitor Screen Sampler** — pick which monitor a sample watches.
- **Audio Visualizer bands** — bass on logo, mids on side strip.
- **Reactive lighting** — light up on button press (host-driven, RAM only).

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
