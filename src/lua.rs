//! Per-profile Lua scripting with G HUB's `OnEvent(event, arg)` API.
//!
//! Every profile can hold a script (`Profile.script`) that runs while GHUB-X
//! controls the mouse. `mlua::Lua` is `!Send`, so one worker thread owns the
//! state; the engine feeds it `LuaMsg::Script` whenever the active profile's
//! script changes and `LuaMsg::Event` for every spy-bitmap change. All input
//! the script produces goes through the `Arc<dyn engine::Input>` passed to
//! `spawn`, so tests hear the same recorder the engine tests use and nothing
//! here calls `SendInput` directly. `--dev`/`--shot` and scripted UI runs use
//! `Handle::fixed()`: no thread, no Lua state, messages dropped.

use mlua::{HookTriggers, Lua, Variadic, VmState};
use std::cell::{Cell, RefCell};
use std::collections::VecDeque;
use std::rc::Rc;
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::engine;
use crate::profile::Assignment;

/// Console lines kept for the editor's CONSOLE pane.
const CONSOLE_CAP: usize = 200;
/// VM instructions a single `OnEvent` call may execute before it is killed
/// (~20M; a `while true do end` must not hang the worker).
const EVENT_BUDGET: u64 = 20_000_000;
/// The instruction hook fires this often and spends that much of the budget.
const HOOK_EVERY: u32 = 1024;

/// Commands for the Lua worker (see [`Handle::send`]).
#[derive(Debug, Clone, PartialEq)]
pub enum LuaMsg {
    /// New script text for the running profile ("" stops it).
    Script(String),
    /// OnEvent name and argument, e.g. ("G_PRESSED", 4).
    Event(&'static str, i32),
    /// Terminal cleanup (engine stop / losing control).
    Stop,
}

#[derive(Default)]
struct Shared {
    lines: VecDeque<String>,
    /// Bumped on every console change; the editor reflows on change only.
    rev: u64,
    /// `EnablePrimaryMouseButtonEvents` of the live script instance.
    primary: bool,
    /// G HUB's virtual modifier bank `SetMKeyState`/`GetMKeyState` (M1..M3):
    /// worker state, so it survives a script reload like G HUB's does.
    mkeys: [bool; 3],
}

/// Owns the worker channel and the shared console; cloning shares both.
#[derive(Clone)]
pub struct Handle {
    tx: Option<Sender<LuaMsg>>,
    shared: Arc<Mutex<Shared>>,
}

impl Handle {
    /// Worker with the real key-state queries and no PlayMacro loopback.
    pub fn spawn(input: Arc<dyn engine::Input>) -> Self {
        Self::spawn_with(input, Arc::new(crate::win::is_vk_down), None)
    }

    /// `key_down` answers IsKeyPressed/IsMouseButtonPressed (tests stub it);
    /// `loopback` feeds `PlayMacro` back to the engine as a `Tap`.
    pub fn spawn_with(input: Arc<dyn engine::Input>, key_down: Arc<dyn Fn(u16) -> bool + Send + Sync>, loopback: Option<crossbeam_channel::Sender<engine::EngineMsg>>) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = channel::<LuaMsg>();
        let s = shared.clone();
        std::thread::Builder::new()
            .name("lua".into())
            .spawn(move || run(rx, input, key_down, loopback, s))
            .ok();
        Self { tx: Some(tx), shared }
    }

    /// No thread and no Lua state: `--dev`, `--shot` and scripted UI runs.
    pub fn fixed() -> Self {
        Self { tx: None, shared: Arc::new(Mutex::new(Shared::default())) }
    }

    pub fn send(&self, m: LuaMsg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(m);
        }
    }

    /// (revision, console lines) — the editor reflows only when the revision changes.
    pub fn console(&self) -> (u64, Vec<String>) {
        let g = self.shared.lock().unwrap();
        (g.rev, g.lines.iter().cloned().collect())
    }

    /// CLEAR OUTPUT.
    pub fn clear(&self) {
        let mut g = self.shared.lock().unwrap();
        g.lines.clear();
        g.rev += 1;
    }
}

/// A live script instance; all of it stays on the worker thread.
struct Live {
    lua: Lua,
    /// VM instructions left in the current `OnEvent` budget (reset per event).
    budget: Rc<Cell<u64>>,
}

fn push(shared: &Arc<Mutex<Shared>>, msg: impl AsRef<str>) {
    let mut g = shared.lock().unwrap();
    for line in msg.as_ref().split('\n') {
        g.lines.push_back(line.to_string());
    }
    while g.lines.len() > CONSOLE_CAP {
        g.lines.pop_front();
    }
    g.rev += 1;
}

fn run(rx: Receiver<LuaMsg>, input: Arc<dyn engine::Input>, key_down: Arc<dyn Fn(u16) -> bool + Send + Sync>, loopback: Option<crossbeam_channel::Sender<engine::EngineMsg>>, shared: Arc<Mutex<Shared>>) {
    let mut live: Option<Live> = None;
    for m in rx {
        match m {
            LuaMsg::Script(text) => {
                if let Some(old) = live.take() {
                    event(&old, "PROFILE_DEACTIVATED", 0, &shared);
                }
                shared.lock().unwrap().primary = false;
                live = if text.is_empty() { None } else { load(&text, &input, &key_down, loopback.as_ref(), &shared) };
            }
            LuaMsg::Event(name, arg) => {
                let Some(l) = &live else { continue };
                if name.starts_with("MOUSE_BUTTON") && !shared.lock().unwrap().primary {
                    continue;
                }
                event(l, name, arg, &shared);
            }
            LuaMsg::Stop => {
                if let Some(old) = live.take() {
                    event(&old, "PROFILE_DEACTIVATED", 0, &shared);
                }
                shared.lock().unwrap().primary = false;
            }
        }
    }
}

