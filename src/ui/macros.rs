//! G HUB's full-screen macro editor: name page, type page and the editor with its sections.

pub use super::macros_model::*;
use super::theme::*;
use super::GhubApp;
use crate::catalog;
use crate::engine::EngineMsg;
use crate::profile::*;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use std::time::Instant;

const SWATCHES: [[u8; 3]; 9] = [
    [0x00, 0xB8, 0xFC],
    [0xFF, 0x00, 0x00],
    [0xFF, 0x6A, 0x00],
    [0xFF, 0xD6, 0x00],
    [0x00, 0xFF, 0x3C],
    [0x00, 0xFF, 0xE1],
    [0x2B, 0x3B, 0xFF],
    [0xD4, 0x00, 0xFF],
    [0xFF, 0xFF, 0xFF],
];

const KINDS: [MacroKind; 4] = [MacroKind::NoRepeat, MacroKind::RepeatWhileHolding, MacroKind::Toggle, MacroKind::Sequence];
const KIND_NAMES: [&str; 4] = ["NO REPEAT", "REPEAT WHILE HOLDING", "TOGGLE", "SEQUENCE"];
const KIND_TIPS: [&str; 4] = ["No repeat", "Repeat while holding", "Toggle", "Sequence"];
const KIND_HELP: [&str; 4] = [
    "Your macro will play once after you press the macro key.",
    "Your macro will loop continuously while the key is pressed.",
    "Your macro will loop continuously until you toggle it off by pressing again.",
    "Press to play the macro. Press and hold to repeat the \"while holding\" section.",
];
const SECTION_NAMES: [&str; 3] = ["ON PRESS", "WHILE HOLDING", "ON RELEASE"];

pub enum Mode {
    Text { sec: usize, idx: usize, buf: String, focus: bool },
    Choose { sec: usize, idx: usize, launch: bool, obs: bool, cat: Option<usize> },
}

impl Mode {
    fn target(&self) -> (usize, usize) {
        match self {
            Mode::Text { sec, idx, .. } | Mode::Choose { sec, idx, .. } => (*sec, *idx),
        }
    }
}

pub enum Confirm {
    Leave,
    Warn(String),
}

pub struct MacroEditor {
    pub stage: Stage,
    pub id: Option<String>,
    pub m: Model,
    pub menu: Option<usize>,
    pub rec: Option<usize>,
    /// When recording started (for the "REC 00:0N" pill).
    pub rec_at: Option<Instant>,
    rec_last: Option<Instant>,
    rec_mods: egui::Modifiers,
    pub mode: Option<Mode>,
    pub name_edit: bool,
    std_edit: Option<String>,
    /// In-place edit of a delay chip: (item, ms text, jitter text).
    delay_edit: Option<(ItemId, String, String)>,
    /// One of the delay edit fields held focus at least once (commit when both lose it).
    delay_focus: bool,
    /// In-place edit of the repeat count ("N TIMES").
    repeat_edit: Option<String>,
    pub color_open: bool,
    picker: super::light::PickerState,
    pub test_at: Option<Instant>,
    pub confirm: Option<Confirm>,
    drag: Option<Vec<ItemId>>,
    scroll: f32,
    stop_rect: Rect,
    menu_tile: Rect,
    list_scroll: f32,
    opened: u64,
}

impl MacroEditor {
    fn base(stage: Stage, m: Model, id: Option<String>) -> Self {
        Self {
            stage,
            id,
            m,
            menu: None,
            rec: None,
            rec_at: None,
            rec_last: None,
            rec_mods: Default::default(),
            mode: None,
            name_edit: false,
            std_edit: None,
            delay_edit: None,
            delay_focus: false,
            repeat_edit: None,
            color_open: false,
            picker: Default::default(),
            test_at: None,
            confirm: None,
            drag: None,
            scroll: 0.0,
            stop_rect: Rect::NOTHING,
            menu_tile: Rect::NOTHING,
            list_scroll: 0.0,
            opened: 0,
        }
    }

    /// CREATE NEW MACRO: starts at the name page.
    pub fn new_named() -> Self {
        Self::base(Stage::Name, Model::fresh("", MacroKind::NoRepeat), None)
    }

    pub fn new_of(name: &str, kind: MacroKind) -> Self {
        Self::base(Stage::Edit, Model::fresh(name, kind), None)
    }

    pub fn open(m: &Macro) -> Self {
        Self::base(Stage::Edit, Model::open(m), Some(m.id.clone()))
    }
}

/// Map a key to its Windows virtual-key code.
pub use super::assign::egui_vk;

/// Option checkbox: the box at `pos`, the label centred on `label_y`. True when clicked.
fn opt_check(ui: &Ui, pos: Pos2, label_y: f32, label: &str, on: bool, enabled: bool, id: &str) -> bool {
    let box_r = Rect::from_min_size(pos, vec2(14.0, 14.0));
    let col = if enabled { pal().text } else { pal().disabled };
    let lr = text(ui, pos2(pos.x + 24.0, label_y), Align2::LEFT_CENTER, label, Ts::new(Fam::Bold, 12.0, col).ls(0.8));
    let r = if enabled { click(ui, box_r.union(lr).expand(3.0), id) } else { hit(ui, box_r.union(lr).expand(3.0), id, Sense::hover()) };
    let p = ui.painter();
    if on {
        p.rect_filled(box_r, CornerRadius::same(2), if enabled { pal().n(0x9a) } else { pal().disabled });
        check(p, box_r.center(), 4.5, Stroke::new(1.8, pal().bg));
    } else {
        p.rect_stroke(box_r, CornerRadius::same(2), Stroke::new(1.5, col), StrokeKind::Inside);
    }
    r.clicked()
}

fn kind_index(k: MacroKind) -> usize {
    KINDS.iter().position(|&x| x == k).unwrap_or(0)
}

// ------------------------------------------------------------------ icons

fn kind_icon(p: &egui::Painter, c: Pos2, kind: usize, k: f32, color: Color32) {
    let st = Stroke::new(2.0, color);
    match kind {
        0 => {
            let (a, b) = (c - vec2(12.0 * k, 0.0), c + vec2(12.0 * k, 0.0));
            p.line_segment([a, b], st);
            p.add(Shape::line(vec![b + vec2(-8.0 * k, -8.0 * k), b, b + vec2(-8.0 * k, 8.0 * k)], st));
        }
        1 => {
            let r = 11.0 * k;
            let pts: Vec<Pos2> = (0..=27).map(|i| {
                let a = std::f32::consts::PI + (i as f32 / 27.0) * 1.5 * std::f32::consts::PI;
                c + vec2(a.cos(), a.sin()) * r
            }).collect();
            p.add(Shape::line(pts, st));
            let e = c + vec2(0.0, r);
            p.add(Shape::convex_polygon(vec![e + vec2(-5.0 * k, 0.0), e + vec2(k, -3.0 * k), e + vec2(k, 3.0 * k)], color, Stroke::NONE));
        }
        2 => {
            let r = Rect::from_center_size(c, vec2(34.0 * k, 20.0 * k));
            p.rect_stroke(r, CornerRadius::same((10.0 * k) as u8), st, StrokeKind::Inside);
            p.circle_filled(pos2(r.right() - 10.0 * k, c.y), 8.0 * k * 0.8 + 0.2, color);
        }
        _ => {
            let x0 = c.x - 19.0 * k;
            let y0 = c.y - 11.0 * k;
            for i in 0..3 {
                let r = Rect::from_min_size(pos2(x0 + 11.0 * k * i as f32, y0 + 3.0 * k * i as f32), vec2(16.0 * k, 16.0 * k));
                if i == 0 {
                    p.rect_filled(r, CornerRadius::same(3), color);
                } else {
                    p.rect_filled(r, CornerRadius::same(3), pal().bg);
                    p.rect_stroke(r, CornerRadius::same(3), Stroke::new(1.5, color), StrokeKind::Inside);
                }
            }
        }
    }
}

fn tri(p: &egui::Painter, cx: f32, y: f32, up: bool, color: Color32) {
    let (a, b) = if up { (y, y + 8.0) } else { (y, y - 8.0) };
    p.add(Shape::convex_polygon(vec![pos2(cx, a), pos2(cx - 8.0, b), pos2(cx + 8.0, b)], color, Stroke::NONE));
}

fn mouse_icon(p: &egui::Painter, tl: Pos2, vk: u16, color: Color32) {
    let r = Rect::from_min_size(tl, vec2(30.0, 46.0));
    let top = 18.0;
    let rad = 14u8;
    let fill = color.gamma_multiply(0.85);
    match vk {
        0x01 => {
            p.rect_filled(Rect::from_min_size(tl, vec2(15.0, top)), CornerRadius { nw: rad, ne: 0, sw: 0, se: 0 }, fill);
        }
        0x02 => {
            p.rect_filled(Rect::from_min_size(tl + vec2(15.0, 0.0), vec2(15.0, top)), CornerRadius { nw: 0, ne: rad, sw: 0, se: 0 }, fill);
        }
        0x04 => {
            p.rect_filled(Rect::from_min_size(tl + vec2(13.0, 4.0), vec2(4.0, 10.0)), CornerRadius::same(2), fill);
        }
        _ => {}
    }
    let st = Stroke::new(1.2, color);
    p.rect_stroke(r, CornerRadius::same(rad), st, StrokeKind::Inside);
    p.line_segment([tl + vec2(0.0, top), tl + vec2(30.0, top)], st);
    p.line_segment([tl + vec2(15.0, 0.0), tl + vec2(15.0, top)], st);
    if vk == 0x04 {
        p.rect_stroke(Rect::from_min_size(tl + vec2(13.0, 4.0), vec2(4.0, 10.0)), CornerRadius::same(2), st, StrokeKind::Inside);
    }
}

