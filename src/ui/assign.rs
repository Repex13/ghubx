//! Assignments: tabs COMMANDS / KEYS / ACTIONS / MACROS / SYSTEM, the
//! device render with G HUB's callouts, VIEW 1/2 and the G-SHIFT layer.

use super::device::{Placed, draw_render};
use super::theme::*;
use super::{DevTab, GhubApp, Overlay};
use crate::catalog::{self, Card, LabelStyle, Section};
use crate::device::SLOTS;
use crate::engine::EngineMsg;
use crate::profile::*;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};
use std::collections::HashSet;
use std::time::Instant;

const TABS: [&str; 5] = ["COMMANDS", "KEYS", "ACTIONS", "MACROS", "SYSTEM"];

#[derive(Default)]
pub struct AsgState {
    pub tab: usize,
    pub search: String,
    pub side: bool,
    pub gshift: bool,
    pub selected: Option<usize>,
    pub collapsed: HashSet<String>,
    pub mods: u8,
    pub editor: Option<MacroEditor>,
    pub last_tab: Option<DevTab>,
    pub launch_path: String,
    pub p180_sens_text: String,
    pub p180_counts_text: String,
    pub game_sens: Option<(usize, Option<crate::games::GameSens>)>,
}

pub struct MacroEditor {
    pub id: Option<String>,
    pub name: String,
    pub kind: MacroKind,
    pub steps: Vec<MacroStep>,
    pub recording: bool,
    pub record_delays: bool,
    last: Option<Instant>,
    mods: egui::Modifiers,
    capture_key: bool,
    click_row: bool,
    text_open: bool,
    text_focus: bool,
    text_buf: String,
    edit_delay: Option<(usize, String)>,
    test_at: Option<Instant>,
    list: Rect,
}

impl MacroEditor {
    pub fn new() -> Self {
        Self { id: None, name: String::new(), kind: MacroKind::NoRepeat, steps: vec![], recording: false,
            record_delays: true,
            last: None,
            mods: Default::default(),
            capture_key: false,
            click_row: false,
            text_open: false,
            text_focus: false,
            text_buf: String::new(),
            edit_delay: None,
            test_at: None,
            list: Rect::NOTHING,
        }
    }
    pub fn from(m: &Macro) -> Self {
        Self { id: Some(m.id.clone()), name: m.name.clone(), kind: m.kind, steps: m.steps.clone(), ..Self::new() }
    }
    pub fn set_delay(&mut self, i: usize, ms: u32) {
        if let Some(s) = self.steps.get_mut(i) {
            s.delay_ms = ms.min(60_000);
        }
    }

    pub fn move_step(&mut self, i: usize, up: bool) {
        let j = if up { i.checked_sub(1) } else { Some(i + 1) };
        if let Some(j) = j
            && i < self.steps.len()
            && j < self.steps.len()
        {
            self.steps.swap(i, j);
        }
    }

    pub fn add_key(&mut self, vk: u16) {
        let first = if self.steps.is_empty() { 0 } else { 50 };
        self.steps.push(MacroStep { vk, down: true, delay_ms: first });
        self.steps.push(MacroStep { vk, down: false, delay_ms: 50 });
    }

    /// `btn` 0..=4 = left, right, middle, back, forward.
    pub fn add_click(&mut self, btn: u8) {
        if let Some(&vk) = [0x01u16, 0x02, 0x04, 0x05, 0x06].get(btn as usize) {
            self.add_key(vk);
        }
    }

    pub fn add_delay(&mut self) {
        self.steps.push(MacroStep { vk: 0, down: false, delay_ms: 50 });
    }

    /// Type text as key steps; `map` gives a character's VK and whether Shift is
    /// needed. Returns how many characters could not be typed.
    pub fn add_text(&mut self, text: &str, map: impl Fn(char) -> Option<(u16, bool)>) -> usize {
        let mut skipped = 0;
        for ch in text.chars() {
            let Some((vk, shift)) = map(ch) else {
                skipped += 1;
                continue;
            };
            let mut step = |vk: u16, down: bool| self.steps.push(MacroStep { vk, down, delay_ms: 10 });
            if shift {
                step(0xA0, true);
            }
            step(vk, true);
            step(vk, false);
            if shift {
                step(0xA0, false);
            }
        }
        skipped
    }

    fn push(&mut self, vk: u16, down: bool) {
        let delay = match (self.record_delays, self.last) {
            (true, Some(t)) => t.elapsed().as_millis().min(10_000) as u32,
            (false, Some(_)) => 50,
            _ => 0,
        };
        self.last = Some(Instant::now());
        self.steps.push(MacroStep { vk, down, delay_ms: delay });
    }
}

/// egui key -> Windows virtual-key code.
pub fn egui_vk(k: egui::Key) -> Option<u16> {
    use egui::Key::*;
    let name = k.name();
    if name.len() == 1 {
        let c = name.chars().next()?.to_ascii_uppercase();
        if c.is_ascii_alphanumeric() {
            return Some(c as u16);
        }
    }
    Some(match k {
        F1 => 0x70, F2 => 0x71, F3 => 0x72, F4 => 0x73, F5 => 0x74, F6 => 0x75, F7 => 0x76, F8 => 0x77,
        F9 => 0x78, F10 => 0x79, F11 => 0x7A, F12 => 0x7B, F13 => 0x7C, F14 => 0x7D, F15 => 0x7E, F16 => 0x7F,
        F17 => 0x80, F18 => 0x81, F19 => 0x82, F20 => 0x83,
        Space => 0x20, Enter => 0x0D, Escape => 0x1B, Tab => 0x09, Backspace => 0x08, Delete => 0x2E,
        Insert => 0x2D, Home => 0x24, End => 0x23, PageUp => 0x21, PageDown => 0x22,
        ArrowLeft => 0x25, ArrowUp => 0x26, ArrowRight => 0x27, ArrowDown => 0x28,
        Minus => 0xBD, Equals | Plus => 0xBB, OpenBracket => 0xDB, CloseBracket => 0xDD, Backslash => 0xDC,
        Semicolon => 0xBA, Quote => 0xDE, Comma => 0xBC, Period => 0xBE, Slash => 0xBF, Backtick => 0xC0,
        _ => return None,
    })
}

/// The edited app's G HUB game commands (one section per category).
fn game_sections(cfg: &Config) -> Vec<Section> {
    let Some(game) = cfg.current().0.ghub_id.as_deref().and_then(|id| crate::games::db()?.by_id(id)) else { return Vec::new() };
    game.categories
        .iter()
        .map(|(cat, tag)| Section {
            title: cat.clone(),
            tag: *tag,
            cards: game
                .commands
                .iter()
                .filter(|c| &c.category == cat)
                .filter_map(|c| {
                    let combo = crate::games::key_combo(&c.keys)?;
                    Some(Card { name: catalog::combo_name(combo), detail: c.name.clone(), assignment: Assignment::Shortcut { name: c.name.clone(), combo } })
                })
                .collect(),
        })
        .filter(|s| !s.cards.is_empty())
        .collect()
}

enum ListItem {
    Header(String, Option<[u8; 3]>),
    Card(Card),
}