/// Build a script instance: API globals, hook, `exec`, `PROFILE_ACTIVATED`.
fn load(text: &str, input: &Arc<dyn engine::Input>, key_down: &Arc<dyn Fn(u16) -> bool + Send + Sync>, loopback: Option<&crossbeam_channel::Sender<engine::EngineMsg>>, shared: &Arc<Mutex<Shared>>) -> Option<Live> {
    let lua = Lua::new();
    let started = Instant::now();
    let budget = Rc::new(Cell::new(EVENT_BUDGET));
    {
        let left = budget.clone();
        if let Err(e) = lua.set_hook(HookTriggers::new().every_nth_instruction(HOOK_EVERY), move |_, _| {
            let next = left.get().saturating_sub(u64::from(HOOK_EVERY));
            left.set(next);
            if next == 0 { Err(mlua::Error::RuntimeError("script too slow".into())) } else { Ok(VmState::Continue) }
        }) {
            push(shared, format!("script setup failed: {e}"));
            return None;
        }
    }
    let reported = Rc::new(RefCell::new(Vec::new()));
    if let Err(e) = api(&lua, input, key_down, loopback, shared, started, reported.clone()) {
        push(shared, format!("script setup failed: {e}"));
        return None;
    }
    if let Err(e) = lua.load(text).exec() {
        push(shared, e.to_string());
        return None;
    }
    let live = Live { lua, budget };
    event(&live, "PROFILE_ACTIVATED", 0, shared);
    Some(live)
}

/// Run the script's `OnEvent`; errors land on the console, the script stays alive.
fn event(l: &Live, name: &'static str, arg: i32, shared: &Arc<Mutex<Shared>>) {
    l.budget.set(EVENT_BUDGET);
    let Ok(f) = l.lua.globals().get::<mlua::Function>("OnEvent") else { return };
    if let Err(e) = f.call::<()>((name, arg)) {
        push(shared, e.to_string());
    }
}

fn unsupported(sh: &Arc<Mutex<Shared>>, rep: &Rc<RefCell<Vec<&'static str>>>, name: &'static str) {
    let mut r = rep.borrow_mut();
    if !r.contains(&name) {
        r.push(name);
        push(sh, format!("{name} is not supported"));
    }
}

