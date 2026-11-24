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
/// Extra height the HARDWARE FLICK section adds to the Perfect 180 box.
const FLICK_H: f32 = 132.0;
/// Extra height the MAKE A LUA 180 row adds under the SENDER row.
const LUA_H: f32 = 48.0;

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
    /// HARDWARE FLICK: physical swipe length in cm (slider, 0.5 steps).
    pub flick_cm: f32,
    /// MAKE A LUA 180: the G-key the generated OnEvent block binds to
    /// (`G_PRESSED` arg 4..=11; primary buttons can't be OnEvent args).
    pub p180_g: u8,
    pub game_sens: Option<(usize, Option<crate::games::GameSens>)>,
    /// Game sens behind the selected Perfect 180 preset pill (READ FROM GAME).
    pub read_sens: Option<(usize, Option<crate::games::GameSens>)>,
    /// `(app, applied preset, GameSens::source)` of the last READ FROM GAME —
    /// the `from …` caption under the link while that read is still in force.
    pub read_from: Option<(usize, usize, String)>,
    pub obs_form: Option<ObsForm>,
    /// Shot pages only: keep the list scrolled to its end.
    pub scroll_to: Option<f32>,
    pub obs_refresh: Option<std::time::Instant>,
}

impl Default for AsgState {
    fn default() -> Self {
        Self {
            tab: 0, search: String::new(), side: false, gshift: false, selected: None,
            collapsed: HashSet::new(), mods: 0, editor: None, last_tab: None,
            launch_path: String::new(), p180_sens_text: String::new(), p180_counts_text: String::new(),
            flick_cm: 2.5, p180_g: 5, game_sens: None, read_sens: None, read_from: None, obs_form: None, scroll_to: None, obs_refresh: None,
        }
    }
}

/// The CREATE NEW ACTION form of the OBS section.
#[derive(Default)]
pub struct ObsForm {
    pub kind: usize,
    pub text: String,
}

const OBS_KINDS: [&str; 8] = ["SCENE", "COLLECTION", "SOURCE ON", "SOURCE OFF", "SOURCE TOGGLE", "MIXER MUTE", "MIXER UNMUTE", "MIXER TOGGLE"];

pub(crate) fn obs_target_action(kind: usize, name: &str) -> ObsAction {
    let n = name.to_string();
    match kind {
        0 => ObsAction::Scene(n),
        1 => ObsAction::Collection(n),
        2 => ObsAction::SourceOn(n),
        3 => ObsAction::SourceOff(n),
        4 => ObsAction::SourceToggle(n),
        5 => ObsAction::MixerMute(n),
        6 => ObsAction::MixerUnmute(n),
        _ => ObsAction::MixerToggle(n),
    }
}

