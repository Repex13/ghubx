//! G HUB's "DLS" design tokens live in `palette.rs`; this file has
//! text helpers with letter-spacing, painter icons and the small widgets
//! G HUB uses (pills, toggles, checkboxes, radios, sliders, tabs).

use egui::text::{LayoutJob, TextFormat};
use egui::{Align2, Color32, CornerRadius, FontFamily, FontId, Galley, Pos2, Rect, Response, Sense, Shape, Stroke, StrokeKind, Ui, Vec2, pos2, vec2};
use std::sync::Arc;

pub use super::palette::*;


/// Font roles. Brown Pro / Brown Logitech Pan when G HUB is installed.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Fam {
    Reg,
    Bold,
    Light,
    Pan,
    PanBold,
}

impl Fam {
    pub fn family(self) -> FontFamily {
        FontFamily::Name(
            match self {
                Fam::Reg => "reg",
                Fam::Bold => "bold",
                Fam::Light => "light",
                Fam::Pan => "pan",
                Fam::PanBold => "panb",
            }
            .into(),
        )
    }
}

/// A text style: family, CSS px size, colour, letter spacing (px).
#[derive(Clone, Copy, Debug)]
pub struct Ts {
    pub fam: Fam,
    pub size: f32,
    pub color: Color32,
    pub ls: f32,
}

impl Ts {
    pub const fn new(fam: Fam, size: f32, color: Color32) -> Self {
        Self { fam, size, color, ls: 1.0 }
    }
    pub const fn ls(mut self, ls: f32) -> Self {
        self.ls = ls;
        self
    }
    pub const fn color(mut self, c: Color32) -> Self {
        self.color = c;
        self
    }
    pub fn font(&self) -> FontId {
        FontId::new(self.size, self.fam.family())
    }
}

// Common styles measured from G HUB's computed styles.
pub fn t_device() -> Ts { Ts::new(Fam::Bold, 34.0, pal().text).ls(0.0) }
pub fn t_panel_title() -> Ts { Ts::new(Fam::Reg, 24.0, pal().text) }
pub fn t_tab() -> Ts { Ts::new(Fam::Reg, 14.0, pal().text) }
pub fn t_section() -> Ts { Ts::new(Fam::Reg, 14.0, pal().label) }
pub fn t_small_label() -> Ts { Ts::new(Fam::Reg, 12.0, pal().label) }
pub fn t_body() -> Ts { Ts::new(Fam::Reg, 12.0, pal().text_high) }
pub fn t_body_bold() -> Ts { Ts::new(Fam::Bold, 12.0, pal().text) }
pub fn t_list() -> Ts { Ts::new(Fam::Light, 12.0, pal().muted) }
pub fn t_callout() -> Ts { Ts::new(Fam::Reg, 14.0, pal().text) }
pub fn t_nav() -> Ts { Ts::new(Fam::Pan, 14.0, pal().nav).ls(-0.42) }
pub fn t_button() -> Ts { Ts::new(Fam::Reg, 12.0, pal().text) }

pub fn galley(ui: &Ui, text: &str, ts: Ts, wrap: f32) -> Arc<Galley> {
    let mut job = LayoutJob::single_section(
        text.to_owned(),
        TextFormat { font_id: ts.font(), color: ts.color, extra_letter_spacing: ts.ls, ..Default::default() },
    );
    job.wrap.max_width = wrap;
    ui.painter().layout_job(job)
}

/// Paint text anchored at `pos`; returns its rect.
pub fn text(ui: &Ui, pos: Pos2, anchor: Align2, s: &str, ts: Ts) -> Rect {
    let g = galley(ui, s, ts, f32::INFINITY);
    let r = anchor.anchor_size(pos, g.size());
    ui.painter().galley(r.min, g, ts.color);
    r
}

/// Wrapped paragraph at `pos` (top-left); returns its rect.
pub fn para(ui: &Ui, pos: Pos2, width: f32, s: &str, ts: Ts) -> Rect {
    let g = galley(ui, s, ts, width);
    let r = Rect::from_min_size(pos, g.size());
    ui.painter().galley(pos, g, ts.color);
    r
}