/// Register the whole G HUB API surface on a fresh state.
fn api(lua: &Lua, input: &Arc<dyn engine::Input>, key_down: &Arc<dyn Fn(u16) -> bool + Send + Sync>, loopback: Option<&crossbeam_channel::Sender<engine::EngineMsg>>, shared: &Arc<Mutex<Shared>>, started: Instant, reported: Rc<RefCell<Vec<&'static str>>>) -> mlua::Result<()> {
    let g = lua.globals();
    // ---- key and button input through the injected sink --------------------
    for (name, down) in [("PressKey", Some(true)), ("ReleaseKey", Some(false)), ("PressAndReleaseKey", None)] {
        let (input, sh) = (input.clone(), shared.clone());
        g.set(
            name,
            lua.create_function(move |_, names: Variadic<String>| {
                let mut vks = Vec::new();
                for n in names.iter() {
                    match vk_of(n) {
                        Some(vk) => vks.push(vk),
                        None => push(&sh, format!("unknown key '{n}'")),
                    }
                }
                match down {
                    Some(d) => {
                        for &vk in &vks {
                            input.key(vk, d);
                        }
                    }
                    None => {
                        for &vk in &vks {
                            input.key(vk, true);
                        }
                        for &vk in vks.iter().rev() {
                            input.key(vk, false);
                        }
                    }
                }
                Ok(())
            })?,
        )?;
    }
    for (name, down) in [("PressMouseButton", Some(true)), ("ReleaseMouseButton", Some(false)), ("PressAndReleaseMouseButton", None)] {
        let (input, sh) = (input.clone(), shared.clone());
        g.set(
            name,
            lua.create_function(move |_, n: i64| {
                let Some(b) = ghub_button(n) else {
                    push(&sh, format!("unknown mouse button {n}"));
                    return Ok(());
                };
                match down {
                    Some(d) => input.mouse(b, d),
                    None => {
                        input.mouse(b, true);
                        input.mouse(b, false);
                    }
                }
                Ok(())
            })?,
        )?;
    }
    {
        let input = input.clone();
        g.set(
            "MoveMouseRelative",
            lua.create_function(move |_, (dx, dy): (i64, i64)| {
                input.mouse_move(dx as i32, dy as i32);
                Ok(())
            })?,
        )?;
    }
    {
        let input = input.clone();
        g.set(
            "MoveMouseWheel",
            lua.create_function(move |_, n: i64| {
                input.wheel((n as i32) * 120);
                Ok(())
            })?,
        )?;
    }
    g.set(
        "SetMousePosition",
        lua.create_function(|_, (x, y): (i64, i64)| {
            crate::win::set_cursor_pos(x as i32, y as i32);
            Ok(())
        })?,
    )?;
    g.set(
        "GetMousePosition",
        lua.create_function(|_, ()| {
            let (x, y) = crate::win::cursor_pos();
            Ok((i64::from(x), i64::from(y)))
        })?,
    )?;
    // ---- timing and log ------------------------------------------------------
    g.set(
        "Sleep",
        lua.create_function(|_, ms: i64| {
            std::thread::sleep(Duration::from_millis(ms.max(0) as u64));
            Ok(())
        })?,
    )?;
    g.set("GetRunningTime", lua.create_function(move |_, ()| Ok(started.elapsed().as_millis() as i64))?)?;
    {
        let sh = shared.clone();
        let log = lua.create_function(move |lua, args: Variadic<mlua::Value>| {
            let string: mlua::Table = lua.globals().get("string")?;
            let format: mlua::Function = string.get("format")?;
            let s: String = format.call(args)?;
            push(&sh, s);
            Ok(())
        })?;
        g.set("OutputLogMessage", log)?;
    }
    {
        let sh = shared.clone();
        g.set(
            "OutputDebugMessage",
            lua.create_function(move |lua, args: Variadic<mlua::Value>| {
                let string: mlua::Table = lua.globals().get("string")?;
                let format: mlua::Function = string.get("format")?;
                let s: String = format.call(args)?;
                push(&sh, format!("DEBUG: {s}"));
                Ok(())
            })?,
        )?;
    }
    {
        let sh = shared.clone();
        g.set(
            "ClearLog",
            lua.create_function(move |_, ()| {
                let mut g = sh.lock().unwrap();
                g.lines.clear();
                g.rev += 1;
                Ok(())
            })?,
        )?;
    }
    g.set("DoNothing", lua.create_function(|_, ()| Ok(()))?)?;
    // ---- queries -------------------------------------------------------------
    {
        let (kd, sh) = (key_down.clone(), shared.clone());
        g.set(
            "IsKeyPressed",
            lua.create_function(move |_, name: String| {
                Ok(match vk_of(&name) {
                    Some(vk) => kd(vk),
                    None => {
                        push(&sh, format!("unknown key '{name}'"));
                        false
                    }
                })
            })?,
        )?;
    }
    {
        let kd = key_down.clone();
        g.set("IsMouseButtonPressed", lua.create_function(move |_, n: i64| Ok(ghub_mouse_vk(n).is_some_and(|vk| kd(vk))))?)?;
    }
    {
        let sh = shared.clone();
        g.set(
            "IsKeyLockOn",
            lua.create_function(move |_, name: String| {
                let vk = match name.trim().to_ascii_lowercase().as_str() {
                    "capslock" => 0x14,
                    "numlock" => 0x90,
                    "scrolllock" => 0x91,
                    _ => {
                        push(&sh, format!("unknown lock '{name}'"));
                        return Ok(false);
                    }
                };
                Ok(crate::win::is_vk_toggled(vk))
            })?,
        )?;
    }
    {
        let sh = shared.clone();
        g.set(
            "EnablePrimaryMouseButtonEvents",
            lua.create_function(move |_, on: bool| {
                sh.lock().unwrap().primary = on;
                Ok(())
            })?,
        )?;
    }
    {
        let (kd, sh) = (key_down.clone(), shared.clone());
        g.set(
            "IsModifierPressed",
            lua.create_function(move |_, name: String| {
                Ok(match vk_of(&name) {
                    Some(vk) if matches!(vk, 0xA0..=0xA5 | 0x5B | 0x5C) => kd(vk),
                    Some(_) => {
                        push(&sh, format!("'{name}' is not a modifier"));
                        false
                    }
                    None => {
                        push(&sh, format!("unknown key '{name}'"));
                        false
                    }
                })
            })?,
        )?;
    }
    // ---- G HUB's virtual modifier bank (M1..M3, event-independent) -----------
    {
        let sh = shared.clone();
        g.set(
            "SetMKeyState",
            lua.create_function(move |_, (n, down): (i64, bool)| {
                if (1..=3).contains(&n) {
                    sh.lock().unwrap().mkeys[n as usize - 1] = down;
                } else {
                    push(&sh, format!("unknown M key {n}"));
                }
                Ok(())
            })?,
        )?;
    }
    {
        let sh = shared.clone();
        g.set(
            "GetMKeyState",
            lua.create_function(move |lua, n: Option<i64>| {
                let m = sh.lock().unwrap().mkeys;
                match n {
                    // G HUB: GetMKeyState(m) reports one key; no arg -> a table
                    Some(n) if (1..=3).contains(&n) => Ok(mlua::Value::Boolean(m[n as usize - 1])),
                    _ => {
                        let t = lua.create_table()?;
                        for (i, v) in m.iter().enumerate() {
                            t.set(i + 1, *v)?;
                        }
                        Ok(mlua::Value::Table(t))
                    }
                }
            })?,
        )?;
    }
    // ---- macros and the rest -------------------------------------------------
    {
        let (sh, rep, lb) = (shared.clone(), reported.clone(), loopback.cloned());
        g.set(
            "PlayMacro",
            lua.create_function(move |_, name: String| {
                match &lb {
                    Some(tx) => {
                        let _ = tx.send(engine::EngineMsg::Tap(Assignment::Macro(name)));
                    }
                    None => unsupported(&sh, &rep, "PlayMacro"),
                }
                Ok(())
            })?,
        )?;
    }
    {
        let (sh, rep, lb) = (shared.clone(), reported.clone(), loopback.cloned());
        g.set(
            "AbortMacro",
            // stops every looping macro (toggles and held repeats); queued
            // PlayMacro taps already left this thread and can't be recalled
            lua.create_function(move |_, ()| {
                match &lb {
                    Some(tx) => {
                        let _ = tx.send(engine::EngineMsg::AbortMacros);
                    }
                    None => unsupported(&sh, &rep, "AbortMacro"),
                }
                Ok(())
            })?,
        )?;
    }
    // absolute cursor moves (G HUB's name for SetMousePosition)
    for name in ["MoveMouseTo", "MoveMouseToVirtual"] {
        g.set(
            name,
            lua.create_function(|_, (x, y): (i64, i64)| {
                crate::win::set_cursor_pos(x as i32, y as i32);
                Ok(())
            })?,
        )?;
    }
    // documented G HUB functions we honestly can't provide (logged once each)
    for name in [
        "SetBacklightColor",
        "GetDeviceInfo",
        "GetDate", // std has no strftime; scripts can use os.date
        "DoFile",
        "PressMacro",
        "ReleaseMacro",
        "SetMouseDPITable",
        "SetMouseDPITableIndex",
        "SetMouseSpeed",
        "GetMouseSpeed",
        "IncrementMouseSpeed",
        "DecrementMouseSpeed",
    ] {
        let (sh, rep) = (shared.clone(), reported.clone());
        g.set(
            name,
            lua.create_function(move |_, _: Variadic<mlua::Value>| {
                unsupported(&sh, &rep, name);
                Ok(())
            })?,
        )?;
    }
    Ok(())
}

