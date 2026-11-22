//! "Edit Samples": G HUB's full-window editor for the user's own SCREEN SAMPLER
//! areas, drawn on a picture of the primary screen (a generated one in `--shot`
//! and scripted runs so captures stay deterministic). Changes go through
//! `self.edit` at once; zones that use a sample follow every edit.

use super::theme::*;
use super::GhubApp;
use crate::profile::*;
use egui::{Align2, Color32, CornerRadius, Pos2, Rect, Sense, Stroke, StrokeKind, TextureHandle, Ui, pos2, vec2};
use std::time::{Duration, Instant};

/// State of the sample editor opened by the SCREEN SAMPLER panel's EDIT.
pub struct SampleEditor {
    /// Index into `Config.samples` of the selected sample (handles, name field, DELETE).
    pub sel: Option<usize>,
    picture: Option<(Vec<u8>, u32, u32)>,
    tex: Option<TextureHandle>,
    drag: Option<Drag>,
    /// RESTORE DEFAULTS was clicked; a second click within 3 s clears all samples.
    restore_arm: Option<Instant>,
    name: String,
    name_for: Option<usize>,
    name_focused: bool,
}

enum Drag {
    /// Move the whole rectangle: `start` the press position, `rect` the sample at the press.
    Move { idx: usize, start: Pos2, rect: [u16; 4] },
    /// Resize the selected sample: corner 0..=3 (top-left, top-right, bottom-right, bottom-left).
    Corner { idx: usize, corner: usize },
}

impl SampleEditor {
    pub fn new(sel: Option<usize>) -> Self {
        Self { sel, picture: None, tex: None, drag: None, restore_arm: None, name: String::new(), name_for: None, name_focused: false }
    }

    /// The picture bytes, captured or generated on open and on REFRESH SCREEN.
    fn picture(&mut self, synthetic: bool) -> &(Vec<u8>, u32, u32) {
        if self.picture.is_none() {
            self.refresh(synthetic);
        }
        self.picture.as_ref().unwrap()
    }

    fn refresh(&mut self, synthetic: bool) {
        self.picture = Some(if synthetic { generated_picture() } else { crate::fx::screen_picture(1280, 720).unwrap_or_else(generated_picture) });
        self.tex = None;
    }
}

/// The generated 16:9 picture for `--shot` / scripted runs: a diagonal gradient
/// between two palette inks (no capture).
fn generated_picture() -> (Vec<u8>, u32, u32) {
    let (w, h) = (320usize, 180usize);
    let (a, b) = (pal().n(0x18), pal().n(0x48));
    let mut px = vec![0u8; w * h * 4];
    for y in 0..h {
        for x in 0..w {
            let t = (x as f32 / w as f32 + y as f32 / h as f32) * 0.5;
            let i = (y * w + x) * 4;
            px[i..i + 4].copy_from_slice(&lerp_color(a, b, t).to_array());
        }
    }
    (px, w as u32, h as u32)
}

/// Corner 0..=3 of a rectangle (top-left, top-right, bottom-right, bottom-left).
fn corner(r: Rect, c: usize) -> Pos2 {
    match c {
        0 => r.left_top(),
        1 => r.right_top(),
        2 => r.right_bottom(),
        _ => r.left_bottom(),
    }
}

impl GhubApp {
    /// The full-window "Edit Samples" overlay (drawn instead of the route while open).
    pub(crate) fn samples_overlay(&mut self, ui: &mut Ui, win: Rect) {
        let Some(mut ed) = self.sample_edit.take() else { return };
        let keep = self.samples_editor(ui, win, &mut ed);
        if keep {
            self.sample_edit = Some(ed);
        }
    }