/// Paragraph made of differently coloured runs (e.g. "DPI SHIFT" in amber).
pub fn rich(ui: &Ui, pos: Pos2, width: f32, runs: &[(&str, Ts)]) -> Rect {
    let mut job = LayoutJob::default();
    for (s, ts) in runs {
        job.append(s, 0.0, TextFormat { font_id: ts.font(), color: ts.color, extra_letter_spacing: ts.ls, ..Default::default() });
    }
    job.wrap.max_width = width;
    let g = ui.painter().layout_job(job);
    let r = Rect::from_min_size(pos, g.size());
    ui.painter().galley(pos, g, pal().text);
    r
}

pub fn hit(ui: &Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug, sense: Sense) -> Response {
    ui.interact(rect, ui.scope_id().with(id), sense)
}

pub fn click(ui: &Ui, rect: Rect, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    hit(ui, rect, id, Sense::click()).on_hover_cursor(egui::CursorIcon::PointingHand)
}

// ------------------------------------------------------------------ widgets

pub fn dark_button(ui: &Ui, rect: Rect, label: &str, id: impl std::hash::Hash + std::fmt::Debug, enabled: bool) -> Response {
    let r = click(ui, rect, id);
    let fill = if !enabled {
        pal().n(0x2b)
    } else if r.is_pointer_button_down_on() {
        pal().n(0x30)
    } else if r.hovered() {
        pal().n(0x46)
    } else {
        pal().button
    };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    text(ui, rect.center(), Align2::CENTER_CENTER, label, t_button().color(if enabled { pal().text } else { pal().disabled }));
    r
}

pub fn blue_button(ui: &Ui, rect: Rect, label: &str, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let r = click(ui, rect, id);
    let fill = if r.hovered() { pal().primary_hover } else { pal().primary };
    ui.painter().rect_filled(rect, CornerRadius::same(4), fill);
    text(ui, rect.center(), Align2::CENTER_CENTER, label, Ts::new(Fam::Bold, 12.0, pal().on_primary));
    r
}

/// Outlined/filled capsule (VIEW 1 / VIEW 2).
pub fn pill(ui: &Ui, rect: Rect, label: &str, selected: bool, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    pill_sized(ui, rect, label, selected, 12.0, id)
}

pub fn pill_sized(ui: &Ui, rect: Rect, label: &str, selected: bool, size: f32, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let r = click(ui, rect, id);
    let p = ui.painter();
    if selected {
        p.rect_filled(rect, CornerRadius::same(100), pal().invert);
    } else {
        let c = if r.hovered() { pal().text } else { pal().text_high };
        p.rect_stroke(rect, CornerRadius::same(100), Stroke::new(1.5, c), StrokeKind::Inside);
    }
    text(ui, rect.center(), Align2::CENTER_CENTER, label, Ts::new(Fam::Bold, size, if selected { pal().bg } else { pal().text }).ls(0.0));
    r
}

/// G HUB switch. `on_color` is blue for settings, red for G-SHIFT.
pub fn switch(ui: &Ui, center: Pos2, on: bool, on_color: Color32, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    switch_sized(ui, center, on, on_color, 1.0, id)
}

pub fn switch_sized(ui: &Ui, center: Pos2, on: bool, on_color: Color32, k: f32, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let rect = Rect::from_center_size(center, vec2(36.0, 20.0) * k);
    let r = click(ui, rect, id);
    let t = ui.ctx().animate_bool_with_time(r.id, on, 0.12);
    let p = ui.painter();
    let track = Rect::from_center_size(center, vec2(34.0, 18.0) * k);
    let off = pal().track;
    let half = track.height() / 2.0;
    p.rect_filled(track, CornerRadius::same(half as u8), lerp_color(off, on_color, t));
    let knob_x = egui::lerp(track.left() + half..=track.right() - half, t);
    p.circle_filled(pos2(knob_x, center.y), half - 2.0, if on && on_color == pal().primary { pal().on_primary } else if on || k <= 1.0 { Color32::WHITE } else { pal().n(0x9a) });
    r
}

