//! LIGHTSYNC: PRIMARY / LOGO zones, effect, colour picker, rate and
//! brightness, with a live glow preview on the render.

use super::device::{Placed, draw_render};
use super::theme::*;
use super::{GhubApp, Overlay};
use crate::profile::*;
use crate::fx::{hsv_to_rgb, rgb_to_hsv};
use egui::ecolor::Hsva;
use egui::{Align2, Color32, CornerRadius, Mesh, Pos2, Rect, Sense, Shape, Stroke, StrokeKind, Ui, pos2, vec2};

pub const EFFECTS: [(&str, Effect); 7] = [
    ("OFF", Effect::Off),
    ("FIXED", Effect::Fixed),
    ("CYCLE", Effect::Cycle),
    ("BREATHING", Effect::Breathing),
    ("SCREEN SAMPLER", Effect::Screen),
    ("AUDIO VISUALIZER", Effect::Audio),
    ("REACTIVE", Effect::Reactive),
];

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
    audio_target: usize,
    advanced: bool,
    picker: PickerState,
    samples_scroll: f32,
}

impl LightState {
    pub fn set_advanced(&mut self, on: bool) {
        self.advanced = on;
    }
}

/// State of one colour picker (SV square, hue bar and hex field).
#[derive(Default)]
pub struct PickerState {
    hsv: Option<(f32, f32, f32)>,
    hsv_for: Option<[u8; 3]>,
    hex: String,
}

pub(super) fn effect_name(e: Effect) -> &'static str {
    EFFECTS.iter().find(|(_, x)| *x == e).map(|(n, _)| *n).unwrap_or("?")
}

/// Colour the zone shows at time `t` (seconds) — for the preview glow.
/// `live` is the colour the fx thread currently computes for a host-driven effect.
pub fn preview_color(l: &Lighting, t: f64, live: Option<[u8; 3]>) -> Option<Color32> {
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
        Effect::Screen => live.map(|c| Color32::from_rgb(c[0], c[1], c[2])).unwrap_or(Color32::from_rgb(0x3A, 0x7B, 0xD5)),
        Effect::Audio | Effect::Reactive => live.map(|c| Color32::from_rgb(c[0], c[1], c[2])).unwrap_or(Color32::from_rgb(l.color[0], l.color[1], l.color[2]).gamma_multiply(0.6)),
    };
    if l.effect.is_host() && live.is_some() {
        return Some(c);
    }
    Some(c.gamma_multiply(k))
}