fn plus(p: &egui::Painter, c: Pos2, rot45: bool, color: Color32) {
    let st = Stroke::new(2.0, color);
    let d = 10.0;
    if rot45 {
        let e = d * std::f32::consts::FRAC_1_SQRT_2;
        p.line_segment([c + vec2(-e, -e), c + vec2(e, e)], st);
        p.line_segment([c + vec2(-e, e), c + vec2(e, -e)], st);
    } else {
        p.line_segment([c + vec2(-d, 0.0), c + vec2(d, 0.0)], st);
        p.line_segment([c + vec2(0.0, -d), c + vec2(0.0, d)], st);
    }
}

fn no_color_sign(p: &egui::Painter, c: Pos2, r: f32) {
    p.circle_filled(c, r, pal().text);
    p.circle_stroke(c, r - 1.0, Stroke::new(2.0, pal().gshift));
    let d = (r - 1.0) * std::f32::consts::FRAC_1_SQRT_2;
    p.line_segment([c + vec2(-d, -d), c + vec2(d, d)], Stroke::new(2.0, pal().gshift));
}

fn text_w(ui: &Ui, s: &str, ts: Ts) -> f32 {
    galley(ui, s, ts, f32::INFINITY).size().x
}

fn fit(ui: &Ui, s: &str, ts: Ts, max: f32) -> String {
    if text_w(ui, s, ts) <= max {
        return s.to_string();
    }
    let mut t: String = s.to_string();
    while t.pop().is_some() {
        let c = format!("{}\u{2026}", t);
        if text_w(ui, &c, ts) <= max {
            return c;
        }
    }
    "\u{2026}".into()
}

// ------------------------------------------------------------------ layout

enum UKind {
    Chip(Item),
    Holder,
}

struct Unit {
    x: f32,
    row: usize,
    kind: UKind,
}

fn chip_label(item: &Item) -> String {
    match &item.kind {
        ItemKind::Key { vk, .. } => catalog::vk_name(*vk),
        ItemKind::Fold(l) => l.clone(),
        _ => String::new(),
    }
}

fn chip_w(ui: &Ui, item: &Item, editing: bool) -> f32 {
    if editing {
        return 280.0;
    }
    match &item.kind {
        ItemKind::Key { .. } | ItemKind::Fold(_) => {
            let l = chip_label(item);
            if l.chars().count() > 2 { text_w(ui, &l, Ts::new(Fam::Reg, 16.0, pal().text).ls(0.0)) + 20.0 } else { 36.0 }
        }
        ItemKind::Mouse { .. } => 30.0,
        ItemKind::Text(t) => (text_w(ui, &format!("\u{201c}{}\u{201d}", t.replace('\n', " ")), Ts::new(Fam::Reg, 16.0, pal().text).ls(0.0)) + 24.0).clamp(55.0, 280.0),
        ItemKind::Sys(l) | ItemKind::Launch(l) | ItemKind::Obs(l) => text_w(ui, l, Ts::new(Fam::Reg, 14.0, pal().text).ls(0.0)) + 24.0,
        ItemKind::Delay { ms, jitter, .. } => {
            let w = text_w(ui, &fmt_delay(*ms).0, Ts::new(Fam::Reg, 21.0, pal().text).ls(0.0));
            let j = if *jitter > 0 { text_w(ui, &format!("±{jitter}"), Ts::new(Fam::Light, 14.0, pal().text).ls(0.0)) + 2.0 } else { 0.0 };
            (w + j + 6.0).max(40.0)
        }
    }
}

fn plan(ui: &Ui, ed: &MacroEditor, sec: usize, avail: f32) -> (Vec<Unit>, usize) {
    let mut units = Vec::new();
    let (mut x, mut row) = (0.0f32, 0usize);
    let edit_t = ed.mode.as_ref().map(Mode::target);
    for item in ed.m.items(sec) {
        let editing = edit_t == Some((sec, item.id.idx)) && !item.id.delay;
        let w = 15.0 + chip_w(ui, &item, editing) + 10.0;
        if x > 0.0 && x + w > avail {
            x = 0.0;
            row += 1;
        }
        units.push(Unit { x, row, kind: UKind::Chip(item) });
        x += w;
    }
    let w = 15.0 + 250.0;
    if x > 0.0 && x + w > avail {
        x = 0.0;
        row += 1;
    }
    units.push(Unit { x, row, kind: UKind::Holder });
    (units, row + 1)
}

// ------------------------------------------------------------------ pages

fn back_button(ui: &Ui, live: bool) -> bool {
    let c = pos2(ui.max_rect().left() + 39.0, ui.max_rect().top() + 58.0);
    let r = if live { click(ui, Rect::from_center_size(c, vec2(44.0, 44.0)), "macro-back") } else { hit(ui, Rect::from_center_size(c, vec2(44.0, 44.0)), "macro-back", Sense::hover()) };
    arrow_back(ui.painter(), c, Stroke::new(2.0, if r.hovered() { pal().primary } else { pal().text }));
    live && r.clicked()
}

impl GhubApp {
    /// The overlay that replaces the whole page while a macro is being created or edited.
    pub(crate) fn macro_overlay(&mut self, ui: &mut Ui, win: Rect) {
        let Some(mut ed) = self.asg.editor.take() else { return };
        if ed.opened == 0 {
            ed.opened = ui.ctx().cumulative_frame_nr().max(1);
        }
        let close = match ed.stage {
            Stage::Name => self.macro_name_page(ui, win, &mut ed),
            Stage::Type => self.macro_type_page(ui, win, &mut ed),
            Stage::Edit => self.macro_edit_page(ui, win, &mut ed),
        };
        if !close {
            self.asg.editor = Some(ed);
        }
    }

    fn macro_name_page(&mut self, ui: &mut Ui, win: Rect, ed: &mut MacroEditor) -> bool {
        let cx = win.center().x;
        let mut close = back_button(ui, true) || ui.input(|i| i.key_pressed(egui::Key::Escape));
        let field = Rect::from_center_size(pos2(cx, win.top() + 365.0), vec2(win.width() / 2.0, 40.0));
        let ts = Ts::new(Fam::Bold, 22.7, pal().text).ls(1.5);
        let mut layouter = |ui: &Ui, t: &dyn egui::TextBuffer, wrap: f32| {
            let mut job = egui::text::LayoutJob::single_section(t.as_str().to_owned(), egui::TextFormat { font_id: ts.font(), color: ts.color, extra_letter_spacing: ts.ls, ..Default::default() });
            job.wrap.max_width = wrap;
            ui.painter().layout_job(job)
        };
        let te = egui::TextEdit::singleline(&mut ed.m.name)
            .font(ts.font())
            .text_color(pal().text)
            .layouter(&mut layouter)
            .align(egui::Align2::CENTER_CENTER)
            .frame(egui::Frame::NONE)
            .char_limit(64);
        let resp = ui.put(field, te);
        if ed.m.name.is_empty() {
            text(ui, pos2(cx, win.top() + 365.0), Align2::CENTER_CENTER, "Name this macro", Ts::new(Fam::Bold, 22.7, pal().n(0x99)).ls(1.5));
        }
        let enter = ui.input(|i| i.key_pressed(egui::Key::Enter));
        if resp.lost_focus() && enter && !ed.m.name.trim().is_empty() {
            ed.m.name = ed.m.name.trim().to_string();
            ed.m.snap.name = ed.m.name.clone();
            ed.stage = Stage::Type;
        } else if self.shot.is_none() && !resp.has_focus() {
            resp.request_focus();
        }
        if !ed.m.name.is_empty() {
            text(ui, pos2(cx, win.top() + 412.0), Align2::CENTER_CENTER, "PRESS ENTER TO CREATE THIS MACRO", Ts::new(Fam::Reg, 10.7, pal().text).ls(1.5));
        }
        close &= ed.stage == Stage::Name;
        close
    }