/// G HUB "ON/OFF" capsule toggle (on-board memory mode).
pub fn onoff(ui: &Ui, center: Pos2, on: bool, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let rect = Rect::from_center_size(center, vec2(48.0, 24.0));
    let r = click(ui, rect, id);
    let p = ui.painter();
    if on {
        p.rect_filled(rect, CornerRadius::same(100), pal().primary);
    } else {
        p.rect_stroke(rect, CornerRadius::same(100), Stroke::new(1.5, pal().text), StrokeKind::Inside);
    }
    text(ui, rect.center(), Align2::CENTER_CENTER, if on { "ON" } else { "OFF" }, Ts::new(Fam::Bold, 12.0, if on { pal().on_primary } else { pal().text }).ls(0.0));
    r
}

pub fn checkbox(ui: &Ui, pos: Pos2, label: &str, on: bool, enabled: bool, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let box_r = Rect::from_min_size(pos, vec2(14.0, 14.0));
    let lr = text(ui, pos2(pos.x + 24.0, pos.y + 7.0), Align2::LEFT_CENTER, label, Ts::new(Fam::Bold, 12.0, if enabled { pal().text } else { pal().disabled }));
    let r = click(ui, box_r.union(lr).expand(3.0), id);
    let p = ui.painter();
    let c = if enabled { pal().text } else { pal().disabled };
    if on {
        p.rect_filled(box_r, CornerRadius::same(2), if enabled { pal().n(0x9a) } else { pal().disabled });
        check(p, box_r.center(), 4.5, Stroke::new(1.8, pal().bg));
    } else {
        p.rect_stroke(box_r, CornerRadius::same(2), Stroke::new(1.5, c), StrokeKind::Inside);
    }
    r
}

pub fn radio(ui: &Ui, center: Pos2, on: bool, id: impl std::hash::Hash + std::fmt::Debug) -> Response {
    let r = click(ui, Rect::from_center_size(center, vec2(18.0, 18.0)), id);
    if on {
        ui.painter().circle_filled(center, 5.0, pal().text);
    } else {
        let c = if r.hovered() { pal().text } else { pal().n(0x85) };
        ui.painter().circle_stroke(center, 4.5, Stroke::new(1.5, c));
    }
    r
}

/// Row of text tabs with an underline under the selected one.
/// Returns the clicked index.
pub fn tabs(ui: &Ui, origin: Pos2, items: &[&str], selected: usize, gap: f32, id: &str) -> Option<usize> {
    let mut x = origin.x;
    let mut out = None;
    for (i, s) in items.iter().enumerate() {
        let sel = i == selected;
        let g = galley(ui, s, t_tab(), f32::INFINITY);
        let rect = Rect::from_min_size(pos2(x, origin.y), vec2(g.size().x, 22.0));
        let r = click(ui, rect.expand2(vec2(4.0, 2.0)), (id, i));
        let c = if sel || r.hovered() { pal().text } else { pal().text_medium };
        ui.painter().galley(pos2(x, origin.y + 2.0), galley(ui, s, t_tab().color(c), f32::INFINITY), c);
        if sel {
            ui.painter().line_segment([pos2(rect.left(), rect.bottom()), pos2(rect.right() - 1.0, rect.bottom())], Stroke::new(1.0, pal().text));
        }
        if r.clicked() {
            out = Some(i);
        }
        x += g.size().x + gap;
    }
    out
}

/// G HUB's thin slider. Returns true when the value changed.
pub fn slider(ui: &Ui, rect: Rect, value: &mut f32, range: std::ops::RangeInclusive<f32>, id: impl std::hash::Hash + std::fmt::Debug) -> bool {
    let r = hit(ui, rect.expand2(vec2(6.0, 8.0)), id, Sense::click_and_drag());
    let (lo, hi) = (*range.start(), *range.end());
    let mut changed = false;
    if (r.dragged() || r.clicked())
        && let Some(p) = r.interact_pointer_pos()
    {
        let t = ((p.x - rect.left()) / rect.width()).clamp(0.0, 1.0);
        let v = lo + t * (hi - lo);
        if (v - *value).abs() > f32::EPSILON {
            *value = v;
            changed = true;
        }
    }
    let t = ((*value - lo) / (hi - lo)).clamp(0.0, 1.0);
    let y = rect.center().y;
    let x = rect.left() + t * rect.width();
    let p = ui.painter();
    p.line_segment([pos2(rect.left(), y), pos2(rect.right(), y)], Stroke::new(2.0, pal().track));
    p.line_segment([pos2(rect.left(), y), pos2(x, y)], Stroke::new(2.0, pal().n(0xd0)));
    let knob = if r.hovered() || r.dragged() { pal().text } else { pal().n(0xe6) };
    p.circle_filled(pos2(x, y), 6.0, knob);
    changed
}

