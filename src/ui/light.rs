//! LIGHTSYNC: PRIMARY / LOGO zones, effect, colour picker, rate and
//! brightness, with a live glow preview on the render.

use super::device::{Placed, draw_render};
use super::theme::*;
use super::{GhubApp, Overlay};
use crate::profile::*;
use egui::ecolor::Hsva;
use egui::{Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

pub const EFFECTS: [(&str, Effect); 4] = [("FIXED", Effect::Fixed), ("BREATHING", Effect::Breathing), ("CYCLE", Effect::Cycle), ("OFF", Effect::Off)];

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

#[derive(Default)]
pub struct LightState {
    pub zone: usize,
    hsv: Option<(f32, f32, f32)>,
    hsv_for: Option<[u8; 3]>,
    hex: String,
}

fn effect_name(e: Effect) -> &'static str {
    EFFECTS.iter().find(|(_, x)| *x == e).map(|(n, _)| *n).unwrap_or("?")
}

/// Colour the zone shows at time `t` (seconds) — for the preview glow.
pub fn preview_color(l: &Lighting, t: f64) -> Option<Color32> {
    let k = l.brightness as f32 / 100.0;
    let period = (l.period_ms.max(500) as f64) / 1000.0;
    let c = match l.effect {
        Effect::Off => return None,
        Effect::Fixed => Color32::from_rgb(l.color[0], l.color[1], l.color[2]),
        Effect::Breathing => {
            let a = (0.5 - 0.5 * (t / period * std::f64::consts::TAU).cos()) as f32;
            Color32::from_rgb(l.color[0], l.color[1], l.color[2]).gamma_multiply(0.15 + 0.85 * a)
        }
        Effect::Cycle => {
            let [r, g, b] = Hsva::new(((t / period) % 1.0) as f32, 1.0, 1.0, 1.0).to_srgb();
            Color32::from_rgb(r, g, b)
        }
    };
    Some(c.gamma_multiply(k))
}

