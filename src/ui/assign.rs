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
    pub editor: Option<super::macros::MacroEditor>,
    pub last_tab: Option<DevTab>,
    pub launch_path: String,
    pub p180_sens_text: String,
    pub p180_counts_text: String,
    pub game_sens: Option<(usize, Option<crate::games::GameSens>)>,
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
        {
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
            self.asg.editor = Some(super::macros::MacroEditor::new_named());
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
                            macro_badge(ui, Rect::from_min_size(pos2(lx, rect.center().y - 7.0), vec2(14.0, 14.0)), macros.iter().find(|m| Assignment::Macro(m.id.clone()) == c.assignment).and_then(|m| m.color));
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
            self.asg.editor = Some(super::macros::MacroEditor::open(m));
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
                    macro_badge(ui, Rect::from_min_size(pos2(lrect.left(), lrect.bottom() - 18.0), vec2(18.0, 18.0)), app.macros.iter().find(|m| Assignment::Macro(m.id.clone()) == layer_a).and_then(|m| m.color));
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
}

pub(super) fn launch_cards(cfg: &Config) -> Vec<Card> {
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