pub fn lerp_color(a: Color32, b: Color32, t: f32) -> Color32 {
    let l = |x: u8, y: u8| (x as f32 + (y as f32 - x as f32) * t).round() as u8;
    Color32::from_rgba_unmultiplied(l(a.r(), b.r()), l(a.g(), b.g()), l(a.b(), b.b()), l(a.a(), b.a()))
}

// ------------------------------------------------------------------ icons

pub fn check(p: &egui::Painter, c: Pos2, s: f32, st: Stroke) {
    p.add(Shape::line(vec![c + vec2(-s, 0.0), c + vec2(-s * 0.3, s * 0.7), c + vec2(s, -s * 0.7)], st));
}

pub fn chevron(p: &egui::Painter, c: Pos2, s: f32, up: bool, st: Stroke) {
    let d = if up { -1.0 } else { 1.0 };
    p.add(Shape::line(vec![c + vec2(-s, -s * 0.5 * d), c + vec2(0.0, s * 0.5 * d), c + vec2(s, -s * 0.5 * d)], st));
}

pub fn arrow_back(p: &egui::Painter, c: Pos2, st: Stroke) {
    p.line_segment([c + vec2(-10.0, 0.0), c + vec2(10.0, 0.0)], st);
    p.add(Shape::line(vec![c + vec2(-3.5, -7.0), c + vec2(-10.5, 0.0), c + vec2(-3.5, 7.0)], st));
}

fn arc(c: Pos2, r: f32, a0: f32, a1: f32, n: usize) -> Vec<Pos2> {
    (0..=n).map(|i| {
        let a = a0 + (a1 - a0) * i as f32 / n as f32;
        c + vec2(a.cos(), a.sin()) * r
    }).collect()
}

/// Logitech G mark.
pub fn logo_g(p: &egui::Painter, c: Pos2, s: f32, color: Color32) {
    use std::f32::consts::PI;
    let w = s * 0.24;
    let r = s * 0.5 - w * 0.5;
    p.add(Shape::line(arc(c, r, -PI * 0.28, PI * 1.5, 40), Stroke::new(w, color)));
    let y = c.y + w * 0.1;
    p.line_segment([pos2(c.x - w * 0.1, y), pos2(c.x + r + w * 0.5, y)], Stroke::new(w, color));
    p.line_segment([pos2(c.x + r, y - w * 0.4), pos2(c.x + r, c.y + r * 0.55)], Stroke::new(w, color));
}

pub fn gear(p: &egui::Painter, c: Pos2, s: f32, st: Stroke) {
    use std::f32::consts::TAU;
    let n = 8;
    let (ro, ri) = (s * 0.5, s * 0.38);
    let mut pts = Vec::new();
    for i in 0..n {
        let a = TAU * i as f32 / n as f32;
        let hw = TAU / n as f32 * 0.2;
        for (da, r) in [(-hw * 1.6, ri), (-hw, ro), (hw, ro), (hw * 1.6, ri)] {
            pts.push(c + vec2((a + da).cos(), (a + da).sin()) * r);
        }
    }
    p.add(Shape::closed_line(pts, st));
    p.circle_stroke(c, s * 0.16, st);
}

/// Notification bell: a stroked dome with flared base and a clapper dot.
pub fn bell(p: &egui::Painter, c: Pos2, s: f32, st: Stroke) {
    use std::f32::consts::{PI, TAU};
    let mut pts = vec![c + vec2(-s * 0.42, s * 0.28), c + vec2(-s * 0.28, -s * 0.10)];
    pts.extend(arc(c + vec2(0.0, -s * 0.10), s * 0.28, PI, TAU, 16));
    pts.push(c + vec2(s * 0.42, s * 0.28));
    p.add(Shape::closed_line(pts, st));
    p.circle_filled(c + vec2(0.0, s * 0.40), s * 0.07, st.color);
}