    fn macro_type_page(&mut self, ui: &mut Ui, win: Rect, ed: &mut MacroEditor) -> bool {
        let cx = win.center().x;
        let mut close = back_button(ui, true) || ui.input(|i| i.key_pressed(egui::Key::Escape));
        let name = fit(ui, &ed.m.name, Ts::new(Fam::Bold, 24.0, pal().text).ls(0.0), win.width() / 2.0);
        text(ui, pos2(cx, win.top() + 159.0), Align2::CENTER_CENTER, &name, Ts::new(Fam::Bold, 24.0, pal().text).ls(0.0));
        text(ui, pos2(cx, win.top() + 298.0), Align2::CENTER_CENTER, "SELECT A TYPE OF MACRO YOU WANT TO CREATE", Ts::new(Fam::Bold, 12.0, pal().text).ls(1.0));
        for i in 0..4 {
            let tile = Rect::from_min_size(pos2(cx - 312.0 + 168.0 * i as f32, win.top() + 353.0), vec2(120.0, 120.0));
            let r = click(ui, tile, ("macro-tile", i));
            let hot = r.hovered();
            let p = ui.painter();
            p.rect_filled(tile, CornerRadius::same(4), if hot { pal().tint(pal().primary, 0.12) } else { pal().n(0x18) });
            if hot {
                p.rect_stroke(tile, CornerRadius::same(4), Stroke::new(2.0, pal().primary), StrokeKind::Inside);
            }
            kind_icon(p, tile.center(), i, 1.0, pal().text);
            let ts = Ts::new(Fam::Bold, 12.0, if hot { pal().primary } else { pal().text }).ls(1.0);
            let mut job = egui::text::LayoutJob::single_section(KIND_NAMES[i].to_owned(), egui::TextFormat { font_id: ts.font(), color: ts.color, extra_letter_spacing: ts.ls, ..Default::default() });
            job.wrap.max_width = 160.0;
            job.halign = egui::Align::Center;
            let g = ui.painter().layout_job(job);
            ui.painter().galley(pos2(tile.center().x + 2.0, tile.top() + 147.0), g, ts.color);
            if hot {
                let g = galley(ui, KIND_HELP[i], Ts::new(Fam::Reg, 12.0, pal().text).ls(0.0), 260.0);
                ui.painter().galley(pos2(tile.center().x - g.size().x / 2.0, tile.top() + 200.0), g, pal().text);
            }
            if r.clicked() {
                *ed = MacroEditor::new_of(&ed.m.name.clone(), KINDS[i]);
            }
        }
        close &= ed.stage == Stage::Type;
        close
    }

    fn save_macro(&mut self, ed: &mut MacroEditor) -> bool {
        let id = ed.id.clone().unwrap_or_else(new_id);
        let m = ed.m.to_macro(&id);
        let name = m.name.clone();
        self.edit(|c| {
            let ca = c.current_app;
            let ms = &mut c.apps[ca].macros;
            match ms.iter_mut().find(|x| x.id == m.id) {
                Some(x) => *x = m,
                None => ms.push(m),
            }
        });
        self.say(format!("Saved macro \"{name}\" - drag it onto a button"));
        true
    }

    fn try_save(&mut self, ed: &mut MacroEditor) -> bool {
        let id = ed.id.clone().unwrap_or_default();
        if let Some(w) = validate(&ed.m.to_macro(&id)) {
            ed.confirm = Some(Confirm::Warn(w.to_string()));
            return false;
        }
        self.save_macro(ed)
    }