/// G HUB button number -> `Input::mouse` code (1 left, 2 middle, 3 right, 4 back, 5 forward).
fn ghub_button(n: i64) -> Option<u8> {
    [0u8, 2, 1, 3, 4].get(usize::try_from(n - 1).ok()?).copied()
}

/// G HUB button number -> the physical VK IsMouseButtonPressed reports.
fn ghub_mouse_vk(n: i64) -> Option<u16> {
    [0x01u16, 0x04, 0x02, 0x05, 0x06].get(usize::try_from(n - 1).ok()?).copied()
}

/// G HUB key name ("a", "spacebar", "lshift", "f5", "g1"…) -> virtual key.
fn vk_of(name: &str) -> Option<u16> {
    let n = name.trim().to_ascii_lowercase();
    Some(match n.as_str() {
        "backspace" => 0x08,
        "tab" => 0x09,
        "enter" | "return" => 0x0D,
        "pause" => 0x13,
        "capslock" => 0x14,
        "escape" | "esc" => 0x1B,
        "spacebar" | "space" => 0x20,
        "pageup" => 0x21,
        "pagedown" => 0x22,
        "end" => 0x23,
        "home" => 0x24,
        "left" => 0x25,
        "up" => 0x26,
        "right" => 0x27,
        "down" => 0x28,
        "printscreen" => 0x2C,
        "insert" => 0x2D,
        "delete" => 0x2E,
        "lwin" => 0x5B,
        "rwin" => 0x5C,
        "apps" | "menu" => 0x5D,
        "nummultiply" | "multiply" => 0x6A,
        "numplus" | "add" => 0x6B,
        "numminus" | "subtract" => 0x6D,
        "numdecimal" | "decimal" => 0x6E,
        "numdivide" | "divide" => 0x6F,
        "numlock" => 0x90,
        "scrolllock" => 0x91,
        "lshift" | "shift" => 0xA0,
        "rshift" => 0xA1,
        "lctrl" | "ctrl" => 0xA2,
        "rctrl" => 0xA3,
        "lalt" | "alt" => 0xA4,
        "ralt" => 0xA5,
        "semicolon" => 0xBA,
        "equal" => 0xBB,
        "comma" => 0xBC,
        "minus" => 0xBD,
        "period" => 0xBE,
        "slash" => 0xBF,
        "grave" | "tilde" => 0xC0,
        "leftbracket" => 0xDB,
        "backslash" => 0xDC,
        "rightbracket" => 0xDD,
        "quote" => 0xDE,
        _ => return vk_fallback(&n),
    })
}

fn vk_fallback(n: &str) -> Option<u16> {
    if n.len() == 1 {
        return crate::win::char_key(n.chars().next()?).map(|(vk, _)| vk);
    }
    if let Some(d) = n.strip_prefix('f').and_then(|s| s.parse::<u16>().ok())
        && (1..=24).contains(&d)
    {
        return Some(0x6F + d); // F1..F24
    }
    if let Some(d) = n.strip_prefix("num").and_then(|s| s.parse::<u16>().ok())
        && d <= 9
    {
        return Some(0x60 + d); // num0..num9
    }
    if let Some(d) = n.strip_prefix('g').and_then(|s| s.parse::<u16>().ok())
        && (1..=5).contains(&d)
    {
        // G HUB lets scripts press the physical G buttons too ("g1".."g5").
        return Some([0x01, 0x02, 0x04, 0x05, 0x06][d as usize - 1]);
    }
    None
}