pub fn sun(p: &egui::Painter, c: Pos2, s: f32, st: Stroke) {
    use std::f32::consts::TAU;
    p.circle_stroke(c, s * 0.2, st);
    for i in 0..8 {
        let a = TAU * i as f32 / 8.0;
        let d = vec2(a.cos(), a.sin());
        p.line_segment([c + d * s * 0.36, c + d * s * 0.5], st);
    }
}

pub fn plus_square(p: &egui::Painter, c: Pos2, s: f32, st: Stroke) {
    let r = Rect::from_center_size(c, vec2(s, s));
    p.rect_stroke(r, CornerRadius::same((s * 0.18) as u8), st, StrokeKind::Middle);
    let h = s * 0.28;
    p.line_segment([c + vec2(-h, 0.0), c + vec2(h, 0.0)], st);
    p.line_segment([c + vec2(0.0, -h), c + vec2(0.0, h)], st);
}

/// Sensitivity: a dot with four outward chevrons.
pub fn dpi_icon(p: &egui::Painter, c: Pos2, s: f32, st: Stroke) {
    p.circle_filled(c, s * 0.1, st.color);
    let d = s * 0.42;
    let k = s * 0.13;
    for (dir, perp) in [(vec2(0.0, -1.0), vec2(1.0, 0.0)), (vec2(0.0, 1.0), vec2(1.0, 0.0)), (vec2(-1.0, 0.0), vec2(0.0, 1.0)), (vec2(1.0, 0.0), vec2(0.0, 1.0))] {
        let tip = c + dir * d;
        p.add(Shape::line(vec![tip - dir * k + perp * k, tip, tip - dir * k - perp * k], st));
    }
}

pub fn search(p: &egui::Painter, c: Pos2, st: Stroke) {
    p.circle_stroke(c + vec2(-1.5, -1.5), 5.0, st);
    p.line_segment([c + vec2(2.0, 2.0), c + vec2(6.0, 6.0)], st);
}

/// Profile selector glyph: a monitor with a padlock.
pub fn profile_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    let st = Stroke::new(1.6, color);
    let m = Rect::from_center_size(c + vec2(-2.0, -1.0), vec2(16.0, 12.0));
    p.rect_stroke(m, CornerRadius::same(1), st, StrokeKind::Middle);
    p.line_segment([m.left_top() + vec2(3.0, 4.0), m.left_top() + vec2(9.0, 4.0)], st);
    p.line_segment([m.left_top() + vec2(3.0, 7.5), m.left_top() + vec2(7.0, 7.5)], st);
    let lk = Rect::from_center_size(c + vec2(6.5, 4.5), vec2(8.0, 6.0));
    p.rect_filled(lk.expand(1.5), CornerRadius::same(1), pal().surface);
    p.rect_filled(lk, CornerRadius::same(1), color);
    p.add(Shape::line(arc(lk.center_top(), 2.4, std::f32::consts::PI, std::f32::consts::TAU, 8), Stroke::new(1.4, color)));
}

pub fn chip(p: &egui::Painter, c: Pos2, color: Color32) {
    let st = Stroke::new(1.5, color);
    let r = Rect::from_center_size(c, vec2(12.0, 12.0));
    p.rect_stroke(r, CornerRadius::same(1), st, StrokeKind::Middle);
    p.rect_stroke(r.shrink(3.5), CornerRadius::ZERO, Stroke::new(1.2, color), StrokeKind::Middle);
    for i in 0..3 {
        let o = -3.5 + i as f32 * 3.5;
        p.line_segment([c + vec2(o, -6.0), c + vec2(o, -9.0)], st);
        p.line_segment([c + vec2(o, 6.0), c + vec2(o, 9.0)], st);
        p.line_segment([c + vec2(-6.0, o), c + vec2(-9.0, o)], st);
        p.line_segment([c + vec2(6.0, o), c + vec2(9.0, o)], st);
    }
}

pub fn burst(p: &egui::Painter, c: Pos2, color: Color32) {
    use std::f32::consts::TAU;
    p.circle_stroke(c, 3.0, Stroke::new(1.5, color));
    for i in 0..12 {
        let a = TAU * i as f32 / 12.0;
        p.circle_filled(c + vec2(a.cos(), a.sin()) * 8.0, 1.1, color);
    }
}