fn obs_status_text(s: &crate::obs::ObsStatus) -> String {
    use crate::obs::ObsStatus::*;
    match s {
        Connected { version } => format!("Connected to OBS {version}."),
        NotRunning | Unknown => "OBS is not running. Actions work as soon as it is.".into(),
        ServerOff => "In OBS open Tools > WebSocket Server Settings and turn on \"Enable WebSocket server\".".into(),
        NeverStarted => "Start OBS once, then turn on Tools > WebSocket Server Settings > \"Enable WebSocket server\".".into(),
        NotInstalled => "OBS Studio is not installed.".into(),
        WrongPassword => "OBS refused the password from its own settings. Restart OBS and try again.".into(),
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
    ObsStatus(String),
    ObsNew,
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
        if self.asg.tab == 2 {
            out.push(ListItem::Header("OBS".into(), None));
            if !(self.asg.collapsed.contains("OBS") && q.is_empty()) {
                if q.is_empty() {
                    out.push(ListItem::ObsStatus(obs_status_text(&self.obs.status())));
                }
                for a in ObsAction::SIMPLE.iter().chain(cfg.obs_actions.iter()) {
                    let c = Card { name: catalog::obs_name(a), detail: String::new(), assignment: Assignment::Obs(a.clone()) };
                    if keep(&c) {
                        out.push(ListItem::Card(c));
                    }
                }
                if q.is_empty() {
                    out.push(ListItem::ObsNew);
                }
            }
        }
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
        let mut delete_obs: Option<ObsAction> = None;
        if self.asg.tab == 2 && !self.asg.collapsed.contains("OBS") && self.shot.is_none() {
            if self.asg.obs_refresh.is_none_or(|t| t.elapsed().as_secs() >= 5) {
                self.obs.refresh();
                self.asg.obs_refresh = Some(std::time::Instant::now());
            }
        } else {
            self.asg.obs_refresh = None;
        }
        let obs_ok = matches!(self.obs.status(), crate::obs::ObsStatus::Connected { .. });
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
        let mut sa = egui::ScrollArea::vertical().id_salt(("asg-list", tab)).auto_shrink([false, false]);
        if let Some(y) = self.asg.scroll_to {
            sa = sa.vertical_scroll_offset(y);
        }
        sa.show(&mut child, |ui| {
            let w = 270.0;
            for (i, it) in items.iter().enumerate() {
                match it {
                    ListItem::Header(title, tag) => {
                        let (rect, r) = ui.allocate_exact_size(vec2(w, 42.0), Sense::click());
                        let open = !self.asg.collapsed.contains(title.as_str());
                        let tx = rect.left();
                        let _ = tag;
                        let mut tx = tx;
                        if title == "OBS" {
                            let c = pos2(tx + 8.0, rect.center().y);
                            ui.painter().circle_stroke(c, 6.5, Stroke::new(1.6, pal().text));
                            ui.painter().circle_filled(c, 2.5, pal().text);
                            tx += 26.0;
                        }
                        let tr = text(ui, pos2(tx, rect.center().y), Align2::LEFT_CENTER, title, Ts::new(Fam::Bold, 14.0, pal().text).ls(0.3));
                        if title == "OBS" && !obs_ok {
                            let c = pos2(tr.right() + 16.0, rect.center().y);
                            let st = Stroke::new(1.5, pal().gshift);
                            ui.painter().add(egui::Shape::closed_line(vec![c + vec2(0.0, -6.5), c + vec2(7.0, 6.0), c + vec2(-7.0, 6.0)], st));
                            ui.painter().line_segment([c + vec2(0.0, -2.0), c + vec2(0.0, 1.5)], st);
                            ui.painter().circle_filled(c + vec2(0.0, 3.8), 0.9, pal().gshift);
                        }
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
                    ListItem::ObsStatus(msg) => {
                        let g = galley(ui, msg, Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4), w);
                        let (rect, _) = ui.allocate_exact_size(vec2(w, g.size().y + 10.0), Sense::hover());
                        ui.painter().galley(rect.min, g, pal().muted);
                    }
                    ListItem::ObsNew => match self.asg.obs_form.take() {
                        None => {
                            let (row, _) = ui.allocate_exact_size(vec2(w, 34.0), Sense::hover());
                            let br = Rect::from_min_size(row.min + vec2(0.0, 4.0), vec2(w, 26.0));
                            let r = click(ui, br, "obs-new");
                            plus_text(ui, br, "CREATE NEW ACTION", r.hovered());
                            if r.clicked() {
                                self.asg.obs_form = Some(ObsForm::default());
                            }
                        }
                        Some(mut f) => {
                            let lists = self.obs.lists();
                            let connected = matches!(self.obs.status(), crate::obs::ObsStatus::Connected { .. });
                            let names: Vec<String> = match f.kind {
                                0 => lists.scenes,
                                1 => lists.collections,
                                2..=4 => lists.sources,
                                _ => lists.mixers,
                            };
                            let q = f.text.to_lowercase();
                            let shown: Vec<String> = if connected { names.into_iter().filter(|n| q.is_empty() || n.to_lowercase().contains(&q)).take(6).collect() } else { Vec::new() };
                            let h = 2.0 * 28.0 + 10.0 + 28.0 + shown.len() as f32 * 22.0 + 8.0 + 34.0 + 8.0;
                            let (rect, _) = ui.allocate_exact_size(vec2(w, h), Sense::hover());
                            let mut form_ui = ui.new_child(UiBuilder::new().max_rect(rect));
                            let ui = &mut form_ui;
                            let pw = (w - 18.0) / 4.0;
                            for (i, label) in OBS_KINDS.iter().enumerate() {
                                let pr = Rect::from_min_size(rect.min + vec2((i % 4) as f32 * (pw + 6.0), (i / 4) as f32 * 28.0 + 2.0), vec2(pw, 22.0));
                                if pill_sized(ui, pr, label, f.kind == i, 8.5, ("obs-kind", i)).clicked() {
                                    f.kind = i;
                                    f.text.clear();
                                }
                            }
                            let fy = rect.top() + 2.0 * 28.0 + 10.0;
                            let fr = Rect::from_min_size(pos2(rect.left(), fy), vec2(w, 24.0));
                            let hint = ["SELECT A SCENE", "SELECT A COLLECTION", "SELECT A SOURCE", "SELECT A SOURCE", "SELECT A SOURCE", "SELECT A MIXER", "SELECT A MIXER", "SELECT A MIXER"][f.kind];
                            let te = egui::TextEdit::singleline(&mut f.text)
                                .hint_text(egui::RichText::new(hint).color(pal().label).font(Ts::new(Fam::Reg, 12.0, pal().label).font()))
                                .frame(egui::Frame::NONE)
                                .font(Ts::new(Fam::Reg, 12.0, pal().text).font())
                                .text_color(pal().text);
                            ui.put(Rect::from_min_max(fr.min + vec2(2.0, 2.0), fr.max - vec2(2.0, 2.0)), te);
                            ui.painter().line_segment([fr.left_bottom(), fr.right_bottom()], Stroke::new(1.0, pal().outline_hi));
                            let mut ry = fr.bottom() + 4.0;
                            let mut fill: Option<String> = None;
                            for (i, n) in shown.iter().enumerate() {
                                let rr = Rect::from_min_size(pos2(rect.left(), ry), vec2(w, 22.0));
                                let r = click(ui, rr, ("obs-name", i));
                                text(ui, pos2(rr.left() + 4.0, rr.center().y), Align2::LEFT_CENTER, n, Ts::new(Fam::Reg, 12.0, if r.hovered() { pal().primary } else { pal().text }).ls(0.4));
                                if r.clicked() {
                                    fill = Some(n.clone());
                                }
                                ry += 22.0;
                            }
                            if let Some(n) = fill {
                                f.text = n;
                            }
                            let by = ry + 8.0;
                            let ok = !f.text.trim().is_empty();
                            let add = Rect::from_min_size(pos2(rect.left(), by), vec2(110.0, 26.0));
                            let cancel = Rect::from_min_size(pos2(rect.left() + 120.0, by), vec2(110.0, 26.0));
                            let add_click = if ok { blue_button(ui, add, "ADD", "obs-add").clicked() } else { {
                                dark_button(ui, add, "ADD", "obs-add", false);
                                false
                            }
                            };
                            let cancel_click = dark_button(ui, cancel, "CANCEL", "obs-cancel", true).clicked();
                            if add_click {
                                let a = obs_target_action(f.kind, f.text.trim());
                                self.edit(|c| {
                                    if !c.obs_actions.contains(&a) {
                                        c.obs_actions.push(a);
                                    }
                                });
                            } else if !cancel_click {
                                self.asg.obs_form = Some(f);
                            }
                        }
                    },
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
                        if let Assignment::Obs(a) = &c.assignment
                            && cfg.obs_actions.contains(a)
                            && ui.rect_contains_pointer(rect)
                        {
                            let xr = Rect::from_center_size(pos2(rect.right() - 16.0, rect.center().y), vec2(20.0, 20.0));
                            let x = click(ui, xr, ("obs-del", i));
                            caption_glyph(ui.painter(), xr.center(), 2, if x.hovered() { pal().error } else { pal().muted });
                            if x.clicked() {
                                delete_obs = Some(a.clone());
                            }
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
                        if r.clicked() && edit_macro.is_none() && delete_macro.is_none() && delete_obs.is_none() {
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
        if let Some(a) = delete_obs {
            let gone = Assignment::Obs(a.clone());
            self.edit(|c| {
                c.obs_actions.retain(|x| *x != a);
                for app in &mut c.apps {
                    for p in &mut app.profiles {
                        for s in p.base.iter_mut().chain(p.gshift.iter_mut()) {
                            if *s == gone {
                                *s = Assignment::Default;
                            }
                        }
                    }
                }
            });
        }
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
                (app.name == "Counter-Strike 2").then_some(crate::games::GameSens { game: "Counter-Strike 2", preset: 0, sens: 0.51, yaw: 0.022, counts: 16043, source: "cs2_user_convars_0_slot0.vcfg".into() })
            } else {
                crate::games::game_sensitivity(app.ghub_id.as_deref(), &app.name)
            };
            self.asg.game_sens = Some((idx, found));
        }
        self.asg.game_sens.as_ref().and_then(|(_, g)| g.clone())
    }

    /// Sensitivity behind the selected Perfect 180 preset pill, re-read when the
    /// selection changes. `--shot` gets the deterministic CS2 sample.
    fn read_sens_for(&mut self, game: usize) -> Option<crate::games::GameSens> {
        if self.asg.read_sens.as_ref().is_none_or(|(i, _)| *i != game) {
            let found = if self.shot.is_some() {
                (game == 0).then_some(crate::games::GameSens { game: "Counter-Strike 2", preset: 0, sens: 0.51, yaw: 0.022, counts: 16043, source: "cs2_user_convars_0_slot0.vcfg".into() })
            } else {
                crate::games::preset_sensitivity(game)
            };
            self.asg.read_sens = Some((game, found));
        }
        self.asg.read_sens.as_ref().and_then(|(_, g)| g.clone())
    }

    fn p180_block(&mut self, ui: &mut Ui) {
        let top = ui.cursor().min;
        let cfg = self.cfg();
        let e = cfg.extras.clone();
        let per_profile = cfg.current().1.turn.is_some();
        let cal = turn_for(cfg.current().1, &e);
        let game_sens = self.game_sens_for(&cfg);
        let extra = if game_sens.is_some() { 16.0 } else { 0.0 };
        // implausible travel: warn between the COUNTS row and ANGLE
        let dpi = cfg.current().1.dpi.active;
        let travel = p180_travel_cm(cal.counts, dpi);
        let warn = travel > 150.0;
        let warn_text = format!("{} cm of mouse travel for one full turn. That is not a real setting: check the sensitivity.", thousands(travel.round() as i64));
        let warn_g = warn.then(|| galley(ui, &warn_text, Ts::new(Fam::Reg, 11.0, pal().amber).ls(0.4), 254.0));
        let warn_h = warn_g.as_ref().map_or(0.0, |g| g.size().y + 8.0);
        // the preset the pills show (a count that doesn't match the preset
        // maths is CUSTOM); resolved before the box so its height can grow for
        // the blocked note
        let mut game = cal.game.min(P180_GAMES.len() - 1);
        if game != P180_CUSTOM && p180_preset_counts(game, cal.sens) != Some(cal.counts) {
            game = P180_CUSTOM;
        }
        let shown_game = game;
        // `sender_h`: SENDER pills + the optional blocked note under SPEED
        let blocked = e.p180_blocked.contains(&game);
        let sender_h = 80.0 + if blocked { 26.0 } else { 0.0 };
        // READ FROM GAME line under IN-GAME SENS (only when the selected
        // preset's parser actually finds a settings file); after a read a
        // `from <source>` caption sits under it for another 14 px.
        let read_sens = self.read_sens_for(shown_game);
        let from_src = self.asg.read_from.as_ref().and_then(|(a, g, s)| (*a == cfg.current_app && *g == shown_game).then(|| s.clone()));
        let read_h = if read_sens.is_some() { 18.0 + if from_src.is_some() { 14.0 } else { 0.0 } } else { 0.0 };
        let box_r = Rect::from_min_size(top, vec2(278.0, 366.0 + extra + warn_h + sender_h + LUA_H + FLICK_H + read_h));
        ui.painter().rect_filled(box_r, CornerRadius::same(8), pal().surface_hi);
        let x = box_r.left() + 12.0;
        text(ui, pos2(x, box_r.top() + 12.0), Align2::LEFT_TOP, "PERFECT 180", Ts::new(Fam::Bold, 12.0, pal().text));
        para(ui, pos2(x, box_r.top() + 30.0), 254.0, "One press turns you 180°. Calibrate it from your in-game sensitivity.", Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.4));

        // game presets (3 x 2)
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
                self.asg.read_from = None;
            }
        }
        let yaw = P180_GAMES[game].1;
        let row = |i: usize| box_r.top() + 136.0 + extra + i as f32 * 28.0 + if i >= 1 { read_h } else { 0.0 } + if i >= 3 { warn_h } else { 0.0 };

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
        if game == P180_FORTNITE && sens_val < 1.0 {
            let pct = format!("{:.1}", sens_val * 100.0);
            let pct = pct.strip_suffix(".0").unwrap_or(&pct);
            text(ui, pos2(x + 196.0, row(0)), Align2::LEFT_CENTER, &format!("= {pct} %"), Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.2));
        }
        if let Some(g) = &read_sens {
            let link = Rect::from_min_size(pos2(x, row(0) + 15.0), vec2(254.0, 14.0));
            let rr = click(ui, link, "p180-read-game");
            let col = if rr.hovered() { pal().primary_hover } else { pal().primary };
            text(ui, pos2(box_r.right() - 12.0, link.center().y), Align2::RIGHT_CENTER, "READ FROM GAME", Ts::new(Fam::Bold, 9.5, col).ls(0.4));
            if rr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                // same math as typing the sens: preset counts, or the parser's
                // own counts when its yaw makes the preset a custom one (CS2)
                let counts = if g.preset == P180_CUSTOM { g.counts } else { p180_preset_counts(g.preset, g.sens).unwrap_or(g.counts) };
                let new = TurnCal { counts, game: g.preset, sens: g.sens, angle: cal.angle };
                self.asg.read_from = Some((cfg.current_app, new.game, g.source.clone()));
                self.edit(|c| {
                    if c.current_profile_mut().turn.is_some() {
                        c.current_profile_mut().turn = Some(new);
                    } else {
                        c.extras.p180_game = new.game;
                        c.extras.p180_sens = new.sens;
                        c.extras.p180_counts = new.counts;
                    }
                });
            }
            // `from <file>`/`from <file>: <key>` — which settings file (and for
            // Fortnite which property) the value just applied came from.
            if let Some(src) = &from_src {
                text(ui, pos2(x, row(0) + 30.0), Align2::LEFT_CENTER, &format!("from {src}"), Ts::new(Fam::Reg, 9.5, pal().muted).ls(0.3));
            }
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

        if let Some(g) = warn_g {
            let y0 = row(2) + 16.0;
            ui.painter().galley(pos2(x, y0), g.clone(), pal().amber);
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

        // SENDER: which user-mode API the turn's moves ride on. A fallback for
        // games with flaky handling of one API — never an anti-cheat workaround
        // (a game that drops injected input drops all three; PLAN.md's boundary
        // keeps drivers, Interception and raw-input devices out of scope). The
        // honest answer for a blocked game is the HARDWARE FLICK below.
        text(ui, pos2(x, row(5)), Align2::LEFT_CENTER, "SENDER", t_small_label());
        {
            let link = Rect::from_min_size(pos2(x + 104.0, row(5) - 11.0), vec2(150.0, 22.0));
            let lr = click(ui, link, "p180-blocked");
            let col = if blocked { pal().amber } else if lr.hovered() { pal().primary_hover } else { pal().primary };
            text(ui, pos2(x + 254.0, row(5)), Align2::RIGHT_CENTER, "GAME BLOCKS IT", Ts::new(Fam::Bold, 9.5, col).ls(0.3));
            if lr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.edit(|c| p180_toggle_blocked(&mut c.extras.p180_blocked, game));
            }
        }
        for (v, label) in [(0u8, "SENDINPUT"), (1, "MOUSE_EVENT"), (2, "POST_MSG")] {
            let pr = Rect::from_min_size(pos2(x + v as f32 * 86.0, row(5) + 20.0), vec2(82.0, 22.0));
            if pill_sized(ui, pr, label, e.p180_sender == v, 8.5, ("p180-sender", v)).clicked() && e.p180_sender != v {
                self.edit(|c| c.extras.p180_sender = v);
                let _ = self.eng.send(EngineMsg::SetSender(v));
            }
        }
        if blocked {
            para(ui, pos2(x, row(5) + 48.0), 254.0, "marked as blocking injected turns — try HARDWARE FLICK instead", Ts::new(Fam::Reg, 10.0, pal().amber).ls(0.3));
        }

        if recalc && let Some(c) = p180_preset_counts(game, sens_val) {
            counts = c;
        }
        // a hand-set count no longer matches the preset maths
        if manual && p180_preset_counts(game, sens_val) != Some(counts) {
            game = P180_CUSTOM;
        }
        if recalc || manual || ms_changed || angle_changed || game != shown_game {
            let new = TurnCal { counts, game, sens: sens_val, angle: angle_deg };
            let ms = ms.round() as u16;
            // the value no longer matches what READ FROM GAME applied
            self.asg.read_from = None;
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

        // MAKE A LUA 180: the same injected turn as an editable OnEvent block
        // in the profile's script, bound to a G-key (G4..G11 fire `G_PRESSED`;
        // the primary buttons can't be OnEvent args). The managed markers let
        // a re-click REPLACE the block instead of piling up copies; the
        // engine's script watcher picks the edit up on the next ConfigChanged.
        let ly = row(4) + 20.0 + sender_h;
        text(ui, pos2(x, ly + 13.0), Align2::LEFT_CENTER, "MAKE A LUA 180", t_small_label());
        let gr = Rect::from_min_size(pos2(x + 112.0, ly + 2.0), vec2(42.0, 22.0));
        if pill_sized(ui, gr, &format!("G{}", self.asg.p180_g), true, 11.0, "p180-lua-g").clicked() {
            let g = self.asg.p180_g + 1;
            self.asg.p180_g = if g > 11 { 4 } else { g };
        }
        let ar = Rect::from_min_size(pos2(x + 254.0 - 96.0, ly), vec2(96.0, 26.0));
        if dark_button(ui, ar, "ADD TO SCRIPT", "p180-lua-add", true).clicked() {
            let g = self.asg.p180_g;
            let total = turn_counts(counts, angle_deg);
            let (steps, ms) = (e.p180_steps, e.p180_ms);
            self.edit(move |c| {
                let p = c.current_profile_mut();
                p.script = p180_lua_inject(&p.script, &p180_lua_snippet(g, total, steps, ms));
            });
            self.say(format!("Lua 180 bound to G{g}"));
        }
        // While the script actually holds a managed block, a REMOVE link sits
        // under the button (same edit -> ConfigChanged -> lua_sync path).
        if cfg.current().1.script.contains(P180_LUA_OPEN) {
            let rm = Rect::from_min_size(pos2(x + 254.0 - 56.0, ly + 30.0), vec2(56.0, 14.0));
            let rr = click(ui, rm, "p180-lua-remove");
            let col = if rr.hovered() { pal().primary_hover } else { pal().muted };
            text(ui, pos2(x + 254.0, rm.center().y), Align2::RIGHT_CENTER, "REMOVE", Ts::new(Fam::Bold, 9.5, col).ls(0.4));
            if rr.on_hover_cursor(egui::CursorIcon::PointingHand).clicked() {
                self.edit(|c| {
                    let p = c.current_profile_mut();
                    p.script = p180_lua_remove(&p.script);
                });
                self.say("Lua 180 removed");
            }
        }

        let br = Rect::from_min_size(pos2(x, ly + LUA_H), vec2(254.0, 26.0));
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

        // HARDWARE FLICK: the on-board variant. A tuned DPI shift plus a
        // physical swipe covers 180° — real sensor movement, so games that
        // drop injected input (and their anti-cheat) still accept it.
        let fy = br.bottom() + 72.0;
        text(ui, pos2(x, fy), Align2::LEFT_CENTER, "HARDWARE FLICK", t_small_label());
        para(ui, pos2(x, fy + 16.0), 254.0, "A real swipe = 180° — works in every game, even ones that ignore the button press.", Ts::new(Fam::Reg, 10.0, pal().muted).ls(0.3));
        text(ui, pos2(x, fy + 46.0), Align2::LEFT_CENTER, "SWIPE", t_small_label().color(pal().label));
        let fr = Rect::from_min_size(pos2(x + 60.0, fy + 40.0), vec2(140.0, 12.0));
        if slider(ui, fr, &mut self.asg.flick_cm, 1.0..=6.0, "flick-cm") {
            self.asg.flick_cm = (self.asg.flick_cm * 2.0).round() / 2.0;
        }
        text(ui, pos2(box_r.right() - 12.0, fy + 46.0), Align2::RIGHT_CENTER, &format!("{:.1} cm", self.asg.flick_cm), t_body_bold());
        let ab = Rect::from_min_size(pos2(x, fy + 66.0), vec2(254.0, 26.0));
        if let Some(d) = flick_dpi(counts, self.asg.flick_cm) {
            if dark_button(ui, ab, &format!("SET SHIFT DPI TO {}", thousands(d as i64)), "flick-apply", true).clicked() {
                let mut res = FlickLevel::Added;
                self.edit(|c| res = set_flick_dpi(&mut c.current_profile_mut().dpi, d));
                let d = thousands(d as i64);
                self.say(match res {
                    FlickLevel::Added => format!("Shift DPI {d} — added as a new stage"),
                    FlickLevel::Existing => format!("Shift DPI {d}"),
                    FlickLevel::Replaced => format!("Shift DPI {d} — replaced the last stage"),
                });
            }
        } else {
            let raw = (counts as f32 * 2.54 / self.asg.flick_cm).round() as i64;
            para(ui, pos2(x, ab.top() + 4.0), 254.0, &format!("Needs {raw} DPI — outside 100–25,600. Give yourself a longer swipe or a higher sensitivity."), Ts::new(Fam::Reg, 10.0, pal().amber).ls(0.3));
        }
        para(ui, pos2(x, fy + 98.0), 254.0, "Then assign DPI SHIFT (SYSTEM) to a button and save to a slot — the flick works with no software running.", Ts::new(Fam::Reg, 10.0, pal().muted).ls(0.3));
        ui.add_space(378.0 + extra + warn_h + sender_h + LUA_H + FLICK_H + read_h);
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
    // Widget-level focus only: has_focus() also needs the OS window focused, which
    // flaps in scripted/off-screen windows and would wipe the text mid-edit.
    if !ui.memory(|m| m.has_focus(resp.id)) {
        *buf = current.to_string();
    }
    out
}

/// 1234567 -> "1,234,567".
fn thousands(n: i64) -> String {
    let d = n.abs().to_string();
    let mut out = String::new();
    for (i, c) in d.chars().enumerate() {
        if i > 0 && (d.len() - i).is_multiple_of(3) {
            out.push(',');
        }
        out.push(c);
    }
    if n < 0 { format!("-{out}") } else { out }
}
