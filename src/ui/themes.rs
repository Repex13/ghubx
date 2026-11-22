//! Settings > Appearance: MY THEMES (user-made colour themes) and the theme editor.

use super::light::PickerState;
use super::palette::{Palette, contrast, custom_palette};
use super::theme::*;
use super::{GhubApp, Route};
use crate::profile::{Appearance, CustomTheme, export_theme, import_theme};
use crossbeam_channel::Receiver;
use egui::{Align2, Color32, CornerRadius, Rect, Stroke, StrokeKind, Ui, pos2, vec2};
use std::time::{Duration, Instant};

const ROLES: [&str; 4] = ["Background", "Panels", "Text", "Accent"];
const BASE_SWATCHES: [[u8; 3]; 9] = [
    [0x00, 0x00, 0x00],
    [0x0A, 0x0A, 0x0A],
    [0x10, 0x11, 0x13],
    [0x16, 0x19, 0x29],
    [0x21, 0x22, 0x25],
    [0x2A, 0x2B, 0x30],
    [0xDC, 0xDD, 0xE0],
    [0xF0, 0xF0, 0xF0],
    [0xFF, 0xFF, 0xFF],
];
const ACCENT_SWATCHES: [[u8; 3]; 9] = [
    [0x11, 0x96, 0xff],
    [0x44, 0xd6, 0x2c],
    [0x9e, 0x5d, 0xff],
    [0xff, 0x5e, 0x01],
    [0xff, 0x3b, 0x4e],
    [0xff, 0x4f, 0xa3],
    [0x00, 0xc8, 0xb4],
    [0xff, 0xc8, 0x00],
    [0xff, 0xff, 0xff],
];

pub struct ThemeEditor {
    pub draft: CustomTheme,
    /// Index into `appearance.custom` when editing an existing theme.
    pub existing: Option<usize>,
    pub role: usize,
    picker: PickerState,
    delete_armed: Option<Instant>,
}

impl ThemeEditor {
    pub fn new(draft: CustomTheme, existing: Option<usize>, role: usize) -> Self {
        Self { draft, existing, role, picker: PickerState::default(), delete_armed: None }
    }

    fn color(&self) -> [u8; 3] {
        match self.role {
            0 => self.draft.bg,
            1 => self.draft.surface,
            2 => self.draft.text,
            _ => self.draft.accent,
        }
    }

    fn set_color(&mut self, c: [u8; 3]) {
        match self.role {
            0 => self.draft.bg = c,
            1 => self.draft.surface = c,
            2 => self.draft.text = c,
            _ => self.draft.accent = c,
        }
    }
}

pub enum ThemeIo {
    Message(String),
    Text(String),
}

pub type ThemeIoRx = Receiver<ThemeIo>;

fn hex_of(c: [u8; 3]) -> String {
    format!("#{:02X}{:02X}{:02X}", c[0], c[1], c[2])
}

fn rgb(c: [u8; 3]) -> Color32 {
    Color32::from_rgb(c[0], c[1], c[2])
}