    fn macro_edit_page(&mut self, ui: &mut Ui, win: Rect, ed: &mut MacroEditor) -> bool {
        let (l, r_x) = (win.left() + 64.0, win.right() - 64.0);
        let cx = win.center().x;
        let live = ed.confirm.is_none();
        let shot = self.shot.is_some();
        let busy = ed.rec.is_some() || ed.menu.is_some() || ed.mode.is_some();
        let mods = ui.input(|i| i.modifiers);
        let mut close = false;
        ed.m.active = ed.m.active.min(ed.m.sections() - 1);

        // recording
        if let Some(sec) = ed.rec {
            let (events, m) = ui.input(|i| (i.events.clone(), i.modifiers));
            let mut pushed: Vec<(u16, bool)> = Vec::new();
            for (vk, now, before) in [(0xA2u16, m.ctrl, ed.rec_mods.ctrl), (0xA0, m.shift, ed.rec_mods.shift), (0xA4, m.alt, ed.rec_mods.alt)] {
                if now != before {
                    pushed.push((vk, now));
                }
            }
            ed.rec_mods = m;
            let area = Rect::from_min_max(pos2(l, win.top() + 257.0), pos2(r_x, win.bottom() - 85.0));
            for e in events {
                match e {
                    egui::Event::Key { key, pressed, repeat: false, .. } => {
                        if let Some(vk) = egui_vk(key) {
                            pushed.push((vk, pressed));
                        }
                    }
                    egui::Event::PointerButton { pos, button, pressed, .. } if area.contains(pos) && !ed.stop_rect.contains(pos) => {
                        let vk = match button {
                            egui::PointerButton::Primary => 0x01,
                            egui::PointerButton::Secondary => 0x02,
                            egui::PointerButton::Middle => 0x04,
                            egui::PointerButton::Extra1 => 0x05,
                            egui::PointerButton::Extra2 => 0x06,
                        };
                        pushed.push((vk, pressed));
                    }
                    _ => {}
                }
            }
            for (vk, down) in pushed {
                let delay = ed.rec_last.map_or(0, |t| t.elapsed().as_millis().min(10_000) as u32);
                ed.rec_last = Some(Instant::now());
                ed.m.lists[sec].push(MacroStep::key(vk, down, delay));
            }
            ui.memory_mut(|mem| {
                if let Some(f) = mem.focused() {
                    mem.surrender_focus(f);
                }
            });
            ui.ctx().request_repaint();
        }

        // header
        if ed.rec.is_none() && back_button(ui, live && ed.mode.is_none()) {
            if ed.m.changed() {
                ed.confirm = Some(Confirm::Leave);
            } else {
                close = true;
            }
        }
        let name_ts = Ts::new(Fam::Bold, 24.0, pal().text).ls(0.0);
        let max_name = r_x - l - 380.0;
        if ed.name_edit {
            let te = egui::TextEdit::singleline(&mut ed.m.name).font(name_ts.font()).text_color(pal().text).frame(egui::Frame::NONE).char_limit(64);
            let resp = ui.put(Rect::from_min_size(pos2(l, win.top() + 115.0), vec2(max_name, 36.0)), te);
            if !resp.has_focus() && !resp.lost_focus() && !shot {
                resp.request_focus();
            }
            if resp.lost_focus() {
                if ed.m.name.trim().is_empty() {
                    ed.m.name = ed.m.snap.name.clone();
                } else {
                    ed.m.name = ed.m.name.trim().to_string();
                }
                ed.name_edit = false;
            }
        } else {
            let shown = fit(ui, &ed.m.name, name_ts, max_name);
            let rr = text(ui, pos2(l + 2.0, win.top() + 135.0), Align2::LEFT_CENTER, &shown, name_ts);
            let r = if live && !busy { click(ui, rr.expand2(vec2(4.0, 6.0)), "macro-name") } else { hit(ui, rr, "macro-name", Sense::hover()) };
            if r.clicked() {
                ed.name_edit = true;
                ed.opened = ui.ctx().cumulative_frame_nr();
            }
        }

        ui.set_opacity(if busy { 0.2 } else { 1.0 });
        let act = live && !busy;
        text(ui, pos2(l, win.top() + 166.0), Align2::LEFT_CENTER, "Macro types", Ts::new(Fam::Bold, 14.0, pal().text).ls(0.5));
        for i in 0..4 {
            let rect = Rect::from_min_size(pos2(l + 108.0 + 44.0 * i as f32, win.top() + 151.0), vec2(36.0, 30.0));
            let r = if act { click(ui, rect, ("kind-btn", i)) } else { hit(ui, rect, ("kind-btn", i), Sense::hover()) };
            let on = kind_index(ed.m.kind) == i || (act && r.hovered());
            ui.painter().rect_filled(rect, CornerRadius::same(4), if on { pal().tint(pal().primary, 0.2) } else { pal().n(0x18) });
            ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, if on { pal().primary } else { pal().ink(51) }), StrokeKind::Inside);
            kind_icon(ui.painter(), rect.center(), i, [0.79, 0.73, 0.65, 0.62][i], pal().text);
            if act && r.hovered() {
                super::device::tooltip(ui, pos2(rect.center().x - 24.0, rect.bottom() + 20.0), KIND_TIPS[i]);
            }
            if r.clicked() {
                ed.m.switch_kind(KINDS[i]);
            }
        }

        // options column
        let ox = r_x - 312.0;
        let top = win.top();
        if opt_check(ui, pos2(ox, top + 125.0), top + 136.0, "USE STANDARD DELAYS", ed.m.std, act, "opt-std") && act {
            ed.m.std = !ed.m.std;
        }
        let ms_txt = format!("{} ms", ed.m.std_ms);
        if let Some(buf) = &mut ed.std_edit {
            let te = egui::TextEdit::singleline(buf).font(Ts::new(Fam::Bold, 12.0, pal().text).font()).text_color(pal().text).frame(egui::Frame::NONE).char_limit(5).align(egui::Align2::RIGHT_CENTER);
            let rr = Rect::from_min_size(pos2(r_x - 16.0 - 60.0, top + 123.0), vec2(60.0, 20.0));
            ui.painter().line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, pal().primary));
            let resp = ui.put(rr, te);
            if !resp.has_focus() && !resp.lost_focus() {
                resp.request_focus();
            }
            if resp.lost_focus() {
                if let Ok(v) = buf.trim().parse::<u32>() {
                    ed.m.std_ms = v.clamp(1, 60_000);
                }
                ed.std_edit = None;
            }
        } else {
            let tr = text(ui, pos2(r_x - 16.0, top + 135.0), Align2::RIGHT_CENTER, &ms_txt, Ts::new(Fam::Bold, 12.0, pal().text).ls(0.0));
            let r = if act { click(ui, tr.expand2(vec2(6.0, 6.0)), "opt-ms") } else { hit(ui, tr, "opt-ms", Sense::hover()) };
            if r.clicked() {
                ed.std_edit = Some(ed.m.std_ms.to_string());
            }
        }
        let help = if ed.m.std { "Currently using a standard delay between actions" } else { "Currently using recorded input delays" };
        para(ui, pos2(ox + 24.0, top + 155.0), 264.0, help, Ts::new(Fam::Reg, 12.0, pal().text).ls(0.4));
        let mut y = top + 248.0;
        if ed.m.std {
            if opt_check(ui, pos2(ox, top + 201.0), top + 211.0, "SHOW KEY DOWN/KEY UP", ed.m.show_up_down, act, "opt-ud") && act {
                ed.m.show_up_down = !ed.m.show_up_down;
            }
        } else {
            y -= 38.0;
        }
        let cc = pos2(ox + 11.0, y - 2.0);
        let cr = if act { click(ui, Rect::from_min_max(pos2(ox, y - 14.0), pos2(ox + 160.0, y + 14.0)), "opt-color") } else { hit(ui, Rect::from_min_max(pos2(ox, y - 14.0), pos2(ox + 160.0, y + 14.0)), "opt-color", Sense::hover()) };
        match ed.m.color {
            Some(c) => {
                ui.painter().circle_filled(cc, 10.0, Color32::from_rgb(c[0], c[1], c[2]));
            }
            None => no_color_sign(ui.painter(), cc, 10.0),
        }
        text(ui, pos2(ox + 31.0, y - 2.0), Align2::LEFT_CENTER, "MACRO COLOR", Ts::new(Fam::Bold, 12.0, pal().text).ls(0.65));
        if cr.clicked() {
            ed.color_open = !ed.color_open;
        }
        let ty = y + 38.0;
        let tc = pos2(ox + 11.0, ty);
        let observer = crate::engine::observer_mode();
        let tr = Rect::from_min_max(pos2(ox, ty - 14.0), pos2(ox + 160.0, ty + 14.0));
        let t_on = act && !observer && ed.test_at.is_none();
        let tresp = if t_on { click(ui, tr, "opt-test") } else { hit(ui, tr, "opt-test", Sense::hover()) };
        ui.set_opacity(if busy { 0.2 } else if observer { 0.4 } else { 1.0 });
        ui.painter().circle_stroke(tc, 10.0, Stroke::new(1.5, pal().text));
        ui.painter().add(Shape::convex_polygon(vec![tc + vec2(-3.0, -5.0), tc + vec2(-3.0, 5.0), tc + vec2(5.0, 0.0)], pal().text, Stroke::NONE));
        let tl = match ed.test_at {
            Some(t) => format!("PLAYS IN {}...", 3 - t.elapsed().as_secs().min(2)),
            None => "TEST MACRO".into(),
        };
        text(ui, pos2(ox + 31.0, ty - 2.0), Align2::LEFT_CENTER, &tl, Ts::new(Fam::Bold, 12.0, pal().text).ls(1.0));
        ui.set_opacity(1.0);
        if observer && act && tresp.hovered() {
            super::device::tooltip(ui, pos2(ox + 150.0, ty), "Not available in dev mode");
        }
        if tresp.clicked() && t_on {
            ed.test_at = Some(Instant::now());
        }
        if let Some(t) = ed.test_at {
            ui.ctx().request_repaint();
            if t.elapsed().as_secs_f32() >= 3.0 {
                ed.test_at = None;
                let id = ed.id.clone().unwrap_or_default();
                let _ = self.eng.send(EngineMsg::TestMacro(Box::new(ed.m.to_macro(&id))));
            }
        }
        ui.set_opacity(1.0);
        // GHUB-X extra: a bounded repeat count for the looping kinds
        if ed.m.kind != MacroKind::NoRepeat {
            let ry = ty + 42.0;
            ui.set_opacity(if busy { 0.2 } else { 1.0 });
            text(ui, pos2(ox + 31.0, ry), Align2::LEFT_CENTER, "REPEAT", Ts::new(Fam::Bold, 12.0, pal().text).ls(1.0));
            let on = ed.m.repeat.is_some();
            let ts_off = Ts::new(Fam::Reg, 12.0, if on { pal().text_high } else { pal().primary }).ls(0.4);
            let r1 = text(ui, pos2(ox + 31.0, ry + 22.0), Align2::LEFT_CENTER, "UNTIL RELEASED", ts_off);
            if act && click(ui, r1.expand(4.0), "repeat-forever").clicked() {
                ed.m.repeat = None;
                ed.repeat_edit = None;
            }
            let nx = ox + 168.0;
            match &mut ed.repeat_edit {
                Some(buf) => {
                    let fr = Rect::from_min_size(pos2(nx, ry + 10.0), vec2(48.0, 24.0));
                    ui.painter().rect_filled(fr, CornerRadius::same(3), pal().bg);
                    let te = egui::TextEdit::singleline(buf).font(Ts::new(Fam::Reg, 12.0, pal().text).font()).text_color(pal().text).frame(egui::Frame::NONE).char_limit(4).align(egui::Align2::CENTER_CENTER);
                    let resp = ui.put(fr, te);
                    if !resp.has_focus() && !resp.lost_focus() {
                        resp.request_focus();
                    }
                    if resp.lost_focus() {
                        ed.m.repeat = Some(buf.trim().parse().unwrap_or(1).clamp(1, 9999));
                        ed.repeat_edit = None;
                    }
                    text(ui, pos2(fr.right() + 8.0, ry + 22.0), Align2::LEFT_CENTER, "TIMES", Ts::new(Fam::Reg, 12.0, pal().primary).ls(0.4));
                }
                None => {
                    let label = ed.m.repeat.map_or_else(|| "N TIMES".to_owned(), |n| format!("{n} TIMES"));
                    let ts_n = Ts::new(Fam::Reg, 12.0, if on { pal().primary } else { pal().text_high }).ls(0.4);
                    let r2 = text(ui, pos2(nx, ry + 22.0), Align2::LEFT_CENTER, &label, ts_n);
                    if act && click(ui, r2.expand(4.0), "repeat-n").clicked() {
                        if ed.m.repeat.is_none() {
                            ed.m.repeat = Some(1);
                        }
                        ed.repeat_edit = Some(ed.m.repeat.unwrap_or(1).to_string());
                    }
                }
            }
            ui.set_opacity(1.0);
        }
        let popup_at = pos2(ox, y + 20.0);
        let color_row = Rect::from_min_max(pos2(ox, y - 14.0), pos2(ox + 160.0, y + 14.0));

        // recording line: pulsing "REC mm:ss" pill + what is being captured
        if ed.rec.is_some() {
            let secs = ed.rec_at.map(|t| t.elapsed().as_secs()).unwrap_or(0).min(5999);
            let clock = if self.shot.is_some() { "REC 00:00".to_owned() } else { format!("REC {:02}:{:02}", secs / 60, secs % 60) };
            let y = win.top() + 199.0;
            let pill_r = Rect::from_center_size(pos2(l + 4.0 + text_w(ui, &clock, Ts::new(Fam::Bold, 12.0, pal().gshift).ls(0.6)) / 2.0 + 16.0, y), vec2(text_w(ui, &clock, Ts::new(Fam::Bold, 12.0, pal().gshift).ls(0.6)) + 30.0, 24.0));
            ui.painter().rect_filled(pill_r, CornerRadius::same(12), pal().tint(pal().gshift, 0.12));
            let pulse = (ui.input(|i| i.time) * 2.5).sin().abs() as f32;
            let dot = pal().tint(pal().gshift, 0.45 + 0.55 * pulse);
            ui.painter().circle_filled(pos2(l + 14.0, y), 4.0 + pulse * 1.6, dot);
            text(ui, pos2(l + 26.0, y), Align2::LEFT_CENTER, &clock, Ts::new(Fam::Bold, 12.0, pal().gshift).ls(0.6));
            text(ui, pos2(pill_r.right() + 12.0, y), Align2::LEFT_CENTER, "Recording keystrokes and mouse clicks", Ts::new(Fam::Reg, 14.0, pal().gshift).ls(0.0));
            ui.ctx().request_repaint();
        }

        // sections
        let area = Rect::from_min_max(pos2(l, win.top() + 257.0), pos2(r_x, win.bottom() - 85.0));
        self.macro_sections(ui, ed, area, win, mods, live && !ed.color_open);
        if ed.menu.is_some() {
            self.macro_menu(ui, ed, win, live && !ed.color_open);
        }
        if ed.color_open {
            self.color_popup(ui, ed, popup_at, color_row, live);
        }

        // bottom buttons
        let by = win.bottom() - 64.0;
        let can_reset = ed.m.can_reset() && !busy && live;
        let can_save = ed.m.can_save() && !busy && live;
        for (i, (label, fill, on)) in [("RESET", pal().n(0x25), can_reset), ("SAVE", pal().primary, can_save)].into_iter().enumerate() {
            let x0 = if i == 0 { cx - 208.0 } else { cx + 8.0 };
            let rect = Rect::from_min_size(pos2(x0, by), vec2(200.0, 32.0));
            let r = if on { click(ui, rect, ("macro-bottom", i)) } else { hit(ui, rect, ("macro-bottom", i), Sense::hover()) };
            ui.set_opacity(if on { 1.0 } else { 0.4 });
            ui.painter().rect_filled(rect, CornerRadius::ZERO, fill);
            text(ui, rect.center(), Align2::CENTER_CENTER, label, Ts::new(Fam::Reg, 12.0, if i == 1 { pal().on_primary } else { pal().text }).ls(1.8));
            ui.set_opacity(1.0);
            if r.clicked() {
                if i == 0 {
                    ed.m.reset();
                    ed.menu = None;
                    ed.mode = None;
                    ed.rec = None;
                } else if self.try_save(ed) {
                    close = true;
                }
            }
        }

        // keyboard
        let typing = ui.memory(|m| m.focused().is_some());
        if ed.rec.is_none() && ed.mode.is_none() && !typing && live && !ed.name_edit {
            let (del, all, copy, cut, paste, esc) = ui.input(|i| {
                (
                    i.key_pressed(egui::Key::Delete) || i.key_pressed(egui::Key::Backspace),
                    i.modifiers.command && i.key_pressed(egui::Key::A),
                    i.events.iter().any(|e| matches!(e, egui::Event::Copy)),
                    i.events.iter().any(|e| matches!(e, egui::Event::Cut)),
                    i.events.iter().any(|e| matches!(e, egui::Event::Paste(_))),
                    i.key_pressed(egui::Key::Escape),
                )
            });
            if del {
                ed.m.delete_selection();
            } else if all {
                ed.m.select_all();
            } else if copy {
                ed.m.copy_selection();
            } else if cut {
                ed.m.cut_selection();
            } else if paste {
                ed.m.paste();
            } else if esc {
                if ed.menu.is_some() {
                    ed.menu = None;
                } else {
                    ed.m.sel.clear();
                }
            }
        } else if ui.input(|i| i.key_pressed(egui::Key::Escape)) && ed.rec.is_none() {
            ed.menu = None;
            if ed.confirm.is_some() {
                ed.confirm = None;
            }
        }

        if ed.confirm.is_some() {
            close |= self.macro_confirm(ui, ed, win);
        }
        close
    }

    fn color_popup(&mut self, ui: &mut Ui, ed: &mut MacroEditor, tl: Pos2, row: Rect, live: bool) {
        let rect = Rect::from_min_size(tl, vec2(300.0, 262.0));
        hit(ui, rect, "color-block", Sense::click_and_drag());
        if ui.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| !rect.contains(p) && !row.contains(p))) {
            ed.color_open = false;
            return;
        }
        ui.painter().rect_filled(rect, CornerRadius::same(6), pal().surface);
        let cur = ed.m.color.unwrap_or(SWATCHES[0]);
        if let Some(c) = GhubApp::color_picker(ui, tl + vec2(11.0, 10.0), cur, &mut ed.picker, "macro-color", &SWATCHES) {
            ed.m.color = Some(c);
        }
        let by = tl.y + 218.0;
        if dark_button(ui, Rect::from_min_size(pos2(tl.x + 11.0, by), vec2(130.0, 28.0)), "NO COLOR", "color-none", live).clicked() && live {
            ed.m.color = None;
            ed.color_open = false;
        }
        if blue_button(ui, Rect::from_min_size(pos2(tl.x + 159.0, by), vec2(130.0, 28.0)), "DONE", "color-done").clicked() && live {
            ed.color_open = false;
        }
    }

    fn macro_confirm(&mut self, ui: &mut Ui, ed: &mut MacroEditor, win: Rect) -> bool {
        ui.painter().rect_filled(win, CornerRadius::ZERO, pal().bg.gamma_multiply(0.6));
        hit(ui, win, "confirm-block", Sense::click_and_drag());
        let msg = match &ed.confirm {
            Some(Confirm::Leave) => "Would you like to save your macro changes?".to_string(),
            Some(Confirm::Warn(w)) => w.clone(),
            None => return false,
        };
        let panel_w = 560.0;
        let g = galley(ui, &msg, Ts::new(Fam::Reg, 14.0, pal().text).ls(0.0), panel_w - 40.0);
        let h = g.size().y + 40.0 + 32.0 + 20.0;
        let c = pos2(win.center().x, win.bottom() - 150.0);
        let rect = Rect::from_center_size(c, vec2(panel_w, h));
        ui.painter().rect_filled(rect, CornerRadius::same(8), pal().surface);
        ui.painter().galley(pos2(rect.left() + 20.0, rect.top() + 20.0), g, pal().text);
        let by = rect.bottom() - 52.0;
        let yes = blue_button(ui, Rect::from_min_size(pos2(c.x - 130.0, by), vec2(120.0, 32.0)), "YES", "confirm-yes").clicked();
        let no = dark_button(ui, Rect::from_min_size(pos2(c.x + 10.0, by), vec2(120.0, 32.0)), "NO", "confirm-no", true).clicked();
        let was = ed.confirm.take();
        match (was, yes, no) {
            (Some(Confirm::Leave), true, _) => {
                let ok = self.try_save_or_warn(ed);
                return ok;
            }
            (Some(Confirm::Leave), _, true) => return true,
            (Some(Confirm::Warn(_)), true, _) => return self.save_macro(ed),
            (Some(Confirm::Warn(_)), _, true) => {}
            (other, false, false) => ed.confirm = other,
            _ => {}
        }
        false
    }

    /// YES on the leave question: save, with the unbalanced-key warning if there is one.
    fn try_save_or_warn(&mut self, ed: &mut MacroEditor) -> bool {
        if !ed.m.can_save() {
            return true;
        }
        self.try_save(ed)
    }

    fn macro_menu(&mut self, ui: &mut Ui, ed: &mut MacroEditor, win: Rect, live: bool) {
        let Some(sec) = ed.menu else { return };
        const ENTRIES: [(&str, u8); 6] = [("RECORD KEYSTROKES", 0), ("TEXT & EMOJIS", 1), ("ACTION", 5), ("LAUNCH APPLICATION", 2), ("SYSTEM", 3), ("DELAY", 4)];
        let tile = ed.menu_tile;
        let n = ENTRIES.len() as f32;
        let total = n * 36.0 + (n - 1.0) * 8.0;
        let mut top = tile.top() - 16.0 - 110.0;
        top = top.clamp(win.top() + 8.0, (win.bottom() - total - 8.0).max(win.top() + 8.0));
        let x = tile.right() + 16.0;
        let mut pick: Option<u8> = None;
        for (i, (label, kind)) in ENTRIES.iter().enumerate() {
            let ts = Ts::new(Fam::Reg, 12.0, pal().text).ls(0.0);
            let w = 8.0 + 16.0 + 8.0 + text_w(ui, label, ts) + 12.0;
            let rect = Rect::from_min_size(pos2(x, top + i as f32 * 44.0), vec2(w, 36.0));
            let r = if live { click(ui, rect, ("menu", i)) } else { hit(ui, rect, ("menu", i), Sense::hover()) };
            let fill = match kind {
                0 => pal().tint(pal().gshift, 0.4),
                1 => pal().tint(pal().amber, 0.4),
                5 => pal().tint(pal().orange(), 0.4),
                2 => pal().n(0x65),
                3 => pal().n(0x33),
                _ => pal().n(0x48),
            };
            let p = ui.painter();
            p.rect_filled(rect, CornerRadius::same(4), fill);
            if r.hovered() {
                p.rect_filled(rect, CornerRadius::same(4), pal().ink(26));
            }
            let ic = pos2(rect.left() + 16.0, rect.center().y);
            match kind {
                0 => {
                    p.circle_filled(ic, 8.0, pal().gshift);
                    p.circle_stroke(ic, 8.0, Stroke::new(1.0, pal().text));
                }
                1 => {
                    p.circle_filled(ic, 8.0, pal().amber);
                    text(ui, ic, Align2::CENTER_CENTER, "T", Ts::new(Fam::Bold, 11.0, pal().bg).ls(0.0));
                }
                5 => {
                    p.circle_filled(ic, 8.0, pal().orange());
                    p.add(Shape::convex_polygon(vec![ic + vec2(1.5, -5.0), ic + vec2(-3.0, 0.8), ic + vec2(-0.2, 0.8), ic + vec2(-1.5, 5.0), ic + vec2(3.0, -1.0), ic + vec2(0.2, -1.0)], pal().bg, Stroke::NONE));
                }
                2 => {
                    p.circle_filled(ic, 8.0, pal().n(0xaf));
                    let st = Stroke::new(1.25, pal().bg);
                    p.line_segment([ic + vec2(-3.0, 3.0), ic + vec2(3.0, -3.0)], st);
                    p.line_segment([ic + vec2(-0.5, -3.0), ic + vec2(3.0, -3.0)], st);
                    p.line_segment([ic + vec2(3.0, -3.0), ic + vec2(3.0, 0.5)], st);
                }
                3 => {
                    p.circle_filled(ic, 8.0, pal().n(0xaf));
                    text(ui, ic, Align2::CENTER_CENTER, "S", Ts::new(Fam::Bold, 11.0, pal().bg).ls(0.0));
                }
                _ => {
                    p.circle_filled(ic, 8.0, pal().n(0xaf));
                    let st = Stroke::new(1.25, pal().bg);
                    p.line_segment([ic, ic + vec2(0.0, -4.5)], st);
                    p.line_segment([ic, ic + vec2(3.5, 1.5)], st);
                }
            }
            text(ui, pos2(rect.left() + 32.0, rect.center().y), Align2::LEFT_CENTER, label, ts);
            if r.clicked() {
                pick = Some(*kind);
            }
        }
        let Some(kind) = pick else { return };
        ed.menu = None;
        ed.m.active = sec;
        match kind {
            0 => {
                ed.rec = Some(sec);
                ed.rec_at = Some(Instant::now());
                ed.rec_last = None;
                ed.rec_mods = ui.input(|i| i.modifiers);
            }
            1 => {
                let idx = ed.m.lists[sec].len();
                ed.m.lists[sec].push(MacroStep::text("", 0));
                ed.mode = Some(Mode::Text { sec, idx, buf: String::new(), focus: true });
            }
            2 | 3 | 5 => {
                let idx = ed.m.lists[sec].len();
                ed.m.lists[sec].push(MacroStep::run(Assignment::Default, 0));
                ed.mode = Some(Mode::Choose { sec, idx, launch: kind == 2, obs: kind == 5, cat: None });
                ed.list_scroll = 0.0;
            }
            _ => {
                ed.m.std = false;
                let d = ed.m.std_ms;
                ed.m.lists[sec].push(MacroStep::pause(d));
            }
        }
    }
}