impl GhubApp {
    pub(crate) fn lighting(&mut self, ui: &mut Ui, panel: Rect, content: Rect) {
        let cfg = self.cfg();
        let prof = cfg.current().1.clone();
        let x0 = panel.left() + 16.0;
        let top = panel.top();
        text(ui, pos2(x0, top + 17.0), Align2::LEFT_TOP, "LIGHTSYNC", t_panel_title());

        let wdl = match self.wdl_cache {
            Some((at, v)) if at.elapsed().as_millis() < 1000 => v,
            _ => {
                let v = crate::win::wdl_state();
                self.wdl_cache = Some((std::time::Instant::now(), v));
                v
            }
        };
        let wdl_on = wdl == Some(true);
        let dy = if wdl_on { 0.0 } else { -96.0 };
        if let Some(on) = wdl {
            let ry = top + 78.0;
            burst(ui.painter(), pos2(x0 + 22.0, ry), pal().primary);
            let tr = text(ui, pos2(x0 + 42.0, ry), Align2::LEFT_CENTER, "WINDOWS SETTINGS", Ts::new(Fam::Bold, 14.0, pal().primary));
            if click(ui, tr.expand(4.0), "wdl-open").clicked() {
                crate::win::open_wdl_settings();
            }
            let qc = pos2(x0 + 215.0, ry);
            ui.painter().circle_stroke(qc, 8.0, Stroke::new(1.3, pal().label));
            text(ui, qc, Align2::CENTER_CENTER, "?", Ts::new(Fam::Bold, 11.0, pal().label));
            if hit(ui, Rect::from_center_size(qc, vec2(20.0, 20.0)), "wdl-help", Sense::hover()).hovered() {
                super::device::tooltip(ui, pos2(qc.x + 12.0, qc.y + 26.0), "Windows controls this device's lighting while this is on.");
            }
            if switch(ui, pos2(x0 + 254.0, ry), on, pal().primary, "wdl-switch").clicked() {
                crate::win::set_wdl(!on);
                self.wdl_cache = None;
            }
        }
        if wdl_on {
            let info = Rect::from_min_size(pos2(x0 + 12.0, top + 116.0), vec2(266.0, 72.0));
            ui.painter().rect_filled(info, CornerRadius::same(8), pal().surface_hi);
            para(ui, pos2(info.left() + 14.0, info.top() + 14.0), 240.0, "To configure LIGHTSYNC, disable Windows Settings above", Ts::new(Fam::Reg, 12.0, pal().text_high).ls(0.4));
        }

        if let Some(z) = tabs(ui, pos2(x0, top + 216.0 + dy), &ZONE_NAMES, self.light.zone, 16.0, "zones") {
            self.light.zone = z;
        }
        let zone = self.light.zone;
        let l = prof.lighting[zone];
        text(ui, pos2(x0, top + 264.0 + dy), Align2::LEFT_TOP, "EFFECT", t_section());
        let er = text(ui, pos2(x0, top + 302.0 + dy), Align2::LEFT_CENTER, effect_name(l.effect), Ts::new(Fam::Light, 12.0, pal().text).ls(1.0));
        chevron(ui.painter(), pos2(er.right() + 12.0, er.center().y), 4.0, false, Stroke::new(1.5, pal().text));
        let eresp = click(ui, Rect::from_min_max(er.min - vec2(4.0, 6.0), pos2(er.right() + 22.0, er.bottom() + 6.0)), "effect-dd");
        if eresp.clicked() && !wdl_on {
            self.open_overlay(ui.ctx(), Overlay::Effect(eresp.rect));
        }

        let mut y = top + 338.0 + dy;
        let mut new_l = l;
        if matches!(l.effect, Effect::Fixed | Effect::Breathing | Effect::Reactive) {
            if let Some(c) = Self::color_picker(ui, pos2(x0, y), l.color, &mut self.light.picker, "light", &SWATCHES) {
                new_l.color = c;
            }
            y += 196.0;
        }
        match l.effect {
            Effect::Screen => y = self.screen_panel(ui, x0, y, panel, zone, l, &mut new_l),
            Effect::Audio => y = self.audio_panel(ui, x0, y, panel, zone, l, &mut new_l),
            Effect::Reactive => y = self.reactive_panel(ui, x0, y, panel, zone, l, &mut new_l),
            _ => {}
        }
        if matches!(l.effect, Effect::Breathing | Effect::Cycle) {
            text(ui, pos2(x0, y), Align2::LEFT_TOP, "EFFECT RATE", Ts::new(Fam::Reg, 12.0, pal().label));
            let mut v = l.period_ms as f32;
            if slider(ui, Rect::from_min_size(pos2(x0, y + 26.0), vec2(180.0, 12.0)), &mut v, 1000.0..=20000.0, ("rate", zone)) {
                new_l.period_ms = ((v / 100.0).round() * 100.0) as u16;
            }
            value_label(ui, pos2(panel.right() - 16.0, y + 32.0), &format!("{}", new_l.period_ms), "ms");
            y += 65.0;
        }
        if l.effect.is_host() {
            if l.effect == Effect::Screen || l.effect == Effect::Reactive || self.light.advanced {
                let mut v = l.brightness as f32;
                if host_slider(ui, panel, x0, y, "EFFECT BRIGHTNESS", &mut v, 1.0..=100.0, ("bright", zone), true) {
                    new_l.brightness = v.round() as u8;
                }
                y += 46.0;
            }
        } else if l.effect != Effect::Off {
            text(ui, pos2(x0, y), Align2::LEFT_TOP, "EFFECT BRIGHTNESS", Ts::new(Fam::Reg, 12.0, pal().label));
            let mut v = l.brightness as f32;
            if slider(ui, Rect::from_min_size(pos2(x0, y + 26.0), vec2(180.0, 12.0)), &mut v, 1.0..=100.0, ("bright", zone)) {
                new_l.brightness = v.round() as u8;
            }
            value_label(ui, pos2(panel.right() - 16.0, y + 32.0), &format!("{}", new_l.brightness), "%");
            y += 79.0;
        } else {
            y += 8.0;
        }
        if dark_button(ui, Rect::from_min_size(pos2(x0, y), vec2(278.0, 32.0)), "SYNC LIGHTING ZONES", "sync-zones", true).clicked() && !wdl_on {
            self.edit(|c| {
                let p = c.current_profile_mut();
                p.lighting[1 - zone] = p.lighting[zone];
            });
            self.say("Both zones now use the same effect");
        }
        if wdl_on {
            let dim = Rect::from_min_max(pos2(panel.left() + 4.0, top + 200.0), pos2(panel.right() - 4.0, panel.bottom() - 4.0));
            ui.painter().rect_filled(dim, CornerRadius::same(6), pal().surface.gamma_multiply(0.62));
            hit(ui, dim, "wdl-block", Sense::click_and_drag());
        } else if new_l != l {
            self.edit(|c| c.current_profile_mut().lighting[zone] = new_l);
        }

        // preview
        let win_h = ui.ctx().content_rect().height();
        let placed = Placed::new(self.meta.front.origin, content.center().x, win_h * 0.333, win_h * 0.465);
        draw_render(ui, self.tex.front.as_ref(), placed, Color32::WHITE);
        let t = ui.ctx().input(|i| i.time);
        let mut clicked_zone = None;
        for (zi, zr) in self.meta.front.zones.clone() {
            let r = Rect::from_min_max(placed.at(zr.min.x, zr.min.y), placed.at(zr.max.x, zr.max.y));
            let lz = prof.lighting[zi];
            if lz.effect.is_host()
                && let Some(h) = &self.fx
            {
                h.preview_zone(zi, lz);
            }
            let live = if lz.effect.is_host() { self.fx.as_ref().and_then(|h| h.preview(zi)) } else { None };
            if let Some(c) = preview_color(&lz, t, live) {
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
        if prof.lighting.iter().any(|l| l.effect.is_host()) {
            ui.ctx().request_repaint_after(std::time::Duration::from_millis(50));
        }
    }

    /// SCREEN SAMPLER controls; returns the y where the next block starts.
    #[allow(clippy::too_many_arguments)]
    fn screen_panel(&mut self, ui: &mut Ui, x0: f32, y: f32, panel: Rect, zone: usize, l: Lighting, new_l: &mut Lighting) -> f32 {
        let th = pal();
        para(ui, pos2(x0, y), 278.0, "Choose an area of your screen below, your device lighting will update to match the area.", Ts::new(Fam::Reg, 11.0, th.label));
        text(ui, pos2(x0, y + 46.0), Align2::LEFT_TOP, "SAMPLE", Ts::new(Fam::Reg, 12.0, th.label));
        // EDIT on the SAMPLE header line, right-aligned: opens the full-window sample editor
        let eg = galley(ui, "EDIT", Ts::new(Fam::Bold, 12.0, th.text), f32::INFINITY);
        let er = Rect::from_center_size(pos2(panel.right() - 16.0 - eg.size().x / 2.0, y + 52.0), eg.size());
        let rr = click(ui, er.expand(4.0), "samples-edit");
        let col = if rr.hovered() { th.primary } else { th.text };
        ui.painter().galley(er.min, galley(ui, "EDIT", Ts::new(Fam::Bold, 12.0, col), f32::INFINITY), col);
        if rr.clicked() {
            let sel = self.cfg().samples.iter().position(|s| ScreenArea::Rect(s.rect) == l.host.area);
            self.sample_edit = Some(super::samples::SampleEditor::new(sel, l.host.monitor));
        }
        let scr = Rect::from_min_size(pos2(x0, y + 64.0), vec2(96.0, 54.0));
        let full = l.host.area == ScreenArea::Full;
        let inner = scr.shrink(1.0);
        let (cw, ch) = (inner.width() / 3.0, inner.height() / 3.0);
        for (i, a) in ScreenArea::GRID.iter().enumerate() {
            let cell = Rect::from_min_size(pos2(inner.left() + (i % 3) as f32 * cw, inner.top() + (i / 3) as f32 * ch), vec2(cw, ch));
            let r = click(ui, cell, ("area", i));
            let fill = if full {
                Some(th.primary_tint)
            } else if l.host.area == *a {
                Some(th.primary)
            } else if r.hovered() {
                Some(th.primary_tint)
            } else {
                None
            };
            if let Some(f) = fill {
                ui.painter().rect_filled(cell.shrink(0.5), CornerRadius::same(2), f);
            }
            if r.clicked() {
                new_l.host.area = *a;
            }
        }
        for k in 1..3 {
            let (gx, gy) = (inner.left() + k as f32 * cw, inner.top() + k as f32 * ch);
            ui.painter().line_segment([pos2(gx, inner.top()), pos2(gx, inner.bottom())], Stroke::new(1.0, th.outline_hi));
            ui.painter().line_segment([pos2(inner.left(), gy), pos2(inner.right(), gy)], Stroke::new(1.0, th.outline_hi));
        }
        ui.painter().rect_stroke(scr, CornerRadius::same(4), Stroke::new(1.0, if full { th.primary } else { th.outline_hi }), StrokeKind::Inside);
        if pill(ui, Rect::from_min_size(pos2(x0 + 112.0, y + 64.0), vec2(100.0, 22.0)), "FULL SCREEN", full, "area-full").clicked() {
            new_l.host.area = ScreenArea::Full;
        }
        let cfg = self.cfg();
        let name = cfg.sample_name(l.host.area).map(|s| s.to_uppercase());
        text(ui, pos2(x0 + 112.0, y + 98.0), Align2::LEFT_TOP, name.as_deref().unwrap_or(l.host.area.label()), Ts::new(Fam::Reg, 12.0, th.text));
        // the user's own samples as rows under the grid (in use: primary bar + text colour)
        let list_h = cfg.samples.len().min(4) as f32 * 24.0;
        if !cfg.samples.is_empty() {
            let lr = Rect::from_min_size(pos2(x0, y + 126.0), vec2(278.0, list_h));
            let max_scroll = (cfg.samples.len() as f32 * 24.0 - list_h).max(0.0);
            if ui.rect_contains_pointer(lr) {
                self.light.samples_scroll -= ui.input(|i| i.smooth_scroll_delta.y);
            }
            self.light.samples_scroll = self.light.samples_scroll.clamp(0.0, max_scroll);
            let scroll = self.light.samples_scroll;
            let mut sub = ui.new_child(egui::UiBuilder::new().max_rect(lr));
            sub.set_clip_rect(lr.intersect(ui.clip_rect()));
            for (i, s) in cfg.samples.iter().enumerate() {
                let rr = Rect::from_min_size(pos2(lr.left(), lr.top() + i as f32 * 24.0 - scroll), vec2(lr.width(), 24.0));
                if rr.bottom() <= lr.top() || rr.top() >= lr.bottom() {
                    continue;
                }
                let in_use = l.host.area == ScreenArea::Rect(s.rect);
                let r = click(&sub, rr, ("sample-row", i));
                if in_use {
                    sub.painter().rect_filled(Rect::from_min_size(rr.min, vec2(3.0, rr.height())), CornerRadius::ZERO, th.primary);
                }
                text(&sub, pos2(rr.left() + 12.0, rr.center().y), Align2::LEFT_CENTER, &s.name.to_uppercase(), Ts::new(Fam::Reg, 12.0, if in_use { th.text } else { th.muted }));
                sub.painter().line_segment([rr.left_bottom(), rr.right_bottom()], Stroke::new(1.0, th.row_line));
                if r.clicked() {
                    new_l.host.area = ScreenArea::Rect(s.rect);
                }
            }
        }
        let mut v = l.host.boost as f32;
        if host_slider(ui, panel, x0, y + 128.0 + list_h, "COLOR BOOST", &mut v, 0.0..=100.0, ("boost", zone), true) {
            new_l.host.boost = v.round() as u8;
        }
        let mut v = l.host.smoothing as f32;
        if host_slider(ui, panel, x0, y + 174.0 + list_h, "SMOOTHING", &mut v, 0.0..=100.0, ("smooth", zone), true) {
            new_l.host.smoothing = v.round() as u8;
        }
        // MONITOR: which screen the area is read from (index into win::monitors(), 0 = primary)
        let mons = self.monitors();
        let my = y + 222.0 + list_h;
        text(ui, pos2(x0, my), Align2::LEFT_TOP, "MONITOR", Ts::new(Fam::Reg, 12.0, th.label));
        let n = mons.len().max(1);
        let pw = ((278.0 - (n - 1) as f32 * 8.0) / n as f32).min(90.0);
        for (i, _) in mons.iter().enumerate() {
            let label = if i == 0 { "PRIMARY".to_string() } else { format!("MONITOR {}", i + 1) };
            if pill(ui, Rect::from_min_size(pos2(x0 + i as f32 * (pw + 8.0), my + 20.0), vec2(pw, 22.0)), &label, l.host.monitor == i, ("monitor", zone, i)).clicked() {
                new_l.host.monitor = i;
            }
        }
        y + 274.0 + list_h
    }

    /// REACTIVE controls; returns the y where the next block starts.
    #[allow(clippy::too_many_arguments)]
    fn reactive_panel(&mut self, ui: &mut Ui, x0: f32, y: f32, panel: Rect, zone: usize, l: Lighting, new_l: &mut Lighting) -> f32 {
        let th = pal();
        para(ui, pos2(x0, y), 278.0, "Lights the zone while a captured button is held, and briefly after it is released.", Ts::new(Fam::Reg, 11.0, th.label));
        text(ui, pos2(x0, y + 46.0), Align2::LEFT_TOP, "STAYS ON FOR", Ts::new(Fam::Reg, 12.0, th.label));
        let mut v = l.host.hold_ms as f32;
        if slider(ui, Rect::from_min_size(pos2(x0, y + 72.0), vec2(180.0, 12.0)), &mut v, 0.0..=2000.0, ("hold", zone)) {
            new_l.host.hold_ms = ((v / 50.0).round() * 50.0) as u16;
        }
        value_label(ui, pos2(panel.right() - 16.0, y + 78.0), &format!("{}", new_l.host.hold_ms), "ms");
        y + 98.0
    }

    /// AUDIO VISUALIZER controls; returns the y where the next block starts.
    #[allow(clippy::too_many_arguments)]
    fn audio_panel(&mut self, ui: &mut Ui, x0: f32, y: f32, panel: Rect, zone: usize, l: Lighting, new_l: &mut Lighting) -> f32 {
        let th = pal();
        let adv_row = |ui: &mut Ui, y: f32, open: bool| {
            let tr = text(ui, pos2(x0, y + 12.0), Align2::LEFT_CENTER, "ADVANCED SETTINGS", Ts::new(Fam::Reg, 12.0, th.label));
            chevron(ui.painter(), pos2(panel.right() - 24.0, y + 12.0), 4.0, open, Stroke::new(1.5, th.label));
            click(ui, Rect::from_min_max(pos2(x0, y), pos2(panel.right() - 8.0, y + 24.0)).union(tr), "audio-adv").clicked()
        };
        if !self.light.advanced {
            if let Some(t) = tabs(ui, pos2(x0, y), &["COLOR", "BACKGROUND COLOR"], self.light.audio_target, 16.0, "audio-tabs") {
                self.light.audio_target = t;
            }
            let bg = self.light.audio_target == 1;
            let color = if bg { l.host.color2 } else { l.color };
            if let Some(c) = Self::color_picker(ui, pos2(x0, y + 22.0), color, &mut self.light.picker, if bg { "audio-bg" } else { "audio" }, &SWATCHES) {
                if bg {
                    new_l.host.color2 = c;
                } else {
                    new_l.color = c;
                }
            }
            para(ui, pos2(x0, y + 204.0), 278.0, "Reacts to audio playing on your computer (music, games, videos), not your microphone.", Ts::new(Fam::Reg, 11.0, th.label));
            if adv_row(ui, y + 246.0, false) {
                self.light.advanced = true;
            }
            return y + 274.0;
        }
        if adv_row(ui, y, true) {
            self.light.advanced = false;
        }
        let mut v = l.host.intensity as f32;
        if host_slider(ui, panel, x0, y + 30.0, "INTENSITY", &mut v, 0.0..=100.0, ("intensity", zone), true) {
            new_l.host.intensity = v.round() as u8;
        }
        if checkbox(ui, pos2(x0, y + 82.0), "AUTO-ADJUST SENSITIVITY", l.host.auto_sens, true, ("auto-sens", zone)).clicked() {
            new_l.host.auto_sens = !l.host.auto_sens;
        }
        let mut v = l.host.sensitivity as f32;
        if host_slider(ui, panel, x0, y + 110.0, "MANUAL SENSITIVITY", &mut v, 1.0..=100.0, ("sens", zone), !l.host.auto_sens) && !l.host.auto_sens {
            new_l.host.sensitivity = v.round() as u8;
        }
        // BAND: which part of the spectrum the visualizer follows
        text(ui, pos2(x0, y + 162.0), Align2::LEFT_TOP, "BAND", Ts::new(Fam::Reg, 12.0, th.label));
        for (i, (label, b)) in [("BASS", AudioBand::Bass), ("MIDS", AudioBand::Mids), ("ALL", AudioBand::All)].iter().enumerate() {
            if pill(ui, Rect::from_min_size(pos2(x0 + i as f32 * 78.0, y + 184.0), vec2(70.0, 22.0)), label, l.host.band == *b, ("band", zone, i)).clicked() {
                new_l.host.band = *b;
            }
        }
        y + 212.0
    }

    /// Saturation/value square + hue bar + swatches + hex. Returns a new colour.
    pub(super) fn color_picker(ui: &mut Ui, origin: Pos2, color: [u8; 3], st: &mut PickerState, salt: &str, swatches: &[[u8; 3]]) -> Option<[u8; 3]> {
        if st.hsv_for != Some(color) {
            let (h, s, v) = rgb_to_hsv(color);
            let keep_h = st.hsv.map(|x| x.0).unwrap_or(h);
            st.hsv = Some((if s > 0.001 && v > 0.001 { h } else { keep_h }, s, v));
            st.hsv_for = Some(color);
            st.hex = format!("{:02X}{:02X}{:02X}", color[0], color[1], color[2]);
        }
        let (mut h, mut s, mut v) = st.hsv.unwrap_or((0.55, 1.0, 1.0));
        let sv = Rect::from_min_size(origin, vec2(278.0, 118.0));
        let hue = Rect::from_min_size(pos2(origin.x, sv.bottom() + 12.0), vec2(278.0, 12.0));
        // grid mesh: the pixel under the cursor is the colour that gets selected
        const COLS: usize = 32;
        const ROWS: usize = 16;
        let mut m = Mesh::default();
        for j in 0..=ROWS {
            for i in 0..=COLS {
                let (fx, fy) = (i as f32 / COLS as f32, j as f32 / ROWS as f32);
                let [r, g, b] = hsv_to_rgb(h, fx, 1.0 - fy);
                m.colored_vertex(pos2(sv.left() + fx * sv.width(), sv.top() + fy * sv.height()), Color32::from_rgb(r, g, b));
            }
        }
        for j in 0..ROWS {
            for i in 0..COLS {
                let a = (j * (COLS + 1) + i) as u32;
                let b = a + 1;
                let c = a + (COLS + 1) as u32;
                m.add_triangle(a, b, c);
                m.add_triangle(b, c, c + 1);
            }
        }
        ui.painter().add(Shape::mesh(m));
        let mut hm = Mesh::default();
        for i in 0..=36 {
            let t = i as f32 / 36.0;
            let c = hsv_to_rgb(t.min(0.9999), 1.0, 1.0);
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
        let r = hit(ui, sv, ("sv", salt), Sense::click_and_drag());
        if (r.dragged() || r.clicked())
            && let Some(p) = r.interact_pointer_pos()
        {
            s = ((p.x - sv.left()) / sv.width()).clamp(0.0, 1.0);
            v = 1.0 - ((p.y - sv.top()) / sv.height()).clamp(0.0, 1.0);
            changed = true;
        }
        let hr = hit(ui, hue.expand2(vec2(0.0, 4.0)), ("hue", salt), Sense::click_and_drag());
        if (hr.dragged() || hr.clicked())
            && let Some(p) = hr.interact_pointer_pos()
        {
            h = ((p.x - hue.left()) / hue.width()).clamp(0.0, 0.9999);
            changed = true;
        }
        let cur = pos2(sv.left() + s * sv.width(), sv.top() + (1.0 - v) * sv.height());
        ui.painter().circle_stroke(cur, 6.0, Stroke::new(2.0, Color32::WHITE));
        ui.painter().circle_stroke(cur, 7.5, Stroke::new(1.0, Color32::from_black_alpha(120)));
        let hx = hue.left() + h * hue.width();
        ui.painter().rect_stroke(Rect::from_center_size(pos2(hx, hue.center().y), vec2(6.0, 16.0)), CornerRadius::same(2), Stroke::new(2.0, Color32::WHITE), StrokeKind::Middle);

        // swatches + hex
        let sy = hue.bottom() + 22.0;
        let mut out = None;
        for (i, c) in swatches.iter().enumerate() {
            let center = pos2(origin.x + 9.0 + i as f32 * 22.0, sy);
            let sr = click(ui, Rect::from_center_size(center, vec2(18.0, 18.0)), ("sw", salt, i));
            ui.painter().circle_filled(center, 8.0, Color32::from_rgb(c[0], c[1], c[2]));
            if *c == color || sr.hovered() {
                ui.painter().circle_stroke(center, 10.5, Stroke::new(1.5, Color32::WHITE));
            }
            if sr.clicked() {
                out = Some(*c);
            }
        }
        let hr = Rect::from_min_size(pos2(origin.x + 206.0, sy - 12.0), vec2(72.0, 24.0));
        ui.painter().rect_stroke(hr, CornerRadius::same(4), Stroke::new(1.0, pal().outline_hi), StrokeKind::Inside);
        text(ui, pos2(hr.left() + 8.0, hr.center().y), Align2::LEFT_CENTER, "#", Ts::new(Fam::Reg, 12.0, pal().muted));
        let mut hex = st.hex.clone();
        let te = egui::TextEdit::singleline(&mut hex).frame(egui::Frame::NONE).char_limit(6).font(Ts::new(Fam::Reg, 12.0, pal().text).font()).text_color(pal().text);
        let mut child = ui.new_child(egui::UiBuilder::new().max_rect(hr));
        let resp = child.put(Rect::from_min_max(pos2(hr.left() + 18.0, hr.top() + 4.0), hr.right_bottom() - vec2(4.0, 2.0)), te);
        if hex != st.hex {
            st.hex = hex.to_uppercase().chars().filter(|c| c.is_ascii_hexdigit()).collect();
        }
        if (resp.lost_focus() || ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)))
            && st.hex.len() == 6
            && let Ok(n) = u32::from_str_radix(&st.hex, 16)
        {
            let c = [(n >> 16) as u8, (n >> 8) as u8, n as u8];
            if c != color {
                out = Some(c);
            }
        }
        if changed {
            st.hsv = Some((h, s, v));
            let c = hsv_to_rgb(h, s, v);
            st.hsv_for = Some(c);
            st.hex = format!("{:02X}{:02X}{:02X}", c[0], c[1], c[2]);
            out = Some(c);
        }
        out
    }
}

fn value_label(ui: &Ui, right_mid: Pos2, value: &str, unit: &str) {
    let ur = text(ui, right_mid, Align2::RIGHT_CENTER, unit, Ts::new(Fam::Reg, 14.0, pal().text).ls(0.0));
    text(ui, pos2(ur.left(), right_mid.y), Align2::RIGHT_CENTER, value, Ts::new(Fam::Bold, 14.0, pal().text).ls(0.0));
}

/// Label + slider + value in the style of EFFECT BRIGHTNESS; `enabled = false` dims it and ignores input.
#[allow(clippy::too_many_arguments)]
fn host_slider(ui: &Ui, panel: Rect, x0: f32, y: f32, label: &str, v: &mut f32, range: std::ops::RangeInclusive<f32>, id: impl std::hash::Hash + std::fmt::Debug, enabled: bool) -> bool {
    text(ui, pos2(x0, y), Align2::LEFT_TOP, label, Ts::new(Fam::Reg, 12.0, if enabled { pal().label } else { pal().disabled }));
    let mut local = *v;
    let changed = slider(ui, Rect::from_min_size(pos2(x0, y + 22.0), vec2(180.0, 12.0)), &mut local, range, id);
    if enabled {
        *v = local;
    }
    value_label(ui, pos2(panel.right() - 16.0, y + 28.0), &format!("{}", local.round() as i32), "%");
    changed && enabled
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn hsv_round_trips() {
        for hex in [0xFF7A59u32, 0x1A1023, 0x00B8FC, 0x808080, 0xFFFFFF, 0x000000, 0x123456] {
            let c = [(hex >> 16) as u8, (hex >> 8) as u8, hex as u8];
            let (h, s, v) = rgb_to_hsv(c);
            let back = hsv_to_rgb(h, s, v);
            assert!(c.iter().zip(back).all(|(&a, b)| (a as i32 - b as i32).abs() <= 1), "{c:?} {back:?}");
        }
    }

    #[test]
    fn hsv_corners() {
        assert_eq!(hsv_to_rgb(0.0, 1.0, 1.0), [255, 0, 0]);
        assert_eq!(hsv_to_rgb(0.37, 0.0, 1.0), [255, 255, 255]);
        assert_eq!(hsv_to_rgb(0.37, 0.8, 0.0), [0, 0, 0]);
    }
}