impl GhubApp {
    pub(crate) fn assignments(&mut self, ui: &mut Ui, panel: Rect, content: Rect) {
        text(ui, pos2(panel.left() + 16.0, panel.top() + 17.0), Align2::LEFT_TOP, "Assignments", t_panel_title());
        let x0 = panel.left() + 16.0;
        if let Some(i) = tabs(ui, pos2(x0, panel.top() + 74.0), &TABS[..3], self.asg.tab.min(9), 16.0, "asg-tabs1") {
            self.asg.tab = i;
            self.asg.search.clear();
        }
        let sel2 = if self.asg.tab >= 3 { self.asg.tab - 3 } else { 9 };
        if let Some(i) = tabs(ui, pos2(x0, panel.top() + 104.0), &TABS[3..], sel2, 16.0, "asg-tabs2") {
            self.asg.tab = 3 + i;
            self.asg.search.clear();
        }
        if self.asg.tab == 3 && self.asg.editor.is_some() {
            self.macro_editor(ui, panel);
        } else {
            let hint = match self.asg.tab {
                0 => "Drag a command onto a target to assign it to this device.",
                1 => "Drag a key onto a target to assign it to this device.",
                2 => "Drag actions onto buttons to assign them in this profile.",
                3 => "Drag a macro onto a target to assign it to this device.",
                _ => "Drag a system control onto a target to assign it to this device.",
            };
            para(ui, pos2(x0, panel.top() + 146.0), 278.0, hint, Ts::new(Fam::Reg, 12.0, pal().text_high).ls(0.6));
            let placeholder = match self.asg.tab {
                0 => "Search for a command",
                1 => "Search for a key",
                2 => "Search for an action",
                3 => "Search for a macro",
                _ => "Search for a system control",
            };
            let sy = panel.top() + 201.0;
            search_field(ui, Rect::from_min_max(pos2(x0, sy - 13.0), pos2(panel.right() - 16.0, sy + 13.0)), &mut self.asg.search, placeholder);
            let list = Rect::from_min_max(pos2(x0, sy + 25.0), pos2(panel.right() - 12.0, panel.bottom() - 8.0));
            self.card_list(ui, list);
        }
        self.device_callouts(ui, content);
        self.drag_ghost(ui);
    }

    fn items(&self, cfg: &Config) -> Vec<ListItem> {
        let q = self.asg.search.to_lowercase();
        let keep = |c: &Card| q.is_empty() || c.name.to_lowercase().contains(&q) || c.detail.to_lowercase().contains(&q);
        let sections: Vec<Section> = match self.asg.tab {
            0 => {
                let mut v = game_sections(cfg);
                v.extend(catalog::commands());
                v
            }
            1 => vec![Section {
                title: String::new(),
                tag: None,
                cards: catalog::keys()
                    .into_iter()
                    .map(|vk| {
                        let combo = KeyCombo { vk, mods: self.asg.mods };
                        Card { name: catalog::combo_name(combo), detail: String::new(), assignment: Assignment::Key(combo) }
                    })
                    .collect(),
            }],
            2 => catalog::actions(),
            3 => vec![Section {
                title: String::new(),
                tag: None,
                cards: cfg.current().0.macros.iter().map(|m| Card { name: m.name.clone(), detail: String::new(), assignment: Assignment::Macro(m.id.clone()) }).collect(),
            }],
            _ => {
                let mut v = vec![Section {
                    title: "Launch Application".into(),
                    tag: None,
                    cards: launch_cards(cfg),
                }];
                v.extend(catalog::system());
                v
            }
        };
        let mut out = Vec::new();
        for s in sections {
            let cards: Vec<Card> = s.cards.into_iter().filter(keep).collect();
            if !s.title.is_empty() {
                out.push(ListItem::Header(s.title.clone(), s.tag));
                if self.asg.collapsed.contains(&s.title) && q.is_empty() {
                    continue;
                }
            }
            out.extend(cards.into_iter().map(ListItem::Card));
        }
        out
    }

