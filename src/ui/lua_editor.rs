//! G HUB's full-window script editor for a profile's Lua `OnEvent` script,
//! opened from the SCRIPTING block on an app detail page. The code field is
//! monospace with a hand-drawn line-number gutter and Lua syntax colouring
//! (`highlight_lua`); IMPORT/EXPORT use the native dialogs (`.lua`) and
//! SAVE & RUN writes `Profile.script` — the engine reloads it on the worker
//! when that profile is active. Leaving with unsaved edits asks first
//! (SAVE / DISCARD / KEEP EDITING). The CONSOLE pane shows the worker's
//! `OutputLogMessage` lines and runtime errors.

use super::theme::*;
use super::GhubApp;
use crossbeam_channel::Receiver;
use egui::{Align2, CornerRadius, FontId, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

/// What `+ CREATE A NEW LUA SCRIPT` seeds (the `scriptedit` shot page shows it too).
pub const TEMPLATE: &str = "-- This is the script GHUB-X runs while this profile is active.\nfunction OnEvent(event, arg)\n    -- if event == \"G_PRESSED\" and arg == 4 then\n    --     PressAndReleaseKey(\"spacebar\")\n    -- end\nend\n";

/// Result of an import/export dialog running on a helper thread.
enum Io {
    Imported(String),
    Message(String),
}

/// State of the script editor overlay (`GhubApp::script_editor`).
pub struct ScriptEditor {
    /// (app, profile) whose `Profile.script` is being edited.
    pub target: (usize, usize),
    pub text: String,
    /// The text last written to `Profile.script` (dirty check for the unsaved bar).
    pub saved: String,
    /// The "Unsaved changes — SAVE / DISCARD / KEEP EDITING" bar is showing.
    pub unsaved_bar: bool,
    io: Option<Receiver<Io>>,
    /// Console revision already drawn + its lines.
    console_rev: u64,
    lines: Vec<String>,
    /// The code field owns the keyboard (Escape must not close the editor).
    code_focused: bool,
}

impl ScriptEditor {
    pub fn new(target: (usize, usize), text: String) -> Self {
        Self { target, saved: text.clone(), text, unsaved_bar: false, io: None, console_rev: u64::MAX, lines: Vec::new(), code_focused: false }
    }
}

/// A plain text link that underlines on hover; returns true when clicked.
fn link(ui: &Ui, pos: egui::Pos2, label: &str, id: &str) -> bool {
    let g = galley(ui, label, Ts::new(Fam::Reg, 11.0, pal().muted), f32::INFINITY);
    let r = Rect::from_min_size(pos, g.size());
    let c = click(ui, r, id);
    let color = if c.hovered() { pal().primary } else { pal().muted };
    ui.painter().galley(r.min, galley(ui, label, Ts::new(Fam::Reg, 11.0, color), f32::INFINITY), color);
    if c.hovered() {
        ui.painter().line_segment([r.left_bottom(), r.right_bottom()], Stroke::new(1.0, color));
    }
    c.clicked()
}

impl GhubApp {
    /// The overlay that replaces the whole page while a script is being edited.
    pub(crate) fn script_overlay(&mut self, ui: &mut Ui, win: Rect) {
        let Some(mut ed) = self.script_editor.take() else { return };
        if self.script_page(ui, win, &mut ed) {
            self.script_editor = Some(ed);
        }
    }

    /// Returns `false` to close the overlay.
    fn script_page(&mut self, ui: &mut Ui, win: Rect, ed: &mut ScriptEditor) -> bool {
        let th = pal();
        // dialogs running on a helper thread
        if let Some(rx) = &ed.io {
            match rx.try_recv() {
                Ok(Io::Imported(t)) => {
                    ed.text = t;
                    ed.io = None;
                }
                Ok(Io::Message(m)) => {
                    ed.io = None;
                    if !m.is_empty() {
                        self.say(m);
                    }
                }
                Err(crossbeam_channel::TryRecvError::Empty) => {}
                Err(crossbeam_channel::TryRecvError::Disconnected) => ed.io = None,
            }
        }

        let bc = pos2(win.left() + 39.0, win.top() + 58.0);
        let back = click(ui, Rect::from_center_size(bc, vec2(44.0, 44.0)), "script-back");
        arrow_back(ui.painter(), bc, Stroke::new(2.0, if back.hovered() { th.primary } else { th.text }));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ed.code_focused;
        let mut open = !(back.clicked() || escape);
        // leaving with edits the profile doesn't have asks first
        if (back.clicked() || escape) && ed.unsaved_bar {
            ed.unsaved_bar = false; // KEEP EDITING
            open = true;
        } else if (back.clicked() || escape) && ed.text != ed.saved {
            ed.unsaved_bar = true;
            open = true;
        }

        let cfg = self.cfg();
        let app = cfg.apps.get(ed.target.0);
        let Some(p) = app.and_then(|a| a.profiles.get(ed.target.1)) else { return false };
        text(ui, pos2(win.left() + 64.0, win.top() + 133.0), Align2::LEFT_CENTER, "Script", Ts::new(Fam::Bold, 24.0, th.text).ls(0.0));
        text(ui, pos2(win.left() + 64.0, win.top() + 157.0), Align2::LEFT_CENTER, &format!("{}: {}", app.map_or(String::new(), |a| a.name.clone()), p.name), Ts::new(Fam::Reg, 12.0, th.muted).ls(0.4));

        // ---- top-right buttons
        let by = win.top() + 46.0;
        let save = blue_button(ui, Rect::from_min_size(pos2(win.right() - 64.0 - 96.0, by), vec2(96.0, 28.0)), "SAVE & RUN", "script-save");
        let er = Rect::from_min_size(pos2(save.rect.left() - 8.0 - 64.0, by), vec2(64.0, 28.0));
        let export = dark_button(ui, er, "EXPORT", "script-export", ed.io.is_none());
        let ir = Rect::from_min_size(pos2(er.left() - 8.0 - 64.0, by), vec2(64.0, 28.0));
        let import = dark_button(ui, ir, "IMPORT", "script-import", ed.io.is_none());
        if export.clicked() && ed.io.is_none() {
            let owner = crate::win::main_window_handle();
            let (tx, rx) = crossbeam_channel::bounded(1);
            let text = ed.text.clone();
            std::thread::spawn(move || {
                let msg = match crate::win::save_file(owner, "Export the Lua script", "script.lua", "Lua script", "*.lua") {
                    Some(path) => match std::fs::write(&path, text) {
                        Ok(()) => format!("Exported {}", path.rsplit(['\\', '/']).next().unwrap_or("script.lua")),
                        Err(e) => format!("Couldn't write the file: {e}"),
                    },
                    None => String::new(),
                };
                let _ = tx.send(Io::Message(msg));
            });
            ed.io = Some(rx);
        }
        if import.clicked() && ed.io.is_none() {
            let owner = crate::win::main_window_handle();
            let (tx, rx) = crossbeam_channel::bounded(1);
            std::thread::spawn(move || {
                let msg = match crate::win::pick_file(owner, "Import a Lua script", "Lua script", "*.lua") {
                    Some(path) => match std::fs::read_to_string(&path) {
                        Ok(t) => Io::Imported(t),
                        Err(e) => Io::Message(format!("Couldn't read the file: {e}")),
                    },
                    None => Io::Message(String::new()),
                };
                let _ = tx.send(msg);
            });
            ed.io = Some(rx);
        }
        if save.clicked() {
            let (ai, pi) = ed.target;
            let text = ed.text.clone();
            self.edit(move |c| {
                if let Some(p) = c.apps.get_mut(ai).and_then(|a| a.profiles.get_mut(pi)) {
                    p.script = text;
                }
            });
            ed.saved = ed.text.clone();
            self.say("Script saved");
        }

        // ---- code field with a line-number gutter
        let code = Rect::from_min_max(pos2(win.left() + 64.0, win.top() + 186.0), pos2(win.right() - 64.0, win.bottom() - 224.0));
        ui.painter().rect_filled(code, CornerRadius::same(4), th.n(0x14));
        ui.painter().rect_stroke(code, CornerRadius::same(4), Stroke::new(1.0, th.ink(51)), StrokeKind::Inside);
        let fid = FontId::monospace(13.0);
        let rh = ui.ctx().fonts_mut(|f| f.row_height(&fid));
        let inner = code.shrink2(vec2(6.0, 6.0));
        let mut child = ui.new_child(UiBuilder::new().max_rect(inner));
        egui::ScrollArea::vertical().id_salt("lua-code").auto_shrink([false, false]).show(&mut child, |ui| {
            ui.horizontal_top(|ui| {
                let n = ed.text.lines().count().max(1);
                let (gut, _) = ui.allocate_exact_size(vec2(34.0, n as f32 * rh + rh), Sense::hover());
                for i in 0..n {
                    ui.painter().text(pos2(gut.right() - 4.0, gut.top() + 4.0 + i as f32 * rh), Align2::RIGHT_TOP, (i + 1).to_string(), fid.clone(), th.muted);
                }
                let mut layouter = |ui: &Ui, t: &dyn egui::TextBuffer, wrap: f32| {
                    let mut job = highlight_lua(t.as_str(), &pal());
                    job.wrap.max_width = wrap;
                    ui.painter().layout_job(job)
                };
                let resp = ui.add(egui::TextEdit::multiline(&mut ed.text).font(fid.clone()).desired_width(f32::INFINITY).lock_focus(true).frame(egui::Frame::NONE).text_color(th.text).layouter(&mut layouter));
                ed.code_focused = resp.has_focus();
            });
        });

        // ---- console pane fed by the worker's output buffer
        let (rev, lines) = self.lua.console();
        if rev != ed.console_rev {
            ed.console_rev = rev;
            ed.lines = lines;
        }
        let cy = code.bottom() + 20.0;
        text(ui, pos2(code.left(), cy), Align2::LEFT_CENTER, "CONSOLE", t_small_label());
        if link(ui, pos2(code.right() - 70.0, cy - 6.0), "CLEAR OUTPUT", "script-clear-log") {
            self.lua.clear();
        }
        let pane = Rect::from_min_max(pos2(code.left(), cy + 12.0), pos2(code.right(), win.bottom() - 52.0));
        ui.painter().rect_filled(pane, CornerRadius::same(4), th.n(0x14));
        ui.painter().rect_stroke(pane, CornerRadius::same(4), Stroke::new(1.0, th.ink(51)), StrokeKind::Inside);
        let cid = FontId::monospace(10.5);
        let crh = ui.ctx().fonts_mut(|f| f.row_height(&cid));
        let fit = ((pane.height() - 10.0) / crh).floor().max(1.0) as usize;
        for (i, line) in ed.lines.iter().skip(ed.lines.len().saturating_sub(fit)).enumerate() {
            ui.painter().text(pos2(pane.left() + 8.0, pane.top() + 5.0 + i as f32 * crh), Align2::LEFT_TOP, line, cid.clone(), th.muted);
        }

        // ---- links
        let ly = win.bottom() - 30.0;
        if link(ui, pos2(code.left(), ly - 5.0), "SCRIPTING API", "script-api-link") {
            self.say("The scripting API is documented in AGENTS.md");
        }
        if link(ui, pos2(code.left() + 110.0, ly - 5.0), "LUA ONLINE REFERENCE", "script-ref-link") {
            crate::win::launch("https://www.lua.org/manual/5.4/");
        }

        // ---- unsaved-changes bar (same look as the on-board WRITE TO MOUSE bar)
        if ed.unsaved_bar {
            let bar = Rect::from_min_max(pos2(win.left(), win.bottom() - 56.0), win.right_bottom());
            ui.painter().rect_filled(bar, CornerRadius::ZERO, pal().primary);
            para(ui, pos2(bar.left() + 24.0, bar.top() + 10.0), bar.width() - 500.0, "Unsaved changes — the profile doesn't have this script yet.", Ts::new(Fam::Reg, 12.0, pal().on_primary));
            let kr = Rect::from_min_size(pos2(bar.right() - 158.0, bar.top() + 12.0), vec2(142.0, 32.0));
            let dr = Rect::from_min_size(pos2(kr.left() - 8.0 - 90.0, bar.top() + 12.0), vec2(90.0, 32.0));
            let sr = Rect::from_min_size(pos2(dr.left() - 8.0 - 66.0, bar.top() + 12.0), vec2(66.0, 32.0));
            if dark_button(ui, sr, "SAVE", "script-bar-save", true).clicked() {
                let (ai, pi) = ed.target;
                let text = ed.text.clone();
                self.edit(move |c| {
                    if let Some(p) = c.apps.get_mut(ai).and_then(|a| a.profiles.get_mut(pi)) {
                        p.script = text;
                    }
                });
                self.say("Script saved");
                open = false;
            } else if dark_button(ui, dr, "DISCARD", "script-bar-discard", true).clicked() {
                open = false;
            } else if dark_button(ui, kr, "KEEP EDITING", "script-bar-keep", true).clicked() {
                ed.unsaved_bar = false;
            }
        }
        open
    }
}

// ------------------------------------------------------------------ syntax colouring

/// What a byte range of a Lua script is coloured as.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Tok {
    /// Identifiers, operators, punctuation — plain text colour.
    Plain,
    /// `-- line` and `--[[ block ]]` comments.
    Comment,
    /// `".."` and `'..'` strings.
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

/// Lex `s` into (start byte, end byte, class) spans covering the whole text.
/// Kept deliberately small: Lua's real grammar (nested long brackets etc.)
/// is out of scope for colouring.
pub fn lex_lua(s: &str) -> Vec<(usize, usize, Tok)> {
    let b = s.as_bytes();
    let mut out = Vec::new();
    let mut i = 0;
    while i < b.len() {
        let start = i;
        let kind = if b[i] == b'-' && b.get(i + 1) == Some(&b'-') {
            if b.get(i + 2) == Some(&b'[') && b.get(i + 3) == Some(&b'[') {
                // long comment --[[ ... ]]
                i += 4;
                while i < b.len() && !(b[i] == b']' && b.get(i + 1) == Some(&b']')) {
                    i += 1;
                }
                i = (i + 2).min(b.len());
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

/// The `egui::TextEdit::layouter` job for a Lua script (egui demo pattern):
/// comments muted, strings amber, keywords primary, numbers bright.
pub fn highlight_lua(s: &str, pal: &Palette) -> egui::text::LayoutJob {
    let fid = FontId::monospace(13.0);
    let mut job = egui::text::LayoutJob::default();
    for (lo, hi, kind) in lex_lua(s) {
        let color = match kind {
            Tok::Comment => pal.muted,
            Tok::Str => pal.amber,
            Tok::Key => pal.primary,
            Tok::Num => pal.text_high,
            Tok::Plain => pal.text,
        };
        job.append(&s[lo..hi], 0.0, egui::TextFormat { font_id: fid.clone(), color, ..Default::default() });
    }
    job
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn highlight_job_has_the_right_sections() {
        let pal = pal();
        let src = "-- c\nlocal x = 42";
        let job = highlight_lua(src, &pal);
        let spans = lex_lua(src);
        assert_eq!(job.sections.len(), spans.len(), "one section per span");
        for (sec, (_, _, kind)) in job.sections.iter().zip(&spans) {
            let want = match kind {
                Tok::Comment => pal.muted,
                Tok::Str => pal.amber,
                Tok::Key => pal.primary,
                Tok::Num => pal.text_high,
                Tok::Plain => pal.text,
            };
            assert_eq!(sec.format.color, want, "{kind:?}");
        }
        // the keyword section in "local x" is primary-coloured
        let kw = job.sections.iter().zip(&spans).find(|(_, (a, b, _))| &src[*a..*b] == "local").unwrap().0;
        assert_eq!(kw.format.color, pal.primary);
    }
}