// ------------------------------------------------------------------ sections

impl GhubApp {
    fn macro_sections(&mut self, ui: &mut Ui, ed: &mut MacroEditor, area: Rect, win: Rect, mods: egui::Modifiers, live: bool) {
        let nsec = ed.m.sections();
        let avail = area.width() - 32.0;
        let plans: Vec<(Vec<Unit>, usize)> = (0..nsec).map(|s| plan(ui, ed, s, avail)).collect();
        // vertical layout in content space (relative to the area top)
        let seq = nsec == 3;
        let mut tops = Vec::new();
        let mut caps = Vec::new();
        let total = if seq {
            let mut y = 45.0;
            for (s, (_, rows)) in plans.iter().enumerate() {
                y += 32.0;
                let active = ed.m.active == s;
                caps.push(y);
                y += if active { 19.0 } else { 17.0 } + 16.0;
                tops.push(y);
                y += *rows as f32 * 80.0;
            }
            y + 32.0
        } else {
            let block = plans[0].1 as f32 * 80.0 + 32.0;
            let free = area.height() - 45.0;
            let t = if block < free { 45.0 + (free - block) / 2.0 } else { 45.0 };
            tops.push(t);
            t + block
        };
        let max_scroll = (total - area.height()).max(0.0);
        if ui.rect_contains_pointer(area) {
            ed.scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        ed.scroll = ed.scroll.clamp(0.0, max_scroll);
        let scroll = ed.scroll;

        let mut sub = ui.new_child(UiBuilder::new().max_rect(area));
        sub.set_clip_rect(area.intersect(ui.clip_rect()));
        let area_resp = hit(&sub, area, "macro-area", Sense::click());
        let focus_sec = ed.rec.or(ed.mode.as_ref().map(|m| m.target().0)).or(ed.menu);
        let mut gaps: Vec<(usize, usize, Pos2)> = Vec::new();
        let mut select: Option<(ItemId, bool)> = None; // (id, double)
        let mut start_drag: Option<ItemId> = None;
        let mut open_menu: Option<usize> = None;
        let mut stop_rec = false;
        let mut finish_mode: Option<bool> = None; // true = commit, false = cancel
        let mut choose: Option<Option<MacroStep>> = None;
        let mut delay_commit: Option<(ItemId, u32, u32)> = None;
        let outside_press = ui.input(|i| i.pointer.any_pressed()).then(|| ui.input(|i| i.pointer.interact_pos())).flatten();

        for (sec, (units, _)) in plans.iter().enumerate() {
            let dim = focus_sec.is_some_and(|f| f != sec);
            let act = live && !dim && ed.rec.is_none();
            sub.set_opacity(if dim { 0.2 } else { 1.0 });
            let row_y = |row: usize| area.top() - scroll + tops[sec.min(tops.len() - 1)] + row as f32 * 80.0;
            if seq {
                let active = ed.m.active == sec;
                let ts = if active { Ts::new(Fam::Reg, 13.0, pal().text).ls(1.2) } else { Ts::new(Fam::Light, 12.0, pal().text).ls(1.2) };
                text(&sub, pos2(area.left(), area.top() - scroll + caps[sec] + if active { 10.0 } else { 9.0 }), Align2::LEFT_CENTER, SECTION_NAMES[sec], ts);
            }
            let list_len = ed.m.lists[sec].len();
            for unit in units {
                let top = row_y(unit.row);
                let x0 = area.left() + unit.x;
                let (g, cell_x) = match &unit.kind {
                    UKind::Chip(it) => (it.id.idx, x0 + 15.0 + 5.0),
                    UKind::Holder => (list_len, x0 + 15.0),
                };
                gaps.push((sec, g, pos2(x0 + 7.5, top + 40.0)));
                let cy = top + 40.0;
                match &unit.kind {
                    UKind::Chip(item) => {
                        let editing = ed.mode.as_ref().map(Mode::target) == Some((sec, item.id.idx)) && !item.id.delay;
                        if editing {
                            let w = 280.0;
                            let bx = Rect::from_min_size(pos2(cell_x, top + 4.0), vec2(w, 36.0));
                            match ed.mode.as_mut() {
                                Some(Mode::Text { buf, focus, .. }) => {
                                    let rect = Rect::from_min_size(pos2(cell_x, top - 8.0), vec2(280.0, 176.0));
                                    sub.painter().rect_filled(rect, CornerRadius::same(4), pal().bg);
                                    sub.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, pal().amber), StrokeKind::Inside);
                                    let te = egui::TextEdit::multiline(buf).font(Ts::new(Fam::Reg, 16.0, pal().text).font()).text_color(pal().text).frame(egui::Frame::NONE).hint_text("Enter text here").desired_width(256.0);
                                    let resp = sub.put(rect.shrink(12.0), te);
                                    if *focus {
                                        if self.shot.is_none() {
                                            resp.request_focus();
                                        }
                                        *focus = false;
                                    }
                                    if outside_press.is_some_and(|p| !rect.contains(p) && !ed.stop_rect.contains(p)) {
                                        finish_mode = Some(true);
                                    }
                                }
                                Some(Mode::Choose { launch, obs, cat, .. }) => {
                                    let (launch, obs, cat) = (*launch, *obs, *cat);
                                    choose = self.chooser(&mut sub, ed, bx, launch, obs, cat, sec);
                                }
                                None => {}
                            }
                            continue;
                        }
                        let selected = ed.m.sel.contains(&item.id);
                        let rect = self.draw_item(&sub, item, cell_x, top, selected, ed.rec == Some(sec));
                        let r = if act { hit(&sub, rect, ("item", sec, item.id.idx, item.id.delay), Sense::click_and_drag()) } else { hit(&sub, rect, ("item", sec, item.id.idx, item.id.delay), Sense::hover()) };
                        if r.drag_started() {
                            start_drag = Some(item.id);
                        } else if r.double_clicked() {
                            select = Some((item.id, true));
                        } else if r.clicked() {
                            select = Some((item.id, false));
                        }
                        if let Some((id, ms_buf, jit_buf)) = &mut ed.delay_edit
                            && *id == item.id
                            && let ItemKind::Delay { .. } = item.kind
                        {
                            // DELAY on the chip, JITTER under it; commit when both lose focus
                            let ms_id = sub.make_persistent_id(("delay-ms", id.sec, id.idx, id.delay));
                            let jit_id = sub.make_persistent_id(("delay-jit", id.sec, id.idx, id.delay));
                            let w = rect.width().max(56.0);
                            let rr = Rect::from_center_size(pos2(rect.center().x, cy - 8.0), vec2(w, 26.0));
                            let rr2 = Rect::from_center_size(pos2(rect.center().x, cy + 24.0), vec2(w, 22.0));
                            sub.painter().rect_filled(rr, CornerRadius::same(3), pal().bg);
                            sub.painter().rect_filled(rr2, CornerRadius::same(3), pal().bg);
                            text(&sub, pos2(rr2.left() + 5.0, rr2.center().y), Align2::LEFT_CENTER, "±", Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.0));
                            let te = egui::TextEdit::singleline(ms_buf).id(ms_id).font(Ts::new(Fam::Reg, 18.0, pal().text).font()).text_color(pal().text).frame(egui::Frame::NONE).char_limit(6).align(egui::Align2::CENTER_CENTER);
                            let resp = sub.put(rr, te);
                            let te2 = egui::TextEdit::singleline(jit_buf).id(jit_id).font(Ts::new(Fam::Reg, 11.0, pal().muted).font()).text_color(pal().muted).frame(egui::Frame::NONE).char_limit(6).align(egui::Align2::CENTER_CENTER);
                            let _resp2 = sub.put(rr2, te2);
                            let (f1, f2) = sub.memory(|m| (m.has_focus(ms_id), m.has_focus(jit_id)));
                            if !f1 && !f2 && !ed.delay_focus && !resp.lost_focus() {
                                resp.request_focus();
                            }
                            if f1 || f2 {
                                ed.delay_focus = true;
                            }
                            // a click moving between the two fields is not "lose focus"
                            let clicked_inside = sub.input(|i| i.pointer.any_pressed() && i.pointer.interact_pos().is_some_and(|p| rr.contains(p) || rr2.contains(p)));
                            let enter = sub.input(|i| i.key_pressed(egui::Key::Enter));
                            if ed.delay_focus && ((!f1 && !f2 && !clicked_inside) || (enter && (f1 || f2))) {
                                delay_commit = Some((*id, ms_buf.trim().parse().unwrap_or(0), jit_buf.trim().parse().unwrap_or(0)));
                            }
                        }
                    }
                    UKind::Holder => {
                        let hx = x0 + 15.0;
                        let tile = Rect::from_min_size(pos2(hx + 16.0, top + 16.0), vec2(48.0, 48.0));
                        let recording_here = ed.rec == Some(sec);
                        let editing_here = ed.mode.as_ref().map(Mode::target).is_some_and(|t| t.0 == sec);
                        let open = ed.menu == Some(sec);
                        let holder = Rect::from_min_size(pos2(hx, top), vec2(250.0, 80.0));
                        let r = if live && (!dim || recording_here || editing_here) { click(&sub, holder, ("holder", sec)) } else { hit(&sub, holder, ("holder", sec), Sense::hover()) };
                        if recording_here {
                            ed.stop_rect = holder;
                            sub.painter().rect_filled(tile, CornerRadius::same(2), if r.hovered() { pal().tint(pal().n(0x39), 0.8) } else { pal().n(0x39) });
                            let sq = Rect::from_center_size(tile.center(), vec2(24.0, 24.0));
                            sub.painter().rect_filled(sq, CornerRadius::same(2), pal().gshift);
                            sub.painter().rect_stroke(sq, CornerRadius::same(2), Stroke::new(2.0, pal().text), StrokeKind::Inside);
                            text(&sub, pos2(tile.right() + 16.0, tile.center().y), Align2::LEFT_CENTER, "STOP RECORDING", Ts::new(Fam::Bold, 12.0, pal().text).ls(1.0));
                            if r.clicked() {
                                stop_rec = true;
                            }
                        } else if editing_here {
                            let is_text = matches!(ed.mode, Some(Mode::Text { .. }));
                            ed.stop_rect = holder;
                            sub.painter().rect_filled(tile, CornerRadius::same(2), if r.hovered() { pal().tint(pal().n(0x39), 0.8) } else { pal().n(0x39) });
                            if is_text {
                                check(sub.painter(), tile.center(), 9.0, Stroke::new(2.5, pal().text));
                            } else {
                                caption_glyph(sub.painter(), tile.center(), 2, pal().text);
                            }
                            text(&sub, pos2(tile.right() + 16.0, tile.center().y), Align2::LEFT_CENTER, if is_text { "DONE" } else { "CANCEL" }, Ts::new(Fam::Bold, 12.0, pal().text).ls(1.0));
                            if r.clicked() {
                                finish_mode = Some(is_text);
                            }
                        } else {
                            sub.painter().rect_filled(tile, CornerRadius::same(2), if r.hovered() { pal().tint(pal().n(0x39), 0.8) } else { pal().n(0x39) });
                            plus(sub.painter(), tile.center(), open, pal().text);
                            if list_len == 0 && !open {
                                text(&sub, pos2(tile.right() + 16.0, tile.center().y), Align2::LEFT_CENTER, "START NOW", Ts::new(Fam::Bold, 12.0, pal().text).ls(1.0));
                            }
                            if r.clicked() {
                                open_menu = Some(sec);
                            }
                            if open {
                                ed.menu_tile = tile;
                            }
                        }
                    }
                }
            }
            sub.set_opacity(1.0);
        }

        // thin scroll bar
        if max_scroll > 0.0 {
            let h = area.height() * area.height() / total;
            let y = area.top() + (area.height() - h) * (scroll / max_scroll);
            ui.painter().rect_filled(Rect::from_min_max(pos2(area.right() - 4.0, area.top()), area.right_bottom()), CornerRadius::same(2), pal().n(0x1a));
            ui.painter().rect_filled(Rect::from_min_size(pos2(area.right() - 4.0, y), vec2(4.0, h)), CornerRadius::same(2), pal().n(0xb7));
        }

        // pointer interactions after drawing
        if area_resp.clicked() && ed.mode.is_none() && ed.rec.is_none() {
            ed.m.sel.clear();
            ed.menu = None;
        }
        if let Some(sec) = open_menu {
            ed.menu = if ed.menu == Some(sec) { None } else { Some(sec) };
        }
        if let Some((id, double)) = select {
            ed.m.active = id.sec;
            let was_only = ed.m.sel == vec![id];
            if mods.shift && ed.m.sel.last().is_some_and(|a| a.sec == id.sec) {
                let anchor = *ed.m.sel.first().unwrap_or(&id);
                let items = ed.m.items(id.sec);
                let pa = items.iter().position(|i| i.id == anchor).unwrap_or(0);
                let pb = items.iter().position(|i| i.id == id).unwrap_or(0);
                ed.m.sel = items[pa.min(pb)..=pa.max(pb)].iter().map(|i| i.id).collect();
            } else if mods.command {
                if let Some(p) = ed.m.sel.iter().position(|s| *s == id) {
                    ed.m.sel.remove(p);
                } else {
                    ed.m.sel.push(id);
                }
            } else {
                ed.m.sel = vec![id];
                if double || was_only {
                    self.start_item_edit(ed, id);
                }
            }
        }
        if let Some((id, v, j)) = delay_commit {
            if let Some(s) = ed.m.lists[id.sec].get_mut(id.idx) {
                s.delay_ms = v.min(600_000);
                s.jitter = j.min(600_000);
            }
            ed.delay_edit = None;
            ed.delay_focus = false;
        }
        if stop_rec {
            ed.rec = None;
        }
        if let Some(commit) = finish_mode {
            self.finish_mode(ed, commit);
        }
        if let Some(c) = choose {
            if let Some(step) = c {
                if let Some(Mode::Choose { sec, idx, .. }) = ed.mode.take()
                    && let Some(s) = ed.m.lists[sec].get_mut(idx)
                {
                    *s = step;
                }
            } else {
                self.finish_mode(ed, false);
            }
        }
        if let Some(id) = start_drag {
            if !ed.m.sel.contains(&id) {
                ed.m.sel = vec![id];
            }
            ed.drag = Some(ed.m.sel.clone());
        }
        if ed.drag.is_some() {
            if let Some(p) = ui.input(|i| i.pointer.interact_pos()) {
                let best = gaps.iter().min_by(|a, b| a.2.distance(p).total_cmp(&b.2.distance(p)));
                if let Some(&(sec, g, c)) = best {
                    ui.painter().rect_stroke(Rect::from_center_size(c, vec2(40.0, 40.0)), CornerRadius::same(10), Stroke::new(2.0, pal().primary_dim), StrokeKind::Inside);
                    if ui.input(|i| i.pointer.any_released()) {
                        ed.drag = None;
                        ed.m.move_selection(sec, g);
                    }
                }
                let gh = Rect::from_center_size(p, vec2(36.0, 36.0));
                ui.painter().rect_filled(gh, CornerRadius::same(4), pal().tint(pal().primary, 0.4));
            }
            if ui.input(|i| i.pointer.any_released()) {
                ed.drag = None;
            }
            ui.ctx().request_repaint();
        }
        let _ = win;
    }

    fn start_item_edit(&mut self, ed: &mut MacroEditor, id: ItemId) {
        let items = ed.m.items(id.sec);
        let Some(it) = items.iter().find(|i| i.id == id) else { return };
        match &it.kind {
            ItemKind::Text(t) => ed.mode = Some(Mode::Text { sec: id.sec, idx: id.idx, buf: t.clone(), focus: true }),
            ItemKind::Delay { ms, jitter, .. } => {
                ed.delay_edit = Some((id, ms.to_string(), jitter.to_string()));
                ed.delay_focus = false;
            }
            _ => {}
        }
    }

    /// Commit (text) or cancel the step being edited; an empty text or a cancelled chooser removes it.
    fn finish_mode(&mut self, ed: &mut MacroEditor, commit: bool) {
        let Some(mode) = ed.mode.take() else { return };
        match mode {
            Mode::Text { sec, idx, buf, .. } => {
                if commit && !buf.is_empty() {
                    if let Some(s) = ed.m.lists[sec].get_mut(idx) {
                        s.act = Some(StepAct::Text(buf));
                    }
                } else if idx < ed.m.lists[sec].len() {
                    ed.m.lists[sec].remove(idx);
                }
            }
            Mode::Choose { sec, idx, .. } => {
                if idx < ed.m.lists[sec].len() {
                    ed.m.lists[sec].remove(idx);
                }
            }
        }
    }

    /// The SYSTEM / LAUNCH APPLICATION chooser. Returns `Some(Some(step))` when a card was picked.
    #[allow(clippy::too_many_arguments)]
    fn chooser(&mut self, ui: &mut Ui, ed: &mut MacroEditor, bx: Rect, launch: bool, obs: bool, cat: Option<usize>, _sec: usize) -> Option<Option<MacroStep>> {
        let sections: Vec<catalog::Section> = if launch || obs { Vec::new() } else { catalog::system().into_iter().chain(catalog::commands()).collect() };
        let placeholder = if obs {
            if cat.is_some() { "Select action".to_string() } else { "Select integration".to_string() }
        } else if launch {
            "Launch Application".to_string()
        } else {
            match cat {
                Some(c) => format!("Select {} command", sections.get(c).map_or("", |s| s.title.as_str())),
                None => "Select system command".to_string(),
            }
        };
        let border = Stroke::new(1.0, pal().ink(102));
        ui.painter().rect_stroke(bx, CornerRadius::same(4), border, StrokeKind::Inside);
        text(ui, pos2(bx.left() + 12.0, bx.center().y), Align2::LEFT_CENTER, &placeholder, Ts::new(Fam::Reg, 12.0, pal().text).ls(0.0));
        chevron(ui.painter(), pos2(bx.right() - 16.0, bx.center().y), 4.0, false, Stroke::new(1.5, pal().text));
        let mut rows: Vec<(String, Option<MacroStep>, Option<usize>, bool)> = Vec::new();
        if obs {
            if cat.is_none() {
                rows.push(("OBS".into(), None, Some(0), false));
            } else {
                let cfg = self.cfg();
                for a in ObsAction::SIMPLE.iter().chain(cfg.obs_actions.iter()) {
                    rows.push((catalog::obs_name(a), Some(MacroStep::run(Assignment::Obs(a.clone()), 0)), None, false));
                }
            }
        } else if launch {
            let cfg = self.cfg();
            for c in super::assign::launch_cards(&cfg) {
                rows.push((format!("Launch {}", c.name), Some(MacroStep::run(c.assignment.clone(), 0)), None, false));
            }
            rows.push(("CREATE NEW".into(), None, None, true));
        } else if let Some(ci) = cat {
            if let Some(s) = sections.get(ci) {
                for c in s.cards.iter().filter(|c| step_allowed(&c.assignment)) {
                    rows.push((c.name.clone(), Some(MacroStep::run(c.assignment.clone(), 0)), None, false));
                }
            }
        } else {
            for (i, s) in sections.iter().enumerate() {
                if s.cards.iter().any(|c| step_allowed(&c.assignment)) {
                    rows.push((s.title.clone(), None, Some(i), false));
                }
            }
        }
        let list_h = (rows.len() as f32 * 24.0 + 24.0).min(200.0);
        let list = Rect::from_min_size(pos2(bx.left(), bx.bottom() + 8.0), vec2(bx.width(), list_h));
        ui.painter().rect_filled(list, CornerRadius::same(4), pal().bg);
        ui.painter().rect_stroke(list, CornerRadius::same(4), border, StrokeKind::Inside);
        let inner = list.shrink(12.0);
        let max_scroll = (rows.len() as f32 * 24.0 - inner.height()).max(0.0);
        if ui.rect_contains_pointer(list) {
            ed.list_scroll -= ui.input(|i| i.smooth_scroll_delta.y);
        }
        ed.list_scroll = ed.list_scroll.clamp(0.0, max_scroll);
        let mut result: Option<Option<MacroStep>> = None;
        let mut next_cat: Option<usize> = None;
        let mut sub = ui.new_child(UiBuilder::new().max_rect(inner));
        sub.set_clip_rect(inner.intersect(ui.clip_rect()));
        for (i, (label, step, c, create)) in rows.iter().enumerate() {
            let rr = Rect::from_min_size(pos2(inner.left(), inner.top() + i as f32 * 24.0 - ed.list_scroll), vec2(inner.width(), 24.0));
            let r = click(&sub, rr, ("choose", i));
            let col = if r.hovered() { pal().text } else { pal().text.gamma_multiply(0.8) };
            if *create {
                sub.painter().line_segment([rr.left_top(), rr.right_top()], Stroke::new(1.0, pal().ink(51)));
                text(&sub, rr.center(), Align2::CENTER_CENTER, label, Ts::new(Fam::Reg, 12.0, col).ls(0.0));
            } else {
                text(&sub, pos2(rr.left(), rr.center().y), Align2::LEFT_CENTER, label, Ts::new(Fam::Reg, 12.0, col).ls(0.0));
            }
            if r.clicked() {
                if let Some(s) = step {
                    result = Some(Some(s.clone()));
                } else if let Some(c) = c {
                    next_cat = Some(*c);
                } else if *create && self.shot.is_none() && self.script_none() {
                    ed.mode = None;
                    if let Some(path) = crate::win::pick_exe(crate::win::main_window_handle()) {
                        let stem = std::path::Path::new(&path).file_stem().and_then(|s| s.to_str()).unwrap_or("App").to_string();
                        result = Some(Some(MacroStep::run(Assignment::Launch { name: stem, path }, 0)));
                    } else {
                        result = Some(None);
                    }
                }
            }
        }
        if let Some(c) = next_cat
            && let Some(Mode::Choose { cat, .. }) = &mut ed.mode
        {
            *cat = Some(c);
            ed.list_scroll = 0.0;
        }
        result
    }

    fn script_none(&self) -> bool {
        !self.scripted()
    }

    /// Draw one item in its cell; returns the rectangle that reacts to the pointer.
    fn draw_item(&self, ui: &Ui, item: &Item, x: f32, top: f32, selected: bool, recording: bool) -> Rect {
        let cy = top + 40.0;
        let p = ui.painter();
        let sel_fill = pal().tint(pal().primary, 0.2);
        let (ink, arrow) = if selected { (pal().primary, pal().primary) } else { (pal().text, pal().text) };
        match &item.kind {
            ItemKind::Key { .. } | ItemKind::Fold(_) => {
                let (label, down) = match &item.kind {
                    ItemKind::Key { vk, down } => (catalog::vk_name(*vk), Some(*down)),
                    ItemKind::Fold(l) => (l.clone(), None),
                    _ => unreachable!(),
                };
                let w = chip_w(ui, item, false);
                let rect = Rect::from_min_size(pos2(x, cy - 18.0), vec2(w, 36.0));
                if selected {
                    p.rect_filled(rect, CornerRadius::same(4), sel_fill);
                    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, pal().primary), StrokeKind::Inside);
                } else if recording {
                    p.rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, pal().gshift), StrokeKind::Inside);
                } else {
                    p.rect_filled(rect, CornerRadius::same(4), pal().tint(pal().gshift, 0.4));
                }
                text(ui, rect.center(), Align2::CENTER_CENTER, &label, Ts::new(Fam::Reg, 16.0, pal().text).ls(0.0));
                if let Some(d) = down {
                    if d {
                        tri(p, rect.center().x, top + 72.0, false, arrow);
                    } else {
                        tri(p, rect.center().x, top + 8.0, true, arrow);
                    }
                }
                rect.expand2(vec2(0.0, 14.0))
            }
            ItemKind::Mouse { vk, down } => {
                let rect = Rect::from_min_size(pos2(x, top + 14.0), vec2(30.0, 46.0));
                mouse_icon(p, rect.min, *vk, ink);
                if matches!(*vk, 0x05 | 0x06) {
                    text(ui, rect.center() + vec2(0.0, 11.0), Align2::CENTER_CENTER, if *vk == 0x05 { "4" } else { "5" }, Ts::new(Fam::Reg, 14.0, ink).ls(0.0));
                }
                if *down {
                    tri(p, rect.center().x, top + 74.0, false, arrow);
                } else {
                    tri(p, rect.center().x, top, true, arrow);
                }
                rect.expand2(vec2(0.0, 14.0))
            }
            ItemKind::Text(t) => {
                let w = chip_w(ui, item, false);
                let rect = Rect::from_min_size(pos2(x, cy - 18.0), vec2(w, 36.0));
                self.pill(ui, rect, if selected { sel_fill } else { pal().tint(pal().amber, 0.4) }, selected);
                let s = fit(ui, &format!("\u{201c}{}\u{201d}", t.replace('\n', " ")), Ts::new(Fam::Reg, 16.0, pal().text).ls(0.0), w - 24.0);
                text(ui, rect.center(), Align2::CENTER_CENTER, &s, Ts::new(Fam::Reg, 16.0, pal().text).ls(0.0));
                rect
            }
            ItemKind::Sys(l) | ItemKind::Launch(l) | ItemKind::Obs(l) => {
                let w = chip_w(ui, item, false);
                let rect = Rect::from_min_size(pos2(x, cy - 19.5), vec2(w, 39.0));
                let base = match item.kind {
                    ItemKind::Launch(_) => pal().ink(102),
                    ItemKind::Obs(_) => pal().tint(pal().orange(), 0.4),
                    _ => pal().ink(51),
                };
                self.pill(ui, rect, if selected { sel_fill } else { base }, selected);
                text(ui, rect.center(), Align2::CENTER_CENTER, l, Ts::new(Fam::Reg, 14.0, pal().text).ls(0.0));
                rect
            }
            ItemKind::Delay { ms, jitter, .. } => {
                let w = chip_w(ui, item, false);
                let (v, u) = fmt_delay(*ms);
                let unit = if *jitter > 0 { format!("{u}±{jitter}") } else { u.to_owned() };
                let rect = Rect::from_min_size(pos2(x, cy - 24.0), vec2(w, 46.0));
                text(ui, pos2(rect.center().x, cy - 12.0), Align2::CENTER_CENTER, &v, Ts::new(Fam::Reg, 21.0, ink).ls(0.0));
                p.line_segment([pos2(rect.left(), cy + 1.0), pos2(rect.right(), cy + 1.0)], Stroke::new(1.0, pal().text));
                text(ui, pos2(rect.center().x, cy + 11.0), Align2::CENTER_CENTER, &unit, Ts::new(Fam::Light, 14.0, ink).ls(0.0));
                rect
            }
        }
    }

    fn pill(&self, ui: &Ui, rect: Rect, fill: Color32, selected: bool) {
        ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
        if selected {
            ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, pal().primary), StrokeKind::Inside);
        }
    }
}