    fn card_list(&mut self, ui: &mut Ui, list: Rect) {
        let cfg = self.cfg();
        let items = self.items(&cfg);
        let prof = cfg.current().1.clone();
        let macros = cfg.current().0.macros.clone();
        let gs = self.asg.gshift;
        // which slots currently hold an assignment (for the right column)
        let slots_for = |a: &Assignment| -> String {
            (0..SLOTS)
                .filter(|&s| &prof.effective(s, gs) == a || prof.layer(gs).get(s) == Some(a))
                .map(catalog::slot_name)
                .collect::<Vec<_>>()
                .join(", ")
        };
        let mut picked: Option<Assignment> = None;
        let mut toggle_section: Option<String> = None;
        let mut edit_macro: Option<String> = None;
        let mut delete_macro: Option<String> = None;
        let mut add_launch = false;
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        let tab = self.asg.tab;
        // tab-specific header blocks
        if tab == 1 {
            self.keys_header(&mut child);
        } else if tab == 3 && dark_button(&child, Rect::from_min_size(child.cursor().min, vec2(278.0, 32.0)), "CREATE NEW MACRO", "new-macro", true).clicked() {
            self.asg.editor = Some(MacroEditor::new());
        }
        if tab == 3 {
            child.add_space(44.0);
        }
        let mut launch_path = std::mem::take(&mut self.asg.launch_path);
        egui::ScrollArea::vertical().id_salt(("asg-list", tab)).auto_shrink([false, false]).show(&mut child, |ui| {
            let w = 270.0;
            for (i, it) in items.iter().enumerate() {
                match it {
                    ListItem::Header(title, tag) => {
                        let (rect, r) = ui.allocate_exact_size(vec2(w, 42.0), Sense::click());
                        let open = !self.asg.collapsed.contains(title.as_str());
                        let tx = rect.left();
                        let _ = tag;
                        text(ui, pos2(tx, rect.center().y), Align2::LEFT_CENTER, title, Ts::new(Fam::Bold, 14.0, pal().text).ls(0.3));
                        chevron(ui.painter(), pos2(rect.right() - 12.0, rect.center().y), 4.5, open, Stroke::new(1.6, pal().text));
                        if r.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                            toggle_section = Some(title.clone());
                        }
                        if title == "Launch Application" && open {
                            let (row, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
                            let te = egui::TextEdit::singleline(&mut launch_path)
                                .hint_text("Path to an .exe")
                                .frame(egui::Frame::NONE)
                                .font(Ts::new(Fam::Reg, 12.0, pal().text).font())
                                .text_color(pal().text);
                            ui.put(Rect::from_min_max(row.min + vec2(4.0, 6.0), pos2(row.right() - 104.0, row.bottom() - 6.0)), te);
                            ui.painter().line_segment([pos2(row.left(), row.bottom() - 4.0), pos2(row.right() - 104.0, row.bottom() - 4.0)], Stroke::new(1.0, pal().outline_hi));
                            let br = Rect::from_min_size(pos2(row.right() - 96.0, row.top() + 4.0), vec2(96.0, 26.0));
                            let r = click(ui, br, "add-app");
                            plus_text(ui, br, "ADD APPLICATION", r.hovered());
                            if r.clicked() {
                                add_launch = true;
                            }
                        }
                    }
                    ListItem::Card(c) => {
                        let left_w = if c.detail.is_empty() { 190.0 } else { 112.0 };
                        let lt = if tab == 4 || tab == 1 { Ts::new(Fam::Light, 12.0, pal().muted) } else { Ts::new(Fam::Reg, 12.0, pal().text).ls(0.6) };
                        let gl = galley(ui, &c.name, lt, left_w);
                        let gd = galley(ui, &c.detail, Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.6), 140.0);
                        let h = (gl.size().y.max(gd.size().y) + 10.0).max(32.0);
                        let (rect, r) = ui.allocate_exact_size(vec2(w, h), Sense::click_and_drag());
                        let r = r.on_hover_cursor(egui::CursorIcon::Grab);
                        r.dnd_set_drag_payload(c.assignment.clone());
                        if r.hovered() {
                            ui.painter().rect_filled(rect, CornerRadius::same(3), pal().ink(10));
                        }
                        let mut lx = rect.left() + 2.0;
                        if matches!(c.assignment, Assignment::Macro(_)) {
                            macro_badge(ui, Rect::from_min_size(pos2(lx, rect.center().y - 7.0), vec2(14.0, 14.0)));
                            lx += 22.0;
                        }
                        let lcol = if r.hovered() { pal().text } else { lt.color };
                        ui.painter().galley(pos2(lx, rect.center().y - gl.size().y / 2.0), gl, lcol);
                        if !c.detail.is_empty() {
                            ui.painter().galley(pos2(rect.left() + 121.0, rect.center().y - gd.size().y / 2.0), gd, pal().muted);
                        }
                        let assigned = slots_for(&c.assignment);
                        if !assigned.is_empty() && c.detail.is_empty() {
                            text(ui, pos2(rect.right() - 30.0, rect.center().y), Align2::RIGHT_CENTER, &assigned, Ts::new(Fam::Reg, 12.0, pal().text).ls(0.3));
                        }
                        if let Assignment::Macro(id) = &c.assignment
                            && ui.rect_contains_pointer(rect)
                        {
                                let er = Rect::from_center_size(pos2(rect.right() - 60.0, rect.center().y), vec2(36.0, 20.0));
                                let xr = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(20.0, 20.0));
                                let e = click(ui, er, ("edit", i));
                                text(ui, er.center(), Align2::CENTER_CENTER, "EDIT", Ts::new(Fam::Bold, 10.0, if e.hovered() { pal().primary } else { pal().text }));
                                let x = click(ui, xr, ("del", i));
                                caption_glyph(ui.painter(), xr.center(), 2, if x.hovered() { pal().error } else { pal().muted });
                                if e.clicked() {
                                    edit_macro = Some(id.clone());
                                } else if x.clicked() {
                                    delete_macro = Some(id.clone());
                                }
                        }
                        ui.painter().line_segment([pos2(rect.left(), rect.bottom() - 0.5), pos2(rect.right(), rect.bottom() - 0.5)], Stroke::new(1.0, pal().row_line));
                        if r.clicked() && edit_macro.is_none() && delete_macro.is_none() {
                            picked = Some(c.assignment.clone());
                        }
                    }
                }
            }
            if tab == 2 {
                ui.add_space(8.0);
                self.p180_block(ui);
            }
            if tab == 3 && macros.is_empty() {
                ui.add_space(8.0);
                let (r, _) = ui.allocate_exact_size(vec2(w, 60.0), Sense::hover());
                para(ui, r.min, w, "No macros yet. Create one, then drag it onto a button.", Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.6));
            }
            ui.add_space(12.0);
        });
        if add_launch && !launch_path.trim().is_empty() {
            let path = launch_path.trim().trim_matches('"').to_string();
            let name = std::path::Path::new(&path).file_stem().map(|s| s.to_string_lossy().into_owned()).unwrap_or_else(|| path.clone());
            picked = Some(Assignment::Launch { name, path });
            launch_path.clear();
        }
        self.asg.launch_path = launch_path;
        if let Some(t) = toggle_section
            && !self.asg.collapsed.remove(&t)
        {
            self.asg.collapsed.insert(t);
        }
        if let Some(id) = edit_macro
            && let Some(m) = macros.iter().find(|m| m.id == id)
        {
            self.asg.editor = Some(MacroEditor::from(m));
        }
        if let Some(id) = delete_macro {
            self.edit(|c| {
                let ca = c.current_app;
                c.apps[ca].macros.retain(|m| m.id != id);
                for p in &mut c.apps[ca].profiles {
                    for a in p.base.iter_mut().chain(p.gshift.iter_mut()) {
                        if *a == Assignment::Macro(id.clone()) {
                            *a = Assignment::Default;
                        }
                    }
                }
            });
            self.say("Macro deleted");
        }
        if let Some(a) = picked {
            match self.asg.selected {
                Some(slot) => self.assign(slot, a),
                None if matches!(a, Assignment::Launch { .. }) => {
                    self.say("Select a button on the mouse, then add the application again");
                }
                None => self.say("Select a button on the mouse first — or drag this onto it"),
            }
        }
    }

    fn keys_header(&mut self, ui: &mut Ui) {
        let top = ui.cursor().min;
        let mut x = top.x;
        for (bit, name) in [(MOD_CTRL, "CTRL"), (MOD_SHIFT, "SHIFT"), (MOD_ALT, "ALT"), (MOD_WIN, "WIN")] {
            let on = self.asg.mods & bit != 0;
            let g = galley(ui, name, Ts::new(Fam::Bold, 11.0, pal().text), f32::INFINITY);
            let r = Rect::from_min_size(pos2(x, top.y + 2.0), vec2(g.size().x + 20.0, 24.0));
            if pill(ui, r, name, on, ("mod", bit)).clicked() {
                self.asg.mods ^= bit;
            }
            x = r.right() + 8.0;
        }
        ui.add_space(36.0);
    }

    /// The edited app's game sensitivity, looked up once per app (and again after a click).
    /// `--shot` never reads real files.
    fn game_sens_for(&mut self, cfg: &Config) -> Option<crate::games::GameSens> {
        let idx = cfg.current_app;
        if self.asg.game_sens.as_ref().is_none_or(|(i, _)| *i != idx) {
            let app = cfg.current().0;
            let found = if self.shot.is_some() {
                (app.name == "Counter-Strike 2").then_some(crate::games::GameSens { game: "Counter-Strike 2", preset: 0, sens: 0.51, yaw: 0.022, counts: 16043 })
            } else {
                crate::games::game_sensitivity(app.ghub_id.as_deref(), &app.name)
            };
            self.asg.game_sens = Some((idx, found));
        }
        self.asg.game_sens.as_ref().and_then(|(_, g)| g.clone())
    }

    fn p180_block(&mut self, ui: &mut Ui) {
        let top = ui.cursor().min;
        let cfg = self.cfg();
        let e = cfg.extras.clone();
        let per_profile = cfg.current().1.turn.is_some();
        let cal = turn_for(cfg.current().1, &e);
        let game_sens = self.game_sens_for(&cfg);
        let extra = if game_sens.is_some() { 16.0 } else { 0.0 };
        let box_r = Rect::from_min_size(top, vec2(278.0, 366.0 + extra));
        ui.painter().rect_filled(box_r, CornerRadius::same(8), pal().surface_hi);
        let x = box_r.left() + 12.0;
        text(ui, pos2(x, box_r.top() + 12.0), Align2::LEFT_TOP, "PERFECT 180", Ts::new(Fam::Bold, 12.0, pal().text));
        para(ui, pos2(x, box_r.top() + 30.0), 254.0, "One press turns you 180°. Calibrate it from your in-game sensitivity.", Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4));

        // game presets (3 x 2); a count that doesn't match the preset maths shows as CUSTOM
        let mut game = cal.game.min(P180_GAMES.len() - 1);
        if game != P180_CUSTOM && p180_counts_for(P180_GAMES[game].1, cal.sens) != Some(cal.counts) {
            game = P180_CUSTOM;
        }
        let shown_game = game;
        let mut recalc = false;
        for (slot, &i) in P180_ORDER.iter().enumerate() {
            let r = Rect::from_min_size(pos2(x + (slot % 3) as f32 * 86.0, box_r.top() + 66.0 + (slot / 3) as f32 * 26.0), vec2(82.0, 22.0));
            if pill_sized(ui, r, P180_GAMES[i].0, game == i, 11.0, ("p180-game", i)).clicked() && game != i {
                game = i;
                recalc = true;
            }
        }
        if let Some(gs) = &game_sens {
            let matches = cal.game == gs.preset && (cal.sens - gs.sens).abs() < 1e-4 && cal.counts == gs.counts;
            let line = Rect::from_min_size(pos2(x, box_r.top() + 117.0), vec2(254.0, 16.0));
            let resp = (!matches).then(|| click(ui, line, "p180-use-game-sens"));
            let (hovered, clicked) = resp.map_or((false, false), |r| (r.hovered(), r.clicked()));
            let (verb, color) = if matches { ("MATCHES", pal().muted) } else { ("USE", if hovered { pal().primary_hover } else { pal().primary }) };
            let label = format!("{verb} MY {} SENSITIVITY ({})", gs.game.to_uppercase(), gs.sens);
            text(ui, pos2(x, line.center().y), Align2::LEFT_CENTER, &label, Ts::new(Fam::Bold, 10.0, color).ls(0.2));
            if clicked {
                let new = TurnCal { counts: gs.counts, game: gs.preset, sens: gs.sens, angle: cal.angle };
                self.edit(|c| c.current_profile_mut().turn = Some(new));
                self.asg.game_sens = None;
            }
        }
        let yaw = P180_GAMES[game].1;
        let row = |i: usize| box_r.top() + 136.0 + extra + i as f32 * 28.0;

        // in-game sensitivity
        text(ui, pos2(x, row(0)), Align2::LEFT_CENTER, "IN-GAME SENS", t_small_label().color(if yaw > 0.0 { pal().label } else { pal().disabled }));
        let sens_r = Rect::from_min_size(pos2(x + 108.0, row(0) - 11.0), vec2(80.0, 22.0));
        let mut sens_val = cal.sens;
        if yaw > 0.0 {
            if let Some(v) = number_field(ui, sens_r, &mut self.asg.p180_sens_text, &format!("{}", cal.sens), "p180-sens")
                && v > 0.0
            {
                sens_val = v as f32;
                recalc = true;
            }
        } else {
            ui.painter().rect_stroke(sens_r, CornerRadius::same(4), Stroke::new(1.0, pal().ink(20)), StrokeKind::Inside);
        }

        // counts: exact field + log slider (100 .. 100 000)
        text(ui, pos2(x, row(1)), Align2::LEFT_CENTER, "COUNTS", t_small_label());
        let counts_r = Rect::from_min_size(pos2(x + 108.0, row(1) - 11.0), vec2(80.0, 22.0));
        let mut counts = cal.counts;
        let mut manual = false;
        if let Some(v) = number_field(ui, counts_r, &mut self.asg.p180_counts_text, &cal.counts.to_string(), "p180-counts")
            && v >= 1.0
        {
            counts = v.round() as i32;
            manual = true;
        }
        let (lo, hi) = (100f32.ln(), 100_000f32.ln());
        let mut t = ((cal.counts.max(100) as f32).ln() - lo) / (hi - lo);
        let sr = Rect::from_min_size(pos2(x, row(2) - 6.0), vec2(254.0, 12.0));
        if slider(ui, sr, &mut t, 0.0..=1.0, "p180-c-log") {
            let v = (lo + t * (hi - lo)).exp();
            counts = if v < 1000.0 { (v / 10.0).round() * 10.0 } else { (v / 50.0).round() * 50.0 } as i32;
            manual = true;
        }

        // angle
        text(ui, pos2(x, row(3)), Align2::LEFT_CENTER, "ANGLE", t_small_label());
        let mut angle = cal.angle as f32;
        let angle_r = Rect::from_min_size(pos2(x + 60.0, row(3) - 6.0), vec2(140.0, 12.0));
        let angle_changed = slider(ui, angle_r, &mut angle, 45.0..=360.0, "p180-angle");
        let angle_deg = ((angle / 5.0).round() * 5.0).clamp(45.0, 360.0) as u16;
        text(ui, pos2(box_r.right() - 12.0, row(3)), Align2::RIGHT_CENTER, &format!("{angle_deg}°"), t_body_bold());

        // speed
        text(ui, pos2(x, row(4)), Align2::LEFT_CENTER, "SPEED", t_small_label());
        let mut ms = e.p180_ms as f32;
        let speed_r = Rect::from_min_size(pos2(x + 60.0, row(4) - 6.0), vec2(140.0, 12.0));
        let ms_changed = slider(ui, speed_r, &mut ms, 0.0..=200.0, "p180-ms");
        text(ui, pos2(box_r.right() - 12.0, row(4)), Align2::RIGHT_CENTER, &format!("{}ms", ms.round() as u16), t_body_bold());

        if recalc && let Some(c) = p180_counts_for(yaw, sens_val) {
            counts = c;
        }
        // a hand-set count no longer matches the preset maths
        if manual && p180_counts_for(yaw, sens_val) != Some(counts) {
            game = P180_CUSTOM;
        }
        if recalc || manual || ms_changed || angle_changed || game != shown_game {
            let new = TurnCal { counts, game, sens: sens_val, angle: angle_deg };
            let ms = ms.round() as u16;
            self.edit(|c| {
                c.extras.p180_ms = ms;
                if c.current_profile_mut().turn.is_some() {
                    c.current_profile_mut().turn = Some(new);
                } else {
                    c.extras.p180_game = new.game;
                    c.extras.p180_sens = new.sens;
                    c.extras.p180_counts = new.counts;
                    c.extras.p180_angle = new.angle;
                }
            });
        }

        let br = Rect::from_min_size(pos2(x, row(4) + 20.0), vec2(254.0, 26.0));
        let label = match self.p180_at {
            Some(t) => format!("TURNING IN {}…", 3 - t.elapsed().as_secs().min(2)),
            None => "TEST IN 3 SECONDS".into(),
        };
        if dark_button(ui, br, &label, "p180-test", self.p180_at.is_none()).clicked() && self.p180_at.is_none() {
            self.p180_at = Some(Instant::now());
        }
        if let Some(t) = self.p180_at {
            ui.ctx().request_repaint();
            if t.elapsed().as_secs_f32() >= 3.0 {
                self.p180_at = None;
                let _ = self.eng.send(EngineMsg::TestPerfect180 { counts: turn_counts(counts, angle_deg) });
            }
        }
        if checkbox(ui, pos2(x, br.bottom() + 12.0), "USE FOR THIS PROFILE ONLY", per_profile, true, "p180-per-profile").clicked() {
            let cur = TurnCal { counts, game, sens: sens_val, angle: angle_deg };
            self.edit(|c| c.current_profile_mut().turn = if per_profile { None } else { Some(cur) });
        }
        para(ui, pos2(x, br.bottom() + 38.0), 254.0, "Some games ignore injected input or don't allow turn binds in competitive play.", Ts::new(Fam::Reg, 10.0, pal().muted).ls(0.3));
        ui.add_space(378.0 + extra);
    }

    pub(crate) fn assign(&mut self, slot: usize, a: Assignment) {
        let gs = self.asg.gshift;
        let cfg = self.cfg();
        let prof = cfg.current().1;
        if gs && prof.effective(slot, false) == Assignment::System(SystemFn::GShift) {
            self.say("This button is your G-Shift button");
            return;
        }
        let (name, _) = catalog::label(&a, slot, &cfg.current().0.macros);
        self.edit(|c| c.current_profile_mut().layer_mut(gs)[slot] = a);
        self.say(format!("{} → {}{}", catalog::slot_name(slot), name, if gs { " (G-Shift)" } else { "" }));
    }

    fn device_callouts(&mut self, ui: &Ui, content: Rect) {
        let win_h = ui.ctx().content_rect().height();
        let view = if self.asg.side { self.meta.side.clone() } else { self.meta.front.clone() };
        let tex = if self.asg.side { self.tex.side.clone() } else { self.tex.front.clone() };
        let top = content.top() + (190.0 - 88.0) / 768.0 * win_h;
        let placed = Placed::new(view.origin, content.center().x, top, win_h * 0.504);
        draw_render(ui, tex.as_ref(), placed, Color32::WHITE);

        let cfg = self.cfg();
        let (app, prof) = cfg.current();
        let gs = self.asg.gshift;
        let mut menu: Option<(usize, Pos2)> = None;
        let mut select: Option<Option<usize>> = None;
        let mut drop: Option<(usize, Assignment)> = None;
        for hs in &view.hotspots {
            let Some(slot) = hs.slot() else { continue };
            let mp = placed.at(hs.marker.x, hs.marker.y);
            let lp = placed.at(hs.label.x, hs.label.y);
            let layer_a = prof.layer(gs).get(slot).cloned().unwrap_or_default();
            let gshift_btn = gs && prof.effective(slot, false) == Assignment::System(SystemFn::GShift);
            let shown = if gshift_btn { Assignment::System(SystemFn::GShift) } else { layer_a.clone() };
            let (label, style) = catalog::label(&shown, slot, &app.macros);
            let side = if hs.label.x < 0.0 { -1 } else if hs.label.x > view.origin.x { 1 } else { 0 };

            // label geometry
            let ts = t_callout();
            let g = galley(ui, &label, ts, f32::INFINITY);
            let extra = match style {
                LabelStyle::Key => 16.0,
                LabelStyle::Macro => 26.0,
                _ => 0.0,
            };
            let w = g.size().x + extra;
            let h = if style == LabelStyle::Key { 26.0 } else { g.size().y };
            let bottom = lp.y - if style == LabelStyle::Key { 6.0 } else { 2.0 };
            let lrect = match side {
                -1 => Rect::from_min_size(pos2(lp.x + 4.0, bottom - h), vec2(w, h)),
                1 => Rect::from_min_size(pos2(lp.x - 2.0 - w, bottom - h), vec2(w, h)),
                _ => Rect::from_min_size(pos2(lp.x - w / 2.0, bottom - h), vec2(w, h)),
            };
            let target = lrect.expand(5.0).union(Rect::from_center_size(mp, vec2(24.0, 24.0)));
            let r_label = click(ui, lrect.expand(5.0), ("callout", slot, self.asg.side));
            let r_mark = click(ui, Rect::from_center_size(mp, vec2(22.0, 22.0)), ("marker", slot, self.asg.side));
            let dnd_hover = r_label.dnd_hover_payload::<Assignment>().is_some() || r_mark.dnd_hover_payload::<Assignment>().is_some();
            for r in [&r_label, &r_mark] {
                if let Some(a) = r.dnd_release_payload::<Assignment>() {
                    drop = Some((slot, (*a).clone()));
                }
                if r.clicked() {
                    select = Some(if self.asg.selected == Some(slot) { None } else { Some(slot) });
                }
                if r.secondary_clicked() {
                    menu = Some((slot, r.interact_pointer_pos().unwrap_or(mp)));
                }
            }
            let _ = target;
            let selected = self.asg.selected == Some(slot);
            let hot = selected || dnd_hover || r_label.hovered() || r_mark.hovered();
            let pressed = self.mask & (1 << slot) != 0;

            // leader line
            let lc = if hot { pal().n(0x8a) } else { pal().leader };
            let st = Stroke::new(1.0, lc);
            let p = ui.painter();
            match side {
                0 => {
                    p.line_segment([pos2(lp.x, lp.y + 2.0), mp], st);
                }
                s => {
                    let dy = (mp.y - lp.y).abs();
                    let ex = if s < 0 { (mp.x - dy).max(lp.x) } else { (mp.x + dy).min(lp.x) };
                    p.line_segment([pos2(lp.x, lp.y), pos2(ex, lp.y)], st);
                    p.line_segment([pos2(ex, lp.y), mp], st);
                    p.circle_filled(pos2(lp.x, lp.y), 1.2, lc);
                }
            }
            // marker
            let ring = if selected || dnd_hover { pal().primary } else if pressed { pal().amber } else { Color32::from_white_alpha(if hot { 120 } else { 70 }) };
            p.circle_filled(mp, 10.0, ring.gamma_multiply(if selected || dnd_hover || pressed { 1.0 } else { 0.55 }));
            p.circle_filled(mp, 7.0, Color32::WHITE);

            // label
            let dim = gs && layer_a == Assignment::Default && !gshift_btn;
            let base_c = match style {
                LabelStyle::Dpi => pal().amber,
                _ => pal().text,
            };
            let color = if selected || dnd_hover { pal().primary } else if dim { pal().muted } else { base_c };
            match style {
                LabelStyle::Key => keycap(ui, lrect, &label, ts.color(color)),
                LabelStyle::Macro => {
                    macro_badge(ui, Rect::from_min_size(pos2(lrect.left(), lrect.bottom() - 18.0), vec2(18.0, 18.0)));
                    ui.painter().galley(pos2(lrect.left() + 26.0, lrect.bottom() - g.size().y), galley(ui, &label, ts.color(color), f32::INFINITY), color);
                }
                _ => {
                    ui.painter().galley(lrect.min, galley(ui, &label, ts.color(color), f32::INFINITY), color);
                }
            }
            if r_label.hovered() || r_mark.hovered() {
                super::device::tooltip(ui, pos2(lrect.right() + 4.0, lrect.center().y), &catalog::slot_name(slot));
            }
        }
        if let Some((slot, a)) = drop {
            self.assign(slot, a);
        } else if let Some(s) = select {
            self.asg.selected = s;
        }
        if let Some((slot, pos)) = menu {
            self.asg.selected = Some(slot);
            self.open_overlay(ui.ctx(), Overlay::Slot { slot, pos });
        }

        // VIEW 1 / VIEW 2
        let cx = content.center().x;
        let vy = content.top() + (666.0 - 88.0) / 680.0 * (win_h - 88.0);
        if pill(ui, Rect::from_center_size(pos2(cx - 31.5, vy), vec2(48.0, 24.0)), "VIEW 1", !self.asg.side, "view1").clicked() {
            self.asg.side = false;
        }
        if pill(ui, Rect::from_center_size(pos2(cx + 33.5, vy), vec2(48.0, 24.0)), "VIEW 2", self.asg.side, "view2").clicked() {
            self.asg.side = true;
        }
        let ty = vy + 52.0;
        let sc = pos2(cx + 2.5, ty);
        text(ui, pos2(sc.x - 27.0, ty), Align2::RIGHT_CENTER, "DEFAULT", Ts::new(Fam::Bold, 12.0, if gs { pal().ink(102) } else { pal().text }).ls(1.44));
        text(ui, pos2(sc.x + 27.0, ty), Align2::LEFT_CENTER, "G-SHIFT", Ts::new(Fam::Bold, 12.0, if gs { pal().text } else { pal().ink(102) }).ls(1.44));
        if switch(ui, sc, gs, pal().gshift, "gshift-switch").clicked() {
            self.asg.gshift = !gs;
            if self.asg.gshift && !prof.uses_gshift() {
                self.say("Assign G-Shift to a button (SYSTEM > Device) to use this layer");
            }
        }
        if let Some(slot) = self.asg.selected {
            let name = catalog::slot_name(slot);
            text(ui, pos2(cx, content.top() + 26.0), Align2::CENTER_CENTER, &format!("{name} selected — pick a command, or right-click for options"), Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.6));
        }
    }

    fn drag_ghost(&self, ui: &Ui) {
        let Some(a) = egui::DragAndDrop::payload::<Assignment>(ui.ctx()) else { return };
        let Some(p) = ui.ctx().pointer_hover_pos() else { return };
        let cfg = self.cfg();
        let (label, _) = catalog::label(&a, 0, &cfg.current().0.macros);
        let painter = ui.ctx().layer_painter(egui::LayerId::new(egui::Order::Tooltip, egui::Id::unique("dnd-ghost")));
        let g = galley(ui, &label, Ts::new(Fam::Bold, 12.0, pal().on_primary), f32::INFINITY);
        let r = Rect::from_min_size(p + vec2(14.0, 10.0), g.size() + vec2(24.0, 14.0));
        painter.rect_filled(r, CornerRadius::same(100), pal().primary);
        painter.galley(r.center() - g.size() / 2.0, g, pal().on_primary);
        ui.ctx().set_cursor_icon(egui::CursorIcon::Grabbing);
    }

    fn macro_editor(&mut self, ui: &mut Ui, panel: Rect) {
        let x0 = panel.left() + 16.0;
        let w = 278.0;
        let mut y = panel.top() + 146.0;
        let Some(ed) = &mut self.asg.editor else { return };
        // capture keystrokes and mouse buttons while recording
        if ed.recording {
            let (events, mods) = ui.ctx().input(|i| (i.events.clone(), i.modifiers));
            for (vk, now, before) in [(0xA2u16, mods.ctrl, ed.mods.ctrl), (0xA0, mods.shift, ed.mods.shift), (0xA4, mods.alt, ed.mods.alt)] {
                if now != before {
                    ed.push(vk, now);
                }
            }
            ed.mods = mods;
            for e in events {
                match e {
                    egui::Event::Key { key, pressed, repeat: false, .. } => {
                        if let Some(vk) = egui_vk(key) {
                            ed.push(vk, pressed);
                        }
                    }
                    egui::Event::PointerButton { pos, button, pressed, .. } if ed.list.contains(pos) => {
                        let vk = match button {
                            egui::PointerButton::Primary => 0x01,
                            egui::PointerButton::Secondary => 0x02,
                            egui::PointerButton::Middle => 0x04,
                            egui::PointerButton::Extra1 => 0x05,
                            egui::PointerButton::Extra2 => 0x06,
                        };
                        ed.push(vk, pressed);
                    }
                    _ => {}
                }
            }
            ui.ctx().request_repaint();
        }
        // "+ KEY": the next key press becomes a step
        if ed.capture_key {
            let events = ui.ctx().input(|i| i.events.clone());
            for e in events {
                if let egui::Event::Key { key, pressed: true, repeat: false, .. } = e {
                    if key != egui::Key::Escape
                        && let Some(vk) = egui_vk(key)
                    {
                        ed.add_key(vk);
                    }
                    ed.capture_key = false;
                    break;
                }
            }
            ui.ctx().request_repaint();
        }
        text(ui, pos2(x0, y), Align2::LEFT_TOP, if ed.id.is_some() { "EDIT MACRO" } else { "NEW MACRO" }, t_section());
        y += 28.0;
        let te = egui::TextEdit::singleline(&mut ed.name).hint_text("Macro name").frame(egui::Frame::NONE).font(Ts::new(Fam::Reg, 14.0, pal().text).font()).text_color(pal().text);
        ui.put(Rect::from_min_size(pos2(x0, y), vec2(w, 22.0)), te);
        ui.painter().line_segment([pos2(x0, y + 24.0), pos2(x0 + w, y + 24.0)], Stroke::new(1.0, pal().outline_hi));
        y += 40.0;
        for (kind, label) in [(MacroKind::NoRepeat, "NO REPEAT"), (MacroKind::RepeatWhileHolding, "REPEAT WHILE HOLDING"), (MacroKind::Toggle, "TOGGLE")] {
            if radio(ui, pos2(x0 + 6.0, y), ed.kind == kind, ("kind", label)).clicked() {
                ed.kind = kind;
            }
            let r = click(ui, Rect::from_min_size(pos2(x0 + 16.0, y - 9.0), vec2(220.0, 18.0)), ("kind-l", label));
            text(ui, pos2(x0 + 22.0, y), Align2::LEFT_CENTER, label, Ts::new(Fam::Reg, 12.0, if ed.kind == kind { pal().text } else { pal().muted }));
            if r.clicked() {
                ed.kind = kind;
            }
            y += 22.0;
        }
        y += 6.0;
        let rec_label = if ed.recording { "STOP RECORDING" } else { "START RECORDING" };
        let rb = Rect::from_min_size(pos2(x0, y), vec2(170.0, 30.0));
        if dark_button(ui, rb, rec_label, "rec", true).clicked() {
            ed.recording = !ed.recording;
            ed.last = None;
            ed.mods = ui.ctx().input(|i| i.modifiers);
        }
        if ed.recording {
            let t = ui.ctx().input(|i| i.time);
            ui.painter().circle_filled(pos2(rb.left() + 14.0, rb.center().y), 4.0, pal().error.gamma_multiply(0.55 + 0.45 * (t * 4.0).sin().abs() as f32));
        }
        if checkbox(ui, pos2(rb.right() + 12.0, y + 8.0), "DELAYS", ed.record_delays, true, "rec-delays").clicked() {
            ed.record_delays = !ed.record_delays;
        }
        y += 40.0;

        // add row: + KEY / + CLICK / + DELAY / + TEXT
        let mut say_msg: Option<String> = None;
        if ed.capture_key {
            if blue_button(ui, Rect::from_min_size(pos2(x0, y), vec2(w, 26.0)), "PRESS A KEY\u{2026}  (ESC CANCELS)", "add-key-cancel").clicked() {
                ed.capture_key = false;
            }
        } else {
            let bw = (w - 18.0) / 4.0;
            for (i, label) in ["+ KEY", "+ CLICK", "+ DELAY", "+ TEXT"].into_iter().enumerate() {
                let r = Rect::from_min_size(pos2(x0 + i as f32 * (bw + 6.0), y), vec2(bw, 26.0));
                let on = (i == 1 && ed.click_row) || (i == 3 && ed.text_open);
                let clicked = if on { blue_button(ui, r, label, ("add", i)).clicked() } else { dark_button(ui, r, label, ("add", i), true).clicked() };
                if clicked {
                    match i {
                        0 => {
                            ed.capture_key = true;
                            ed.click_row = false;
                            ed.text_open = false;
                        }
                        1 => {
                            ed.click_row = !ed.click_row;
                            ed.text_open = false;
                        }
                        2 => ed.add_delay(),
                        _ => {
                            ed.text_open = !ed.text_open;
                            ed.text_focus = ed.text_open;
                            ed.click_row = false;
                        }
                    }
                }
            }
        }
        y += 32.0;
        if ed.click_row {
            let pw = (w - 16.0) / 5.0;
            for (i, label) in ["LEFT", "RIGHT", "MIDDLE", "BACK", "FORWARD"].into_iter().enumerate() {
                let r = Rect::from_min_size(pos2(x0 + i as f32 * (pw + 4.0), y), vec2(pw, 22.0));
                if pill_sized(ui, r, label, false, 10.0, ("click-pill", i)).clicked() {
                    ed.add_click(i as u8);
                    ed.click_row = false;
                }
            }
            y += 30.0;
        }
        if ed.text_open {
            let fr = Rect::from_min_size(pos2(x0, y), vec2(w, 22.0));
            let te = egui::TextEdit::singleline(&mut ed.text_buf)
                .hint_text("Type text, press Enter")
                .frame(egui::Frame::NONE)
                .font(Ts::new(Fam::Reg, 12.0, pal().text).font())
                .text_color(pal().text);
            let resp = ui.put(fr, te);
            ui.painter().line_segment([pos2(fr.left(), fr.bottom() + 2.0), pos2(fr.right(), fr.bottom() + 2.0)], Stroke::new(1.0, pal().outline_hi));
            if ed.text_focus {
                resp.request_focus();
                ed.text_focus = false;
            }
            if resp.lost_focus() && ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) {
                let typed = std::mem::take(&mut ed.text_buf);
                let skipped = ed.add_text(&typed, crate::win::char_key);
                if skipped > 0 {
                    say_msg = Some(format!("Skipped {skipped} character(s) this keyboard can't type"));
                }
                ed.text_focus = true;
            }
            y += 32.0;
        }

        let list = Rect::from_min_max(pos2(x0, y), pos2(panel.right() - 12.0, panel.bottom() - 56.0));
        ed.list = list;
        let recording = ed.recording;
        let steps = ed.steps.clone();
        let mut edit_delay = ed.edit_delay.take();
        let mut remove: Option<usize> = None;
        let mut mv: Option<(usize, bool)> = None;
        let mut commit: Option<(usize, u32)> = None;
        let mut child = ui.new_child(UiBuilder::new().max_rect(list));
        egui::ScrollArea::vertical().id_salt("macro-steps").auto_shrink([false, false]).stick_to_bottom(true).show(&mut child, |ui| {
            if recording {
                let (r, _) = ui.allocate_exact_size(vec2(270.0, 34.0), Sense::hover());
                para(ui, r.min, 270.0, "Recording: keys anywhere, mouse buttons inside this box.", Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4));
            }
            if steps.is_empty() {
                let (r, _) = ui.allocate_exact_size(vec2(270.0, 40.0), Sense::hover());
                para(ui, r.min, 270.0, "Press Start Recording, then type the keys for this macro.", Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.6));
            }
            for (i, s) in steps.iter().enumerate() {
                let (r, _) = ui.allocate_exact_size(vec2(270.0, 26.0), Sense::hover());
                let hover = !recording && ui.rect_contains_pointer(r);
                let dr = Rect::from_min_size(pos2(r.left(), r.top() + 3.0), vec2(62.0, 20.0));
                let editing = matches!(&edit_delay, Some((ei, _)) if *ei == i);
                if editing {
                    let mut finish = false;
                    if let Some((_, buf)) = &mut edit_delay {
                        let te = egui::TextEdit::singleline(buf).frame(egui::Frame::NONE).char_limit(5).font(Ts::new(Fam::Reg, 11.0, pal().text).font()).text_color(pal().text);
                        let er = ui.put(dr, te);
                        ui.painter().line_segment([pos2(dr.left(), dr.bottom()), pos2(dr.right(), dr.bottom())], Stroke::new(1.0, pal().primary));
                        if er.lost_focus() {
                            commit = Some((i, buf.trim().parse().unwrap_or(s.delay_ms)));
                            finish = true;
                        } else if !er.has_focus() {
                            er.request_focus();
                        }
                    }
                    if finish {
                        edit_delay = None;
                    }
                } else if recording {
                    if s.delay_ms > 0 {
                        text(ui, pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, &format!("{} ms", s.delay_ms), Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4));
                    }
                } else {
                    let dc = click(ui, dr, ("step-delay", i));
                    if s.delay_ms > 0 || hover {
                        let c = if s.delay_ms > 0 { pal().muted } else { pal().muted.gamma_multiply(0.5) };
                        text(ui, pos2(r.left() + 2.0, r.center().y), Align2::LEFT_CENTER, &format!("{} ms", s.delay_ms), Ts::new(Fam::Reg, 11.0, if dc.hovered() { pal().text } else { c }).ls(0.4));
                    }
                    if dc.clicked() {
                        edit_delay = Some((i, s.delay_ms.to_string()));
                    }
                }
                if s.vk == 0 {
                    text(ui, pos2(r.left() + 84.0, r.center().y), Align2::LEFT_CENTER, "Delay", Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.4));
                } else {
                    chevron(ui.painter(), pos2(r.left() + 70.0, r.center().y), 4.0, !s.down, Stroke::new(1.5, if s.down { pal().primary } else { pal().muted }));
                    text(ui, pos2(r.left() + 84.0, r.center().y), Align2::LEFT_CENTER, &catalog::vk_name(s.vk), Ts::new(Fam::Reg, 12.0, pal().text).ls(0.4));
                }
                if hover {
                    let xr = Rect::from_center_size(pos2(r.right() - 12.0, r.center().y), vec2(20.0, 20.0));
                    let x = click(ui, xr, ("step-x", i));
                    caption_glyph(ui.painter(), xr.center(), 2, if x.hovered() { pal().error } else { pal().muted });
                    if x.clicked() {
                        remove = Some(i);
                    }
                    for (k, up) in [(0usize, false), (1, true)] {
                        let cr = Rect::from_center_size(pos2(r.right() - 34.0 - k as f32 * 20.0, r.center().y), vec2(18.0, 18.0));
                        let c = click(ui, cr, ("step-mv", i, up));
                        chevron(ui.painter(), cr.center(), 3.5, up, Stroke::new(1.5, if c.hovered() { pal().primary } else { pal().muted }));
                        if c.clicked() {
                            mv = Some((i, up));
                        }
                    }
                }
            }
        });
        ed.edit_delay = edit_delay;
        if let Some((i, ms)) = commit {
            ed.set_delay(i, ms);
        }
        if let Some((i, up)) = mv {
            ed.move_step(i, up);
        }
        if let Some(i) = remove {
            ed.steps.remove(i);
        }

        // bottom row: SAVE / TEST / CANCEL
        let by = panel.bottom() - 46.0;
        let bw = (w - 16.0) / 3.0;
        let save = blue_button(ui, Rect::from_min_size(pos2(x0, by), vec2(bw, 32.0)), "SAVE", "macro-save").clicked();
        let test_label = match ed.test_at {
            Some(t) => format!("PLAYS IN {}\u{2026}", 3 - t.elapsed().as_secs().min(2)),
            None => "TEST".into(),
        };
        if dark_button(ui, Rect::from_min_size(pos2(x0 + bw + 8.0, by), vec2(bw, 32.0)), &test_label, "macro-test", ed.test_at.is_none()).clicked() && ed.test_at.is_none() {
            ed.test_at = Some(Instant::now());
        }
        if let Some(t) = ed.test_at {
            ui.ctx().request_repaint();
            if t.elapsed().as_secs_f32() >= 3.0 {
                ed.test_at = None;
                let _ = self.eng.send(EngineMsg::TestMacro(ed.steps.clone()));
            }
        }
        let cancel = dark_button(ui, Rect::from_min_size(pos2(x0 + 2.0 * (bw + 8.0), by), vec2(bw, 32.0)), "CANCEL", "macro-cancel", true).clicked();
        if let Some(m) = say_msg {
            self.say(m);
        }
        if cancel {
            self.asg.editor = None;
        } else if save {
            let ed = self.asg.editor.take().unwrap();
            let name = if ed.name.trim().is_empty() { "Macro".to_string() } else { ed.name.trim().to_string() };
            let id = ed.id.clone().unwrap_or_else(new_id);
            let m = Macro { id: id.clone(), name: name.clone(), kind: ed.kind, steps: ed.steps };
            self.edit(|c| {
                let ca = c.current_app;
                let ms = &mut c.apps[ca].macros;
                match ms.iter_mut().find(|x| x.id == id) {
                    Some(x) => *x = m,
                    None => ms.push(m),
                }
            });
            self.say(format!("Saved macro \"{name}\" \u{2014} drag it onto a button"));
        }
    }
}