fn elide(ui: &Ui, s: &str, ts: Ts, max: f32) -> String {
    if galley(ui, s, ts, f32::INFINITY).size().x <= max {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while t.pop().is_some() {
        let cand = format!("{}\u{2026}", t.trim_end());
        if galley(ui, &cand, ts, f32::INFINITY).size().x <= max {
            return cand;
        }
    }
    "\u{2026}".to_string()
}

/// Mini preview of a theme (shared by the preset and custom tiles).
pub(super) fn theme_tile(ui: &Ui, tile: Rect, pv: &Palette, selected: bool) {
    let th = pal();
    let p = ui.painter();
    p.rect_filled(tile, CornerRadius::same(8), pv.bg);
    p.rect_filled(Rect::from_min_size(tile.min + vec2(10.0, 10.0), vec2(34.0, 52.0)), CornerRadius::same(4), pv.surface);
    p.rect_filled(Rect::from_min_size(tile.min + vec2(54.0, 14.0), vec2(36.0, 6.0)), CornerRadius::same(3), pv.text_high);
    p.rect_filled(Rect::from_min_size(tile.min + vec2(54.0, 28.0), vec2(24.0, 6.0)), CornerRadius::same(3), pv.label);
    p.rect_filled(Rect::from_min_size(tile.right_bottom() - vec2(40.0, 20.0), vec2(30.0, 10.0)), CornerRadius::same(5), pv.primary);
    let stroke = if selected { Stroke::new(2.0, th.primary) } else { Stroke::new(1.0, th.outline_hi) };
    p.rect_stroke(tile, CornerRadius::same(8), stroke, StrokeKind::Inside);
}

fn new_draft() -> CustomTheme {
    let p = pal();
    let c = |c: Color32| [c.r(), c.g(), c.b()];
    CustomTheme { id: String::new(), name: "My Theme".into(), bg: c(p.bg), surface: c(p.surface), text: c(p.text), accent: c(p.primary) }
}

impl GhubApp {
    /// MY THEMES: label row with IMPORT / EXPORT / EDIT, then one page of tiles.
    pub(crate) fn my_themes_section(&mut self, ui: &mut Ui, main: Rect, y: f32, ap: &Appearance) {
        let th = pal();
        self.poll_theme_io(ui);
        let label = text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "MY THEMES", t_section());
        let n = ap.custom.len();
        let slots = n + usize::from(n < Appearance::MAX_CUSTOM);
        let pages = slots.div_ceil(5).max(1);
        self.theme_page = self.theme_page.min(pages - 1);
        if pages > 1 {
            let (cx, cy) = (label.right() + 24.0, y);
            for (dir, dx) in [(false, 0.0), (true, 22.0)] {
                let r = click(ui, Rect::from_center_size(pos2(cx + dx, cy), vec2(20.0, 20.0)), ("theme-page", dir));
                let enabled = if dir { self.theme_page + 1 < pages } else { self.theme_page > 0 };
                let c = if !enabled { th.disabled } else if r.hovered() { th.text } else { th.label };
                ui.painter().add(egui::Shape::line(
                    if dir { vec![pos2(cx + dx - 2.5, cy - 5.0), pos2(cx + dx + 2.5, cy), pos2(cx + dx - 2.5, cy + 5.0)] } else { vec![pos2(cx + dx + 2.5, cy - 5.0), pos2(cx + dx - 2.5, cy), pos2(cx + dx + 2.5, cy + 5.0)] },
                    Stroke::new(1.6, c),
                ));
                if r.clicked() && enabled {
                    self.theme_page = if dir { self.theme_page + 1 } else { self.theme_page - 1 };
                }
            }
        }
        let active_custom = ap.custom.iter().position(|c| c.id == ap.theme);
        let mut right = main.right();
        let buttons: &[(&str, f32, bool)] = &[("IMPORT", 76.0, true), ("EXPORT", 76.0, active_custom.is_some()), ("EDIT", 60.0, active_custom.is_some())];
        for (name, w, shown) in buttons {
            if !shown {
                continue;
            }
            let r = Rect::from_min_size(pos2(right - w, y - 12.0), vec2(*w, 24.0));
            if dark_button(ui, r, name, ("theme-btn", *name), true).clicked() {
                match *name {
                    "IMPORT" => self.start_theme_import(),
                    "EXPORT" => {
                        if let Some(i) = active_custom {
                            self.start_theme_export(&ap.custom[i]);
                        }
                    }
                    _ => {
                        if let Some(i) = active_custom {
                            self.theme_edit = Some(ThemeEditor::new(ap.custom[i].clone(), Some(i), 0));
                        }
                    }
                }
            }
            right = r.left() - 8.0;
        }
        let ty = y + 20.0;
        for slot in 0..5 {
            let idx = self.theme_page * 5 + slot;
            if idx >= slots {
                break;
            }
            let tile = Rect::from_min_size(pos2(main.left() + slot as f32 * 120.0, ty), vec2(106.0, 84.0));
            if let Some(t) = ap.custom.get(idx) {
                let r = click(ui, tile, ("my-theme", idx));
                let selected = ap.theme == t.id;
                theme_tile(ui, tile, &custom_palette(t), selected);
                let ts = Ts::new(Fam::Reg, 12.0, if selected { th.text } else { th.label });
                let name = elide(ui, &t.name, ts, 112.0);
                text(ui, pos2(tile.center().x, tile.bottom() + 14.0), Align2::CENTER_CENTER, &name, ts);
                if r.clicked() {
                    self.set_theme(&t.id);
                }
            } else {
                let r = click(ui, tile, "my-theme-new");
                if r.hovered() {
                    ui.painter().rect_filled(tile, CornerRadius::same(8), th.surface);
                }
                ui.painter().rect_stroke(tile, CornerRadius::same(8), Stroke::new(1.0, th.outline_hi), StrokeKind::Inside);
                let c = if r.hovered() { th.text } else { th.label };
                ui.painter().line_segment([tile.center() - vec2(10.0, 0.0), tile.center() + vec2(10.0, 0.0)], Stroke::new(2.0, c));
                ui.painter().line_segment([tile.center() - vec2(0.0, 10.0), tile.center() + vec2(0.0, 10.0)], Stroke::new(2.0, c));
                text(ui, pos2(tile.center().x, tile.bottom() + 14.0), Align2::CENTER_CENTER, "New theme", Ts::new(Fam::Reg, 12.0, th.label));
                if r.clicked() {
                    self.theme_edit = Some(ThemeEditor::new(new_draft(), None, 0));
                }
            }
        }
    }

    fn start_theme_import(&mut self) {
        if self.theme_io.is_some() {
            return;
        }
        let owner = crate::win::main_window_handle();
        let (tx, rx) = crossbeam_channel::bounded(1);
        std::thread::spawn(move || {
            let msg = match crate::win::pick_file(owner, "Import theme", "GHUB-X theme", "*.ghubxtheme") {
                Some(path) => match std::fs::read_to_string(&path) {
                    Ok(t) => ThemeIo::Text(t),
                    Err(e) => ThemeIo::Message(format!("Couldn't read the file: {e}")),
                },
                None => ThemeIo::Message(String::new()),
            };
            let _ = tx.send(msg);
        });
        self.theme_io = Some(rx);
    }

    fn start_theme_export(&mut self, t: &CustomTheme) {
        if self.theme_io.is_some() {
            return;
        }
        let owner = crate::win::main_window_handle();
        let (tx, rx) = crossbeam_channel::bounded(1);
        let (name, body) = (t.name.clone(), export_theme(t));
        std::thread::spawn(move || {
            let file = format!("{}.ghubxtheme", name.replace(['/', '\\', ':', '*', '?', '"', '<', '>', '|'], "_"));
            let msg = match crate::win::save_file(owner, "Export theme", &file, "GHUB-X theme", "*.ghubxtheme") {
                Some(path) => match std::fs::write(&path, body) {
                    Ok(()) => format!("Exported theme {name}"),
                    Err(e) => format!("Couldn't write the file: {e}"),
                },
                None => String::new(),
            };
            let _ = tx.send(ThemeIo::Message(msg));
        });
        self.theme_io = Some(rx);
    }

    fn poll_theme_io(&mut self, ui: &Ui) {
        let Some(rx) = &self.theme_io else { return };
        match rx.try_recv() {
            Ok(res) => {
                self.theme_io = None;
                match res {
                    ThemeIo::Message(m) if m.is_empty() => {}
                    ThemeIo::Message(m) => self.say(m),
                    ThemeIo::Text(t) => match import_theme(&t) {
                        Ok(theme) => {
                            let mut added: Option<String> = None;
                            self.edit(|c| {
                                if let Some(i) = c.appearance.add_theme(theme) {
                                    c.appearance.theme = c.appearance.custom[i].id.clone();
                                    c.appearance.accent = None;
                                    added = Some(c.appearance.custom[i].name.clone());
                                }
                            });
                            match added {
                                Some(n) => self.say(format!("Imported theme {n}")),
                                None => self.say(format!("You can have at most {} custom themes", Appearance::MAX_CUSTOM)),
                            }
                        }
                        Err(e) => self.say(e),
                    },
                }
            }
            Err(_) => ui.ctx().request_repaint_after(Duration::from_millis(100)),
        }
    }

    /// The editor view (replaces the Appearance page while a draft is open).
    pub(crate) fn theme_editor_page(&mut self, ui: &mut Ui, main: Rect) {
        let th = pal();
        let Some(ed) = &mut self.theme_edit else { return };
        let mut cancel = false;
        let back = click(ui, Rect::from_center_size(pos2(main.left() + 14.0, main.top() + 36.0), vec2(40.0, 40.0)), "theme-back");
        arrow_back(ui.painter(), back.rect.center(), Stroke::new(2.0, if back.hovered() { th.text } else { th.text_high }));
        cancel |= back.clicked();
        text(ui, pos2(main.left() + 44.0, main.top() + 36.0), Align2::LEFT_CENTER, if ed.existing.is_some() { "Edit Theme" } else { "New Theme" }, Ts::new(Fam::Bold, 22.0, th.text).ls(0.0));

        text(ui, pos2(main.left(), main.top() + 95.0), Align2::LEFT_CENTER, "NAME", t_section());
        let nb = Rect::from_min_size(pos2(main.left(), main.top() + 110.0), vec2(280.0, 32.0));
        ui.painter().rect_stroke(nb, CornerRadius::same(4), Stroke::new(1.0, th.outline_hi), StrokeKind::Inside);
        let te = egui::TextEdit::singleline(&mut ed.draft.name).frame(egui::Frame::NONE).char_limit(24).font(Ts::new(Fam::Reg, 14.0, th.text).font()).text_color(th.text);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(nb));
        child.put(Rect::from_min_max(nb.min + vec2(10.0, 6.0), nb.max - vec2(10.0, 4.0)), te);

        text(ui, pos2(main.left(), main.top() + 175.0), Align2::LEFT_CENTER, "COLORS", t_section());
        let top = main.top() + 195.0;
        for (i, role) in ROLES.iter().enumerate() {
            let row = Rect::from_min_size(pos2(main.left(), top + i as f32 * 44.0), vec2(270.0, 40.0));
            let r = click(ui, row, ("theme-role", i));
            if ed.role == i {
                ui.painter().rect_filled(row, CornerRadius::same(8), th.primary_tint);
            } else if r.hovered() {
                ui.painter().rect_filled(row, CornerRadius::same(8), th.surface);
            }
            let c = match i {
                0 => ed.draft.bg,
                1 => ed.draft.surface,
                2 => ed.draft.text,
                _ => ed.draft.accent,
            };
            let sw = Rect::from_center_size(pos2(row.left() + 24.0, row.center().y), vec2(28.0, 28.0));
            ui.painter().rect_filled(sw, CornerRadius::same(6), rgb(c));
            ui.painter().rect_stroke(sw, CornerRadius::same(6), Stroke::new(1.0, th.outline_hi), StrokeKind::Inside);
            text(ui, pos2(row.left() + 48.0, row.center().y), Align2::LEFT_CENTER, role, Ts::new(Fam::Bold, 14.0, if ed.role == i { th.primary } else { th.text }).ls(0.2));
            text(ui, pos2(row.right() - 12.0, row.center().y), Align2::RIGHT_CENTER, &hex_of(c), Ts::new(Fam::Reg, 12.0, th.label));
            if r.clicked() {
                ed.role = i;
            }
        }
        let swatches = if ed.role == 3 { &ACCENT_SWATCHES } else { &BASE_SWATCHES };
        let color = ed.color();
        if let Some(c) = GhubApp::color_picker(ui, pos2(main.right() - 278.0, top), color, &mut ed.picker, "theme", swatches) {
            ed.set_color(c);
        }

        let (cb, cs) = (contrast(rgb(ed.draft.text), rgb(ed.draft.bg)), contrast(rgb(ed.draft.text), rgb(ed.draft.surface)));
        let line_y = top + 4.0 * 44.0 + 14.0;
        if cb < 4.5 || cs < 4.5 {
            text(ui, pos2(main.left(), line_y), Align2::LEFT_CENTER, "Text is hard to read on these colors", Ts::new(Fam::Reg, 12.0, th.amber));
        } else {
            text(ui, pos2(main.left(), line_y), Align2::LEFT_CENTER, &format!("Text contrast {cb:.1} : 1"), Ts::new(Fam::Reg, 12.0, th.label));
        }

        let mut y = line_y + 22.0;
        ui.painter().line_segment([pos2(main.left(), y), pos2(main.right(), y)], Stroke::new(1.0, th.divider));
        y += 24.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "PREVIEW", t_section());
        y += 20.0;
        let x = main.left();
        blue_button(ui, Rect::from_min_size(pos2(x, y), vec2(110.0, 32.0)), "BUTTON", "pv-blue");
        dark_button(ui, Rect::from_min_size(pos2(x + 122.0, y), vec2(110.0, 32.0)), "BUTTON", "pv-dark", true);
        switch(ui, pos2(x + 268.0, y + 16.0), true, th.primary, "pv-switch");
        checkbox(ui, pos2(x + 320.0, y + 9.0), "OPTION", true, true, "pv-check");
        y += 40.0;
        let mut v = 0.6;
        slider(ui, Rect::from_min_size(pos2(x, y + 15.0), vec2(160.0, 2.0)), &mut v, 0.0..=1.0, "pv-slider");
        pill(ui, Rect::from_min_size(pos2(x + 196.0, y + 5.0), vec2(58.0, 22.0)), "10 MIN", true, "pv-pill-on");
        pill(ui, Rect::from_min_size(pos2(x + 262.0, y + 5.0), vec2(58.0, 22.0)), "30 MIN", false, "pv-pill-off");
        y += 46.0;

        let save = blue_button(ui, Rect::from_min_size(pos2(x, y), vec2(110.0, 32.0)), "SAVE", "theme-save").clicked();
        cancel |= dark_button(ui, Rect::from_min_size(pos2(x + 122.0, y), vec2(110.0, 32.0)), "CANCEL", "theme-cancel", true).clicked();
        let mut delete = false;
        if ed.existing.is_some() {
            let armed = ed.delete_armed.is_some_and(|t| t.elapsed().as_secs_f32() < 4.0);
            let label = if armed { "CLICK AGAIN TO DELETE" } else { "DELETE" };
            let w = if armed { 190.0 } else { 110.0 };
            if dark_button(ui, Rect::from_min_size(pos2(main.right() - w, y), vec2(w, 32.0)), label, "theme-delete", true).clicked() {
                if armed {
                    delete = true;
                } else {
                    ed.delete_armed = Some(Instant::now());
                }
            }
            if armed {
                ui.ctx().request_repaint_after(Duration::from_millis(250));
            }
        }

        let (draft, existing) = (ed.draft.clone(), ed.existing);
        if cancel {
            self.theme_edit = None;
        } else if save {
            self.theme_edit = None;
            self.edit(|c| {
                let ap = &mut c.appearance;
                let id = match existing.filter(|&i| i < ap.custom.len()) {
                    Some(i) => {
                        let mut t = draft;
                        t.name = ap.normalize_name(&t.name, Some(i));
                        t.id = ap.custom[i].id.clone();
                        let id = t.id.clone();
                        ap.custom[i] = t;
                        Some(id)
                    }
                    None => ap.add_theme(draft).map(|i| ap.custom[i].id.clone()),
                };
                if let Some(id) = id {
                    ap.theme = id;
                    ap.accent = None;
                }
            });
            self.say("Theme saved");
        } else if delete && let Some(i) = existing {
            self.theme_edit = None;
            self.edit(|c| {
                let ap = &mut c.appearance;
                if i < ap.custom.len() {
                    let gone = ap.custom.remove(i);
                    if ap.theme == gone.id {
                        ap.theme = "ghub-dark".into();
                    }
                }
            });
            self.say("Theme deleted");
        }
    }

    /// Open the editor on a draft (used by the shot pages).
    pub(crate) fn open_theme_editor(&mut self, draft: CustomTheme, existing: Option<usize>, role: usize) {
        self.theme_edit = Some(ThemeEditor::new(draft, existing, role));
        self.route = Route::Settings(1);
    }
}