pub fn diamond(p: &egui::Painter, c: Pos2, r: f32, color: Color32) {
    p.add(Shape::convex_polygon(vec![c + vec2(0.0, -r), c + vec2(r, 0.0), c + vec2(0.0, r), c + vec2(-r, 0.0)], color, Stroke::NONE));
}

pub fn info(p: &egui::Painter, c: Pos2, color: Color32) {
    p.circle_stroke(c, 7.0, Stroke::new(1.3, color));
    p.line_segment([c + vec2(0.0, -1.0), c + vec2(0.0, 3.5)], Stroke::new(1.5, color));
    p.circle_filled(c + vec2(0.0, -3.5), 1.0, color);
}

pub fn grid_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    for (dx, dy) in [(-4.0, -4.0), (4.0, -4.0), (-4.0, 4.0), (4.0, 4.0)] {
        p.circle_stroke(c + vec2(dx, dy), 2.6, Stroke::new(1.5, color));
    }
}

pub fn list_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    for dy in [-3.5, 3.5] {
        p.rect_stroke(Rect::from_center_size(c + vec2(0.0, dy), vec2(14.0, 5.0)), CornerRadius::same(3), Stroke::new(1.5, color), StrokeKind::Middle);
    }
}

pub fn user_icon(p: &egui::Painter, c: Pos2, color: Color32) {
    let st = Stroke::new(1.6, color);
    p.circle_stroke(c + vec2(0.0, -4.0), 4.0, st);
    p.add(Shape::line(arc(c + vec2(0.0, 9.0), 8.0, std::f32::consts::PI * 1.05, std::f32::consts::PI * 1.95, 16), st));
    p.line_segment([c + vec2(-8.0, 8.0), c + vec2(8.0, 8.0)], st);
}

/// Window caption buttons (minimise / maximise / close).
pub fn caption_glyph(p: &egui::Painter, c: Pos2, kind: u8, color: Color32) {
    let st = Stroke::new(1.0, color);
    match kind {
        0 => {
            p.line_segment([c + vec2(-5.0, 0.0), c + vec2(5.0, 0.0)], st);
        }
        1 => {
            p.rect_stroke(Rect::from_center_size(c, vec2(9.0, 9.0)), CornerRadius::ZERO, st, StrokeKind::Middle);
        }
        _ => {
            p.line_segment([c + vec2(-5.0, -5.0), c + vec2(5.0, 5.0)], st);
            p.line_segment([c + vec2(-5.0, 5.0), c + vec2(5.0, -5.0)], st);
        }
    }
}

pub fn keycap(ui: &Ui, rect: Rect, label: &str, ts: Ts) {
    ui.painter().rect_stroke(rect, CornerRadius::same(3), Stroke::new(1.2, ts.color), StrokeKind::Inside);
    text(ui, rect.center(), Align2::CENTER_CENTER, label, ts);
}

/// The "M" badge of a macro; a macro with a colour fills it with that colour.
pub fn macro_badge(ui: &Ui, rect: Rect, color: Option<[u8; 3]>) {
    let (fill, ink) = match color {
        Some(c) => {
            let f = Color32::from_rgb(c[0], c[1], c[2]);
            let lum = |x: Color32| 0.2126 * x.r() as f32 + 0.7152 * x.g() as f32 + 0.0722 * x.b() as f32;
            let (a, b) = (pal().bg, pal().text);
            (f, if (lum(f) - lum(a)).abs() > (lum(f) - lum(b)).abs() { a } else { b })
        }
        None => (pal().n(0x6e), pal().bg),
    };
    ui.painter().rect_filled(rect, CornerRadius::same(2), fill);
    text(ui, rect.center() + vec2(0.0, 0.5), Align2::CENTER_CENTER, "M", Ts::new(Fam::Bold, 10.0, ink).ls(0.0));
}

pub fn soft_shadow(ui: &Ui, rect: Rect, r: u8) {
    for (i, a) in [(8.0, 18u8), (4.0, 28), (2.0, 40)] {
        ui.painter().rect_filled(rect.expand(i), CornerRadius::same(r + i as u8), Color32::from_black_alpha(a));
    }
}

pub fn v(x: f32, y: f32) -> Vec2 {
    vec2(x, y)
}