fn launch_cards(cfg: &Config) -> Vec<Card> {
    let mut seen = HashSet::new();
    let mut v = Vec::new();
    for p in &cfg.current().0.profiles {
        for a in p.base.iter().chain(p.gshift.iter()) {
            if let Assignment::Launch { name, path } = a
                && seen.insert(path.clone())
            {
                v.push(Card { name: name.clone(), detail: String::new(), assignment: a.clone() });
            }
        }
    }
    v
}

fn plus_text(ui: &Ui, r: Rect, label: &str, hot: bool) {
    let c = if hot { pal().primary } else { pal().text };
    let pc = pos2(r.left() + 6.0, r.center().y);
    ui.painter().line_segment([pc - vec2(3.5, 0.0), pc + vec2(3.5, 0.0)], Stroke::new(1.5, c));
    ui.painter().line_segment([pc - vec2(0.0, 3.5), pc + vec2(0.0, 3.5)], Stroke::new(1.5, c));
    text(ui, pos2(r.left() + 20.0, r.center().y), Align2::LEFT_CENTER, label, Ts::new(Fam::Bold, 12.0, c));
}

pub(crate) fn search_field(ui: &mut Ui, rect: Rect, value: &mut String, placeholder: &str) {
    search(ui.painter(), pos2(rect.left() + 8.0, rect.center().y), Stroke::new(1.6, pal().text));
    let te = egui::TextEdit::singleline(value)
        .hint_text(egui::RichText::new(placeholder).color(pal().label).font(Ts::new(Fam::Reg, 14.0, pal().label).font()))
        .frame(egui::Frame::NONE)
        .font(Ts::new(Fam::Reg, 14.0, pal().text).font())
        .text_color(pal().text);
    let mut child = ui.new_child(UiBuilder::new().max_rect(Rect::from_min_max(pos2(rect.left() + 26.0, rect.top()), rect.right_bottom())));
    child.put(Rect::from_min_max(pos2(rect.left() + 26.0, rect.top() + 3.0), rect.right_bottom()), te);
    ui.painter().line_segment([pos2(rect.left(), rect.bottom() + 1.0), pos2(rect.right(), rect.bottom() + 1.0)], Stroke::new(1.0, pal().n(0xb2)));
    let _ = StrokeKind::Inside;
}