// ------------------------------------------------------------------ lexing
// Shared by the editor's syntax colouring (`ui::lua_editor::highlight_lua`)
// and the managed-block injection (`profile::p180_lua_inject`).

/// What a byte range of a Lua script is coloured/recognised as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    /// Identifiers, operators, punctuation — plain text colour.
    Plain,
    /// `-- line` and `--[[ block ]]` / `--[==[ block ]==]` comments.
    Comment,
    /// `".."` / `'..'` strings and `[[ .. ]]` / `[=[ .. ]=]` long strings.
    Str,
    /// Lua keywords (primary colour, like G HUB's editor).
    Key,
    /// Number literals.
    Num,
}

const KEYWORDS: &[&str] = &["function", "end", "if", "then", "else", "elseif", "for", "while", "do", "local", "return", "true", "false", "nil", "and", "or", "not", "repeat", "until", "in", "break"];

fn is_ident(b: u8) -> bool {
    b.is_ascii_alphanumeric() || b == b'_'
}

/// `[` `=`* `[` — the level of a Lua long bracket at `i`, if there is one.
fn long_bracket(b: &[u8], i: usize) -> Option<usize> {
    if b.get(i) != Some(&b'[') {
        return None;
    }
    let mut n = i + 1;
    while b.get(n) == Some(&b'=') {
        n += 1;
    }
    (b.get(n) == Some(&b'[')).then_some(n - i - 1)
}

/// `]` `=`*level `]` — whether a long-bracket close sits at `i`.
fn bracket_close(b: &[u8], i: usize, level: usize) -> bool {
    b.get(i) == Some(&b']') && (i + 1..=i + level).all(|j| b.get(j) == Some(&b'=')) && b.get(i + level + 1) == Some(&b']')
}

/// Lex `s` into (start byte, end byte, class) spans covering the whole text.
/// Kept deliberately small: Lua's real grammar is out of scope, but long
/// comments and long strings are covered at every `--[=*[`/`[=*[` level so a
/// keyword inside one can never be mistaken for code.
pub fn lex_lua(s: &str) -> Vec<(usize, usize, Tok)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let kind = if b[i] == b'-' && b.get(i + 1) == Some(&b'-') {
            if let Some(lv) = long_bracket(b, i + 2) {
                // long comment --[[ ... ]] / --[==[ ... ]==]
                i += lv + 4;
                while i < b.len() && !bracket_close(b, i, lv) {
                    i += 1;
                }
                i = (i + lv + 2).min(b.len());
            } else {
                while i < b.len() && b[i] != b'\n' {
                    i += 1;
                }
            }
            Tok::Comment
        } else if b[i] == b'"' || b[i] == b'\'' {
            let q = b[i];
            i += 1;
            while i < b.len() && b[i] != q && b[i] != b'\n' {
                i += if b[i] == b'\\' { 2 } else { 1 };
            }
            i = (i + 1).min(b.len());
            Tok::Str
        } else if let Some(lv) = long_bracket(b, i) {
            // long string [[ ... ]] / [=[ ... ]=]
            i += lv + 2;
            while i < b.len() && !bracket_close(b, i, lv) {
                i += 1;
            }
            i = (i + lv + 2).min(b.len());
            Tok::Str
        } else if b[i].is_ascii_digit() || (b[i] == b'.' && b.get(i + 1).is_some_and(|c| c.is_ascii_digit())) {
            while i < b.len() && (b[i].is_ascii_alphanumeric() || b[i] == b'.' || b[i] == b'_') {
                i += 1;
            }
            Tok::Num
        } else if b[i].is_ascii_alphabetic() || b[i] == b'_' {
            while i < b.len() && is_ident(b[i]) {
                i += 1;
            }
            if KEYWORDS.contains(&&s[start..i]) { Tok::Key } else { Tok::Plain }
        } else {
            i += 1;
            Tok::Plain
        };
        if let Some((_, e, k)) = out.last_mut()
            && *k == kind
            && *e == start
        {
            *e = i;
        } else {
            out.push((start, i, kind));
        }
    }
    out
}