impl GhubApp {
    pub(crate) fn lighting(&mut self, ui: &mut Ui, panel: Rect, content: Rect) {
        let cfg = self.cfg();
        let prof = cfg.current().1.clone();
        let x0 = panel.left() + 16.0;
        let top = panel.top();
        text(ui, pos2(x0, top + 17.0), Align2::LEFT_TOP, "LIGHTSYNC", T_PANEL_TITLE);

        let info = Rect::from_min_size(pos2(x0 + 12.0, top + 64.0), vec2(266.0, 72.0));
        ui.painter().rect_filled(info, CornerRadius::same(8), Color32::from_rgb(0x2a, 0x2b, 0x30));
        burst(ui.painter(), pos2(info.left() + 22.0, info.top() + 22.0), PRIMARY);
        text(ui, pos2(info.left() + 42.0, info.top() + 22.0), Align2::LEFT_CENTER, "DEVICE LIGHTING", Ts::new(Fam::Bold, 12.0, PRIMARY));
        para(ui, pos2(info.left() + 14.0, info.top() + 38.0), 240.0, "If Windows Dynamic Lighting controls this mouse it can override these effects.", Ts::new(Fam::Reg, 11.0, HIGH).ls(0.4));

        if let Some(z) = tabs(ui, pos2(x0, top + 216.0), &ZONE_NAMES, self.light.zone, 16.0, "zones") {
            self.light.zone = z;
        }
        let zone = self.light.zone;
        let l = prof.lighting[zone];
        text(ui, pos2(x0, top + 264.0), Align2::LEFT_TOP, "EFFECT", T_SECTION);
        let er = text(ui, pos2(x0, top + 302.0), Align2::LEFT_CENTER, effect_name(l.effect), Ts::new(Fam::Light, 12.0, WHITE).ls(1.0));
        chevron(ui.painter(), pos2(er.right() + 12.0, er.center().y), 4.0, false, Stroke::new(1.5, WHITE));
        let eresp = click(ui, Rect::from_min_max(er.min - vec2(4.0, 6.0), pos2(er.right() + 22.0, er.bottom() + 6.0)), "effect-dd");
        if eresp.clicked() {
            self.open_overlay(ui.ctx(), Overlay::Effect(eresp.rect));
        }

        let mut y = top + 338.0;
        let mut new_l = l;
        if matches!(l.effect, Effect::Fixed | Effect::Breathing) {
            if let Some(c) = self.color_picker(ui, pos2(x0, y), l.color) {
                new_l.color = c;
            }
            y += 196.0;
        }
        if matches!(l.effect, Effect::Breathing | Effect::Cycle) {
            text(ui, pos2(x0, y), Align2::LEFT_TOP, "EFFECT RATE", Ts::new(Fam::Reg, 12.0, LABEL));
            let mut v = l.period_ms as f32;
            if slider(ui, Rect::from_min_size(pos2(x0, y + 26.0), vec2(180.0, 12.0)), &mut v, 1000.0..=20000.0, ("rate", zone)) {
                new_l.period_ms = ((v / 100.0).round() * 100.0) as u16;
            }
            value_label(ui, pos2(panel.right() - 16.0, y + 32.0), &format!("{}", new_l.period_ms), "ms");
            y += 65.0;
        }
        if l.effect != Effect::Off {
            text(ui, pos2(x0, y), Align2::LEFT_TOP, "EFFECT BRIGHTNESS", Ts::new(Fam::Reg, 12.0, LABEL));
            let mut v = l.brightness as f32;
            if slider(ui, Rect::from_min_size(pos2(x0, y + 26.0), vec2(180.0, 12.0)), &mut v, 1.0..=100.0, ("bright", zone)) {
                new_l.brightness = v.round() as u8;
            }
            value_label(ui, pos2(panel.right() - 16.0, y + 32.0), &format!("{}", new_l.brightness), "%");
            y += 79.0;
        } else {
            y += 8.0;
        }
        if dark_button(ui, Rect::from_min_size(pos2(x0, y), vec2(278.0, 32.0)), "SYNC LIGHTING ZONES", "sync-zones", true).clicked() {
            self.edit(|c| {
                let p = c.current_profile_mut();
                p.lighting[1 - zone] = p.lighting[zone];
            });
            self.say("Both zones now use the same effect");
        }
        if new_l != l {
            self.edit(|c| c.current_profile_mut().lighting[zone] = new_l);
        }

        // preview
        let win_h = ui.ctx().content_rect().height();
        let placed = Placed::new(self.meta.front.origin, content.center().x, win_h * 0.333, win_h * 0.465);
        draw_render(ui, self.tex.front.as_ref(), placed, WHITE);
        let t = ui.ctx().input(|i| i.time);
        let mut clicked_zone = None;
        for (zi, zr) in self.meta.front.zones.clone() {
            let r = Rect::from_min_max(placed.at(zr.min.x, zr.min.y), placed.at(zr.max.x, zr.max.y));
            if let Some(c) = preview_color(&prof.lighting[zi], t) {
                let g = Rect::from_center_size(r.center(), r.size() * 1.9);
                ui.painter().image(self.tex.glow.id(), g, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), c.gamma_multiply(0.55));
                ui.painter().image(self.tex.glow.id(), Rect::from_center_size(r.center(), r.size() * 1.2), Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), c);
            }
            let resp = click(ui, r.expand(6.0), ("zone-hit", zi));
            if zi == zone || resp.hovered() {
                ui.painter().rect_stroke(r.expand(6.0), CornerRadius::same(6), Stroke::new(1.0, if zi == zone { Color32::from_white_alpha(90) } else { Color32::from_white_alpha(40) }), StrokeKind::Outside);
            }
            if resp.clicked() {
                clicked_zone = Some(zi);
            }
            if resp.hovered() {
                super::device::tooltip(ui, pos2(r.right() + 8.0, r.center().y), ZONE_NAMES[zi]);
            }
        }
        if let Some(z) = clicked_zone {
            self.light.zone = z;
        }
        if prof.lighting.iter().any(|l| matches!(l.effect, Effect::Breathing | Effect::Cycle)) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(33));
        }
    }

    /// Saturation/value square + hue bar + swatches + hex. Returns a new colour.
    fn color_picker(&mut self, ui: &mut Ui, origin: Pos2, color: [u8; 3]) -> Option<[u8; 3]> {
        if self.light.hsv_for != Some(color) {
            let h = Hsva::from_srgb(color);
            let keep_h = self.light.hsv.map(|x| x.0).unwrap_or(h.h);
            self.light.hsv = Some((if h.s > 0.001 && h.v > 0.001 { h.h } else { keep_h }, h.s, h.v));
            self.light.hsv_for = Some(color);
            self.light.hex = format!("{:02X}{:02X}{:02X}", color[0], color[1], color[2]);
        }
        let (mut h, mut s, mut v) = self.light.hsv.unwrap_or((0.55, 1.0, 1.0));
        let sv = Rect::from_min_size(origin, vec2(278.0, 118.0));
        let hue = Rect::from_min_size(pos2(origin.x, sv.bottom() + 12.0), vec2(278.0, 12.0));
        let pure = Hsva::new(h, 1.0, 1.0, 1.0).to_srgb();
        let pure = Color32::from_rgb(pure[0], pure[1], pure[2]);
        let mut m = Mesh::default();
        for (p, c) in [(sv.left_top(), WHITE), (sv.right_top(), pure), (sv.right_bottom(), Color32::BLACK), (sv.left_bottom(), Color32::BLACK)] {
            m.colored_vertex(p, c);
        }
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        ui.painter().add(Shape::mesh(m));
        let mut hm = Mesh::default();
        for i in 0..=12 {
            let t = i as f32 / 12.0;
            let c = Hsva::new(t, 1.0, 1.0, 1.0).to_srgb();
            let c = Color32::from_rgb(c[0], c[1], c[2]);
            let x = hue.left() + t * hue.width();
            hm.colored_vertex(pos2(x, hue.top()), c);
            hm.colored_vertex(pos2(x, hue.bottom()), c);
            if i > 0 {
                let b = (i * 2) as u32;
                hm.add_triangle(b - 2, b - 1, b);
                hm.add_triangle(b - 1, b, b + 1);
            }
        }
        ui.painter().add(Shape::mesh(hm));
        let mut changed = false;
        let r = hit(ui, sv, "sv", Sense::click_and_drag());
        if (r.dragged() || r.clicked())
            && let Some(p) = r.interact_pointer_pos()
        {
            s = ((p.x - sv.left()) / sv.width()).clamp(0.0, 1.0);
            v = 1.0 - ((p.y - sv.top()) / sv.height()).clamp(0.0, 1.0);
            changed = true;
        }
        let hr = hit(ui, hue.expand2(vec2(0.0, 4.0)), "hue", Sense::click_and_drag());
        if (hr.dragged() || hr.clicked())
            && let Some(p) = hr.interact_pointer_pos()
        {
            h = ((p.x - hue.left()) / hue.width()).clamp(0.0, 0.9999);
            changed = true;
        }
        let cur = pos2(sv.left() + s * sv.width(), sv.top() + (1.0 - v) * sv.height());
        ui.painter().circle_stroke(cur, 6.0, Stroke::new(2.0, WHITE));
        ui.painter().circle_stroke(cur, 7.5, Stroke::new(1.0, Color32::from_black_alpha(120)));
        let hx = hue.left() + h * hue.width();
        ui.painter().rect_stroke(Rect::from_center_size(pos2(hx, hue.center().y), vec2(6.0, 16.0)), CornerRadius::same(2), Stroke::new(2.0, WHITE), StrokeKind::Middle);

        // swatches + hex
        let sy = hue.bottom() + 22.0;
        let mut out = None;
        for (i, c) in SWATCHES.iter().enumerate() {
            let center = pos2(origin.x + 9.0 + i as f32 * 22.0, sy);
            let sr = click(ui, Rect::from_center_size(center, vec2(18.0, 18.0)), ("sw", i));
            ui.painter().circle_filled(center, 8.0, Color32::from_rgb(c[0], c[1], c[2]));
            if *c == color || sr.hovered() {
                ui.painter().circle_stroke(center, 10.5, Stroke::new(1.5, WHITE));
            }
            if sr.clicked() {
                out = Some(*c);
            }
        }
        let hr = Rect::from_min_size(pos2(origin.x + 206.0, sy - 12.0), vec2(72.0, 24.0));
        ui.painter().rect_stroke(hr, CornerRadius::same(4), Stroke::new(1.0, OUTLINE_HI), StrokeKind::Inside);
        text(ui, pos2(hr.left() + 8.0, hr.center().y), Align2::LEFT_CENTER, "#", Ts::new(Fam::Reg, 12.0, MUTED));
        let mut hex = self.light.hex.clone();
        let te = egui::TextEdit::singleline(&mut hex).frame(egui::Frame::NONE).char_limit(6).font(Ts::new(Fam::Reg, 12.0, WHITE).font()).text_color(WHITE);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(hr));
        let resp = child.put(Rect::from_min_max(pos2(hr.left() + 18.0, hr.top() + 4.0), hr.right_bottom() - vec2(4.0, 2.0)), te);
        if hex != self.light.hex {
            self.light.hex = hex.to_uppercase().chars().filter(|c| c.is_ascii_hexdigit()).collect();
        }
        if (resp.lost_focus() || ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)))
            && self.light.hex.len() == 6
            && let Ok(n) = u32::from_str_radix(&self.light.hex, 16)
        {
            let c = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            if c != color {
                out = Some(c);
            }
        }
        if changed {
            self.light.hsv = Some((h, s, v));
            let c = Hsva::new(h, s, v, 1.0).to_srgb();
            self.light.hsv_for = Some(c);
            self.light.hex = format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
            out = Some(c);
        }
        out
    }
}

fn value_label(ui: &Ui, right_mid: Pos2, value: &str, unit: &str) {
    let ur = text(ui, right_mid, Align2::RIGHT_CENTER, unit, Ts::new(Fam::Reg, 14.0, WHITE).ls(0.0));
    text(ui, pos2(ur.left(), right_mid.y), Align2::RIGHT_CENTER, value, Ts::new(Fam::Bold, 14.0, WHITE).ls(0.0));
}
