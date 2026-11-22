//! G HUB's full-window script editor for a profile's Lua `OnEvent` script,
//! opened from the SCRIPTING block on an app detail page. The code field is
//! monospace with a hand-drawn line-number gutter; IMPORT/EXPORT use the
//! native dialogs (`.lua`) and SAVE & RUN writes `Profile.script` — the
//! engine reloads it on the worker when that profile is active. The CONSOLE
//! pane shows the worker's `OutputLogMessage` lines and runtime errors.
//! No syntax colouring yet (G HUB highlights; plain text is the noted gap).

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
    io: Option<Receiver<Io>>,
    /// Console revision already drawn + its lines.
    console_rev: u64,
    lines: Vec<String>,
    /// The code field owns the keyboard (Escape must not close the editor).
    code_focused: bool,
}

impl ScriptEditor {
    pub fn new(target: (usize, usize), text: String) -> Self {
        Self { target, text, io: None, console_rev: u64::MAX, lines: Vec::new(), code_focused: false }
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
        let open = !(back.clicked() || escape);

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
                let resp = ui.add(egui::TextEdit::multiline(&mut ed.text).font(fid.clone()).desired_width(f32::INFINITY).lock_focus(true).frame(egui::Frame::NONE).text_color(th.text));
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
        open
    }
}