    /// Returns `false` to close the overlay.
    fn samples_editor(&mut self, ui: &mut Ui, win: Rect, ed: &mut SampleEditor) -> bool {
        let th = pal();
        let bc = pos2(win.left() + 39.0, win.top() + 58.0);
        let back = click(ui, Rect::from_center_size(bc, vec2(44.0, 44.0)), "sample-back");
        arrow_back(ui.painter(), bc, Stroke::new(2.0, if back.hovered() { th.primary } else { th.text }));
        let escape = ui.input(|i| i.key_pressed(egui::Key::Escape)) && !ed.name_focused;
        let open = !(back.clicked() || escape);
        text(ui, pos2(win.left() + 64.0, win.top() + 133.0), Align2::LEFT_CENTER, "Edit Samples", Ts::new(Fam::Bold, 24.0, th.text));

        // the screen picture: a real capture, or the generated one in --shot / scripted runs
        let synthetic = self.shot.is_some() || self.scripted();
        if ed.tex.is_none() {
            let img = {
                let (px, w, h) = ed.picture(synthetic);
                egui::ColorImage::from_rgba_unmultiplied([*w as usize, *h as usize], px)
            };
            ed.tex = Some(ui.ctx().load_texture("sample-screen", img, egui::TextureOptions::LINEAR));
        }
        let (tex_id, pw, ph) = ed.tex.as_ref().map(|t| (t.id(), t.size()[0] as f32, t.size()[1] as f32)).unwrap_or_default();
        // the largest picture-aspect rectangle in x 64..right-64, y 170..bottom-150, centred, top-aligned
        let avail = Rect::from_min_max(pos2(win.left() + 64.0, win.top() + 170.0), pos2(win.right() - 64.0, win.bottom() - 150.0));
        let ar = if ph > 0.0 { pw / ph } else { 16.0 / 9.0 };
        let (mut w, mut h) = (avail.width(), avail.width() / ar);
        if h > avail.height() {
            h = avail.height();
            w = h * ar;
        }
        let pic = Rect::from_min_size(pos2(avail.center().x - w / 2.0, avail.top()), vec2(w, h));
        ui.painter().image(tex_id, pic, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::WHITE);
        ui.painter().rect_stroke(pic, CornerRadius::ZERO, Stroke::new(1.0, th.ink(51)), StrokeKind::Inside);
        hit(ui, pic, "sample-pic", Sense::click_and_drag());

        let cfg = self.cfg();
        let to_px = |r: [u16; 4]| {
            Rect::from_min_max(
                pos2(pic.left() + r[0] as f32 / 1000.0 * pic.width(), pic.top() + r[1] as f32 / 1000.0 * pic.height()),
                pos2(pic.left() + r[2] as f32 / 1000.0 * pic.width(), pic.top() + r[3] as f32 / 1000.0 * pic.height()),
            )
        };
        let to_th = |p: Pos2| {
            [
                ((p.x - pic.left()) / pic.width() * 1000.0).round().clamp(0.0, 1000.0) as u16,
                ((p.y - pic.top()) / pic.height() * 1000.0).round().clamp(0.0, 1000.0) as u16,
            ]
        };

        // every sample on the picture
        for (i, s) in cfg.samples.iter().enumerate() {
            let r = to_px(s.rect);
            let sel = ed.sel == Some(i);
            let (border, fill) = if sel {
                (Stroke::new(2.0, th.primary), th.primary.linear_multiply(0.25))
            } else {
                (Stroke::new(1.0, th.text), th.ink(40))
            };
            ui.painter().rect_filled(r, CornerRadius::ZERO, fill);
            ui.painter().rect_stroke(r, CornerRadius::ZERO, border, StrokeKind::Inside);
            ui.painter()
                .with_clip_rect(r)
                .galley(pos2(r.left() + 4.0, r.top() + 2.0), galley(ui, &s.name.to_uppercase(), Ts::new(Fam::Bold, 11.0, th.text), f32::INFINITY), th.text);
            if sel {
                for c in 0..4 {
                    ui.painter().rect_filled(Rect::from_center_size(corner(r, c), vec2(10.0, 10.0)), CornerRadius::same(2), th.primary);
                }
            }
        }

        // pointer: press a handle to resize, press inside to select and move, empty clears
        let (pressed, released, down, pos) = ui.input(|i| (i.pointer.any_pressed(), i.pointer.any_released(), i.pointer.any_down(), i.pointer.interact_pos()));
        if pressed && pos.is_some_and(|p| pic.contains(p)) {
            let p = pos.unwrap();
            let mut found = false;
            if let Some(si) = ed.sel.filter(|&i| i < cfg.samples.len()) {
                let r = to_px(cfg.samples[si].rect);
                for c in 0..4 {
                    if Rect::from_center_size(corner(r, c), vec2(14.0, 14.0)).contains(p) {
                        ed.drag = Some(Drag::Corner { idx: si, corner: c });
                        found = true;
                        break;
                    }
                }
            }
            if !found {
                match (0..cfg.samples.len()).rev().find(|&i| to_px(cfg.samples[i].rect).contains(p)) {
                    Some(i) => {
                        ed.sel = Some(i);
                        ed.drag = Some(Drag::Move { idx: i, start: p, rect: cfg.samples[i].rect });
                    }
                    None => ed.sel = None,
                }
            }
        }
        if down && let (Some(p), Some(d)) = (pos, &ed.drag) {
            let (idx, new) = match *d {
                Drag::Move { idx, start, rect } => {
                    let (dx, dy) = ((p.x - start.x) / pic.width() * 1000.0, (p.y - start.y) / pic.height() * 1000.0);
                    let (rw, rh) = (rect[2] - rect[0], rect[3] - rect[1]);
                    let x0 = (rect[0] as f32 + dx).round().clamp(0.0, (1000 - rw) as f32) as u16;
                    let y0 = (rect[1] as f32 + dy).round().clamp(0.0, (1000 - rh) as f32) as u16;
                    (idx, [x0, y0, x0 + rw, y0 + rh])
                }
                Drag::Corner { idx, corner } => {
                    let fp = to_th(p);
                    let mut new = cfg.samples.get(idx).map(|s| s.rect).unwrap_or([fp[0], fp[1], fp[0], fp[1]]);
                    match corner {
                        0 => {
                            new[0] = fp[0];
                            new[1] = fp[1];
                        }
                        1 => {
                            new[2] = fp[0];
                            new[1] = fp[1];
                        }
                        2 => {
                            new[2] = fp[0];
                            new[3] = fp[1];
                        }
                        _ => {
                            new[0] = fp[0];
                            new[3] = fp[1];
                        }
                    }
                    (idx, new)
                }
            };
            if cfg.samples.get(idx).is_some_and(|s| s.rect != new) {
                self.edit(|c| c.set_sample_rect(idx, new));
            }
        }
        if released {
            ed.drag = None;
        }

        // the row under the picture
        let by = pic.bottom() + 16.0;
        let bx = win.left() + 64.0;
        let can_add = cfg.samples.len() < SAMPLE_MAX;
        if dark_button(ui, Rect::from_min_size(pos2(bx, by), vec2(170.0, 32.0)), "ADD NEW SAMPLE", "sample-add", can_add).clicked() {
            let mut added = None;
            self.edit(|c| added = c.add_sample());
            if added.is_some() {
                ed.sel = added;
                ed.name_for = None;
            }
        }
        if dark_button(ui, Rect::from_min_size(pos2(bx + 184.0, by), vec2(150.0, 32.0)), "REFRESH SCREEN", "sample-refresh", true).clicked() {
            ed.refresh(synthetic);
        }
        let armed = ed.restore_arm.is_some_and(|t| t.elapsed() < Duration::from_secs(3));
        if !armed {
            ed.restore_arm = None;
        }
        let restore_label = if armed { "CLICK AGAIN TO REMOVE ALL" } else { "RESTORE DEFAULTS" };
        if dark_button(ui, Rect::from_min_size(pos2(bx + 348.0, by), vec2(170.0, 32.0)), restore_label, "sample-restore", true).clicked() {
            if armed {
                self.edit(|c| c.clear_samples());
                ed.sel = None;
                ed.name_for = None;
                ed.restore_arm = None;
            } else {
                ed.restore_arm = Some(Instant::now());
            }
        }
        if ed.restore_arm.is_some() {
            ui.ctx().request_repaint_after(Duration::from_secs(1));
        }
        // the selected sample's name, right-aligned in an underlined field
        if let Some(si) = ed.sel.filter(|&i| i < cfg.samples.len()) {
            if ed.name_for != Some(si) {
                ed.name_for = Some(si);
                ed.name = cfg.samples[si].name.clone();
            }
            let fr = Rect::from_min_max(pos2(win.right() - 64.0 - 220.0, by), pos2(win.right() - 64.0, by + 32.0));
            let te = egui::TextEdit::singleline(&mut ed.name)
                .frame(egui::Frame::NONE)
                .font(Ts::new(Fam::Reg, 14.0, th.text).font())
                .text_color(th.text)
                .align(egui::Align2::RIGHT_CENTER)
                .char_limit(40);
            let resp = ui.put(fr, te);
            ui.painter().line_segment([pos2(fr.left(), fr.bottom() - 5.0), pos2(fr.right(), fr.bottom() - 5.0)], Stroke::new(1.0, th.text));
            if resp.lost_focus() || (resp.has_focus() && ui.input(|i| i.key_pressed(egui::Key::Enter))) {
                self.edit(|c| c.rename_sample(si, &ed.name));
                ed.name_for = None;
            }
            ed.name_focused = resp.has_focus();
        } else {
            ed.name_focused = false;
        }
        text(ui, pos2(bx, by + 46.0), Align2::LEFT_TOP, "To remove a sample, select it and press DELETE.", Ts::new(Fam::Reg, 12.0, th.muted));
        // Delete removes the selected sample while no text field is focused
        if ui.input(|i| i.key_pressed(egui::Key::Delete)) && !ed.name_focused && let Some(si) = ed.sel {
            self.edit(|c| c.remove_sample(si));
            ed.sel = None;
            ed.name_for = None;
        }
        open
    }
}