/// Small numeric input. Shows `current` unless focused; returns a parsed
/// value when the user changes the text.
fn number_field(ui: &mut Ui, rect: Rect, buf: &mut String, current: &str, id: &str) -> Option<f64> {
    ui.painter().rect_stroke(rect, CornerRadius::same(4), Stroke::new(1.0, pal().outline_hi), StrokeKind::Inside);
    let te = egui::TextEdit::singleline(buf)
        .id_salt(id)
        .frame(egui::Frame::NONE)
        .char_limit(8)
        .font(Ts::new(Fam::Bold, 12.0, pal().text).font())
        .text_color(pal().text);
    let mut child = ui.new_child(UiBuilder::new().max_rect(rect));
    let resp = child.put(rect.shrink2(vec2(8.0, 3.0)), te);
    let out = if resp.changed() { buf.trim().replace(',', ".").parse::<f64>().ok() } else { None };
    if !resp.has_focus() {
        *buf = current.to_string();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(vk: u16, down: bool, delay_ms: u32) -> MacroStep {
        MacroStep { vk, down, delay_ms }
    }

    #[test]
    fn set_delay_clamps() {
        let mut e = MacroEditor::new();
        e.steps.push(step(0x41, true, 0));
        e.set_delay(0, 70_000);
        assert_eq!(e.steps[0].delay_ms, 60_000);
        e.set_delay(5, 10);
        assert_eq!(e.steps.len(), 1);
    }

    #[test]
    fn move_step_swaps_and_stops_at_ends() {
        let mut e = MacroEditor::new();
        e.steps = vec![step(1, true, 0), step(2, true, 0), step(3, true, 0)];
        e.move_step(1, true);
        assert_eq!(e.steps.iter().map(|s| s.vk).collect::<Vec<_>>(), [2, 1, 3]);
        e.move_step(0, true);
        e.move_step(2, false);
        assert_eq!(e.steps.iter().map(|s| s.vk).collect::<Vec<_>>(), [2, 1, 3]);
        e.move_step(1, false);
        assert_eq!(e.steps.iter().map(|s| s.vk).collect::<Vec<_>>(), [2, 3, 1]);
    }

    #[test]
    fn add_key_and_click_append_pairs() {
        let mut e = MacroEditor::new();
        e.add_key(0x41);
        assert_eq!(e.steps, vec![step(0x41, true, 0), step(0x41, false, 50)]);
        e.add_click(2);
        assert_eq!(e.steps[2..], [step(0x04, true, 50), step(0x04, false, 50)]);
    }

    #[test]
    fn add_text_uses_shift_and_counts_skips() {
        let mut e = MacroEditor::new();
        let map = |c: char| match c {
            'a' => Some((0x41, false)),
            'A' => Some((0x41, true)),
            _ => None,
        };
        let skipped = e.add_text("aA\u{20ac}", map);
        assert_eq!(skipped, 1);
        let got: Vec<(u16, bool)> = e.steps.iter().map(|s| (s.vk, s.down)).collect();
        assert_eq!(got, [(0x41, true), (0x41, false), (0xA0, true), (0x41, true), (0x41, false), (0xA0, false)]);
    }
}