#[cfg(test)]
pub(crate) fn spy() -> (Handle, Receiver<LuaMsg>) {
    let (tx, rx) = channel();
    (Handle { tx: Some(tx), shared: Arc::new(Mutex::new(Shared::default())) }, rx)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[derive(Clone, Debug, PartialEq)]
    enum Ev {
        Key(u16, bool),
        Mouse(u8, bool),
        Move(i32, i32),
        Wheel(i32),
        HWheel(i32),
        Text(String),
        Launch(String),
    }

    #[derive(Default)]
    struct Rec(Mutex<Vec<Ev>>);

    impl Rec {
        fn events(&self) -> Vec<Ev> {
            self.0.lock().unwrap().clone()
        }
    }

    impl engine::Input for Rec {
        fn key(&self, vk: u16, down: bool) {
            self.0.lock().unwrap().push(Ev::Key(vk, down));
        }
        fn mouse(&self, btn: u8, down: bool) {
            self.0.lock().unwrap().push(Ev::Mouse(btn, down));
        }
        fn mouse_move(&self, dx: i32, dy: i32) {
            self.0.lock().unwrap().push(Ev::Move(dx, dy));
        }
        fn wheel(&self, delta: i32) {
            self.0.lock().unwrap().push(Ev::Wheel(delta));
        }
        fn hwheel(&self, delta: i32) {
            self.0.lock().unwrap().push(Ev::HWheel(delta));
        }
        fn text(&self, s: &str) {
            self.0.lock().unwrap().push(Ev::Text(s.into()));
        }
        fn launch(&self, path: &str) {
            self.0.lock().unwrap().push(Ev::Launch(path.into()));
        }
    }

    fn spawn(rec: &Arc<Rec>) -> Handle {
        let input: Arc<dyn engine::Input> = rec.clone();
        Handle::spawn_with(input, Arc::new(|_| false), None)
    }

    fn lines(h: &Handle) -> Vec<String> {
        h.console().1
    }

    fn wait_for(mut f: impl FnMut() -> bool) -> bool {
        let end = Instant::now() + Duration::from_secs(10);
        while Instant::now() < end {
            if f() {
                return true;
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        false
    }

    #[test]
    fn g_pressed_runs_the_script() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        h.send(LuaMsg::Script(
            r#"function OnEvent(event, arg)
                if event == "G_PRESSED" and arg == 4 then PressAndReleaseKey("spacebar") end
            end"#
                .into(),
        ));
        h.send(LuaMsg::Event("G_PRESSED", 4));
        h.send(LuaMsg::Event("G_PRESSED", 5));
        assert!(wait_for(|| rec.events() == vec![Ev::Key(0x20, true), Ev::Key(0x20, false)]), "{:?}", rec.events());
    }

    #[test]
    fn console_gets_logs_and_errors_without_dying() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        h.send(LuaMsg::Script(
            r#"function OnEvent(event, arg)
                if event == "PING" then OutputLogMessage("hello %s", "world")
                elseif event == "BAD" then error("boom")
                elseif event == "X" then OutputLogMessage("alive") end
            end"#
                .into(),
        ));
        h.send(LuaMsg::Event("PING", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l.contains("hello world"))), "{:?}", lines(&h));
        h.send(LuaMsg::Event("BAD", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l.contains("boom"))), "{:?}", lines(&h));
        h.send(LuaMsg::Event("X", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l.contains("alive"))), "{:?}", lines(&h));
    }

    #[test]
    fn sleep_keeps_queued_events_in_order() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        h.send(LuaMsg::Script(
            r#"function OnEvent(event, arg)
                if event == "SLEEP" then Sleep(50) OutputLogMessage("slept")
                elseif event == "PING" then OutputLogMessage("second") end
            end"#
                .into(),
        ));
        h.send(LuaMsg::Event("SLEEP", 0));
        h.send(LuaMsg::Event("PING", 0));
        assert!(wait_for(|| {
            let l = lines(&h);
            let (a, b) = (l.iter().position(|s| s == "slept"), l.iter().position(|s| s == "second"));
            a.is_some() && a < b
        }));
    }

    #[test]
    fn instruction_budget_kills_an_infinite_loop() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        h.send(LuaMsg::Script(
            r#"function OnEvent(event, arg)
                if event == "HANG" then while true do end else OutputLogMessage("next") end
            end"#
                .into(),
        ));
        h.send(LuaMsg::Event("HANG", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l.contains("script too slow"))), "{:?}", lines(&h));
        h.send(LuaMsg::Event("X", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l.contains("next"))), "{:?}", lines(&h));
    }

    #[test]
    fn primary_events_need_enabling() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        h.send(LuaMsg::Script(r#"function OnEvent(e, a) if e == "MOUSE_BUTTON_PRESSED" then OutputLogMessage("mb") end end"#.into()));
        h.send(LuaMsg::Event("MOUSE_BUTTON_PRESSED", 1));
        std::thread::sleep(Duration::from_millis(200));
        assert!(lines(&h).is_empty(), "{:?}", lines(&h));
        h.send(LuaMsg::Script(
            r#"function OnEvent(e, a)
                if e == "PROFILE_ACTIVATED" then EnablePrimaryMouseButtonEvents(true) OutputLogMessage("on")
                elseif e == "MOUSE_BUTTON_PRESSED" then OutputLogMessage("mb " .. a) end
            end"#
                .into(),
        ));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "on")), "{:?}", lines(&h));
        h.send(LuaMsg::Event("MOUSE_BUTTON_PRESSED", 1));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "mb 1")), "{:?}", lines(&h));
    }

    #[test]
    fn key_queries_use_the_injected_stub() {
        let rec = Arc::new(Rec::default());
        let input: Arc<dyn engine::Input> = rec.clone();
        let h = Handle::spawn_with(input, Arc::new(|vk| vk == 0x41 || vk == 0x01), None);
        h.send(LuaMsg::Script(
            r#"function OnEvent(e, a)
                if IsKeyPressed("a") then OutputLogMessage("a down") end
                if IsMouseButtonPressed(1) then OutputLogMessage("m1 down") end
                if IsKeyPressed("b") then OutputLogMessage("b down") end
            end"#
                .into(),
        ));
        h.send(LuaMsg::Event("X", 0));
        assert!(wait_for(|| lines(&h).len() >= 2), "{:?}", lines(&h));
        let l = lines(&h);
        assert!(l.iter().any(|s| s == "a down") && l.iter().any(|s| s == "m1 down") && !l.iter().any(|s| s == "b down"), "{l:?}");
    }

    #[test]
    fn lifecycle_marks_each_instance() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        let script = |tag: &str| {
            format!(
                r#"function OnEvent(e, a)
                    if e == "PROFILE_ACTIVATED" then OutputLogMessage("on {tag}")
                    elseif e == "PROFILE_DEACTIVATED" then OutputLogMessage("off {tag}") end
                end"#
            )
        };
        h.send(LuaMsg::Script(script("v1")));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "on v1")), "{:?}", lines(&h));
        h.send(LuaMsg::Script(script("v2")));
        assert!(wait_for(|| {
            let l = lines(&h);
            let (a, b) = (l.iter().position(|s| s == "off v1"), l.iter().position(|s| s == "on v2"));
            a.is_some() && a < b
        }));
        h.send(LuaMsg::Stop);
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "off v2")), "{:?}", lines(&h));
    }

    #[test]
    fn play_macro_loops_back_a_tap() {
        let rec = Arc::new(Rec::default());
        let input: Arc<dyn engine::Input> = rec.clone();
        let (tx, rx) = crossbeam_channel::unbounded();
        let h = Handle::spawn_with(input, Arc::new(|_| false), Some(tx));
        h.send(LuaMsg::Script(r#"function OnEvent(e, a) if e == "G_PRESSED" then PlayMacro("jump") end end"#.into()));
        h.send(LuaMsg::Event("G_PRESSED", 4));
        assert!(wait_for(|| rx.try_iter().any(|m| matches!(m, engine::EngineMsg::Tap(Assignment::Macro(n)) if n == "jump"))));
    }

    #[test]
    fn load_errors_land_on_the_console() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        h.send(LuaMsg::Script("this is not lua".into()));
        assert!(wait_for(|| !lines(&h).is_empty()));
        h.send(LuaMsg::Script(r#"function OnEvent(e, a) OutputLogMessage("after") end"#.into()));
        h.send(LuaMsg::Event("X", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "after")), "{:?}", lines(&h));
    }

    #[test]
    fn fixed_handle_does_nothing() {
        let h = Handle::fixed();
        h.send(LuaMsg::Script("function OnEvent() end".into()));
        h.send(LuaMsg::Event("G_PRESSED", 4));
        h.send(LuaMsg::Stop);
        assert!(h.console().1.is_empty());
    }

    #[test]
    fn key_names_map_to_vks() {
        for (n, vk) in [
            ("a", 0x41u16),
            ("Spacebar", 0x20),
            ("lshift", 0xA0),
            ("f5", 0x74),
            ("num3", 0x63),
            ("g1", 0x01),
            ("g5", 0x06),
            ("escape", 0x1B),
        ] {
            assert_eq!(vk_of(n), Some(vk), "{n}");
        }
        assert_eq!(vk_of("notakey"), None);
        assert_eq!(ghub_button(1), Some(0));
        assert_eq!(ghub_button(2), Some(2));
        assert_eq!(ghub_button(3), Some(1));
        assert_eq!(ghub_mouse_vk(5), Some(0x06));
    }

    #[test]
    fn mkeys_are_a_shared_modifier_bank() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        h.send(LuaMsg::Script(
            r#"function OnEvent(e, a)
                if e == "SET" then SetMKeyState(2, true)
                elseif e == "GET" then
                    local t = GetMKeyState()
                    OutputLogMessage("m %s %s %s one:%s", t[1], t[2], t[3], GetMKeyState(2))
                end
            end"#
                .into(),
        ));
        h.send(LuaMsg::Event("SET", 0));
        h.send(LuaMsg::Event("GET", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "m false true false one:true")), "{:?}", lines(&h));
        // the bank lives on the worker, so a script reload keeps it
        h.send(LuaMsg::Script(r#"function OnEvent(e, a) if e == "GET" then OutputLogMessage("still %s", GetMKeyState()[2]) end end"#.into()));
        h.send(LuaMsg::Event("GET", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "still true")), "{:?}", lines(&h));
    }

    #[test]
    fn abort_macro_goes_over_the_loopback() {
        let rec = Arc::new(Rec::default());
        let input: Arc<dyn engine::Input> = rec.clone();
        let (tx, rx) = crossbeam_channel::unbounded();
        let h = Handle::spawn_with(input, Arc::new(|_| false), Some(tx));
        h.send(LuaMsg::Script(r#"function OnEvent(e, a) if e == "STOP" then AbortMacro() end end"#.into()));
        h.send(LuaMsg::Event("STOP", 0));
        assert!(wait_for(|| rx.try_iter().any(|m| matches!(m, engine::EngineMsg::AbortMacros))));
        // without a loopback it degrades to a once-logged unsupported
        let h2 = spawn(&rec);
        h2.send(LuaMsg::Script(r#"function OnEvent(e, a) AbortMacro() end"#.into()));
        h2.send(LuaMsg::Event("X", 0));
        assert!(wait_for(|| lines(&h2).iter().any(|l| l.contains("AbortMacro is not supported"))), "{:?}", lines(&h2));
    }

    // ---------------------------------------------------------------- lexer

    fn kinds(s: &str) -> Vec<(Tok, &str)> {
        lex_lua(s).into_iter().map(|(a, b, k)| (k, &s[a..b])).collect()
    }

    #[test]
    fn comments_strings_keywords_and_numbers_class() {
        let got = kinds("-- note\nfunction f(x) return \"a\\'b\" .. 's' .. 1.5 .. 0xFF .. not_a end");
        assert!(got.contains(&(Tok::Comment, "-- note")), "{got:?}");
        assert!(got.contains(&(Tok::Key, "function")), "{got:?}");
        assert!(got.contains(&(Tok::Key, "return")), "{got:?}");
        assert!(got.contains(&(Tok::Str, "\"a\\'b\"")), "{got:?}");
        assert!(got.contains(&(Tok::Str, "'s'")), "{got:?}");
        assert!(got.contains(&(Tok::Num, "1.5")), "{got:?}");
        assert!(got.contains(&(Tok::Num, "0xFF")), "{got:?}");
        assert!(got.iter().any(|(k, s)| *k == Tok::Plain && s.contains("not_a")), "{got:?}");
        assert!(got.contains(&(Tok::Key, "end")), "{got:?}");
    }

    #[test]
    fn long_comment_covers_block() {
        let got = kinds("a = 1 --[[ skipped \"x\" 999 ]] b = 2");
        assert!(got.contains(&(Tok::Comment, "--[[ skipped \"x\" 999 ]]")), "{got:?}");
        // unterminated still colours as a comment
        let got = kinds("x --[[ open");
        assert_eq!(got.last(), Some(&(Tok::Comment, "--[[ open")));
        // unterminated string too
        let got = kinds("y = \"no end");
        assert_eq!(got.last(), Some(&(Tok::Str, "\"no end")));
    }

    #[test]
    fn leveled_long_brackets_class() {
        // --[==[ ... ]==] is one comment at any level
        let got = kinds("--[==[ managed\nfunction end\n]==]\nend");
        assert!(got.contains(&(Tok::Comment, "--[==[ managed\nfunction end\n]==]")), "{got:?}");
        assert_eq!(got.last(), Some(&(Tok::Key, "end")), "{got:?}");
        // a level-0 ]] does not close a level-2 block
        let got = kinds("--[==[ a ]] b ]==] x");
        assert!(matches!(got.first(), Some((Tok::Comment, _))), "{got:?}");
        assert_eq!(got.last(), Some(&(Tok::Plain, " x")), "{got:?}");
        // long strings keep keywords out of the code stream
        let got = kinds("s = [[function end]] .. [=[if end]=]");
        assert!(got.contains(&(Tok::Str, "[[function end]]")), "{got:?}");
        assert!(got.contains(&(Tok::Str, "[=[if end]=]")), "{got:?}");
        assert!(!got.iter().any(|(k, _)| *k == Tok::Key), "{got:?}");
        // but a plain [ stays plain
        let got = kinds("t[1] = 2");
        assert!(!got.iter().any(|(k, _)| *k == Tok::Str), "{got:?}");
    }

    #[test]
    fn generated_p180_script_moves_the_baked_counts() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        let script = crate::profile::p180_lua_inject("", &crate::profile::p180_lua_snippet(5, 2348, 12, 60));
        h.send(LuaMsg::Script(script));
        h.send(LuaMsg::Event("G_PRESSED", 5));
        assert!(wait_for(|| rec.events().len() == 12), "{:?}", rec.events());
        let evs = rec.events();
        let total: i32 = evs
            .iter()
            .map(|e| match e {
                Ev::Move(dx, dy) => {
                    assert_eq!(*dy, 0);
                    *dx
                }
                other => panic!("unexpected event {other:?}"),
            })
            .sum();
        assert_eq!(total, 2348, "{evs:?}");
        // a different G doesn't fire
        h.send(LuaMsg::Event("G_PRESSED", 4));
        std::thread::sleep(Duration::from_millis(150));
        assert_eq!(rec.events().len(), 12, "{:?}", rec.events());
    }

    #[test]
    fn injected_p180_block_keeps_the_users_handler() {
        let rec = Arc::new(Rec::default());
        let h = spawn(&rec);
        let script = crate::profile::p180_lua_inject(
            r#"function OnEvent(e, a)
                if e == "G_PRESSED" then OutputLogMessage("mine " .. a) end
            end"#,
            &crate::profile::p180_lua_snippet(4, 40, 4, 4),
        );
        h.send(LuaMsg::Script(script));
        h.send(LuaMsg::Event("G_PRESSED", 4));
        assert!(wait_for(|| rec.events().len() == 4 && lines(&h).iter().any(|l| l == "mine 4")), "{:?} {:?}", rec.events(), lines(&h));
    }

    #[test]
    fn output_debug_message_is_prefixed_and_modifier_query_works() {
        let rec = Arc::new(Rec::default());
        let input: Arc<dyn engine::Input> = rec.clone();
        // lshift (0xA0) counts as held
        let h = Handle::spawn_with(input, Arc::new(|vk| vk == 0xA0), None);
        h.send(LuaMsg::Script(
            r#"function OnEvent(e, a)
                if e == "D" then OutputDebugMessage("dbg %d", 7)
                elseif e == "M" then OutputLogMessage("shift:%s alt:%s", IsModifierPressed("lshift"), IsModifierPressed("lalt"))
                end
            end"#
                .into(),
        ));
        h.send(LuaMsg::Event("D", 0));
        h.send(LuaMsg::Event("M", 0));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "DEBUG: dbg 7")), "{:?}", lines(&h));
        assert!(wait_for(|| lines(&h).iter().any(|l| l == "shift:true alt:false")), "{:?}", lines(&h));
    }
}
