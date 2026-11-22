//! Profiles: "All Games & Apps", profile switching, persistent profile and
//! per-app profile management.

use super::theme::*;
use super::{GhubApp, Overlay};
use crate::games::{self, Installed};
use crate::profile::*;
use crossbeam_channel::Receiver;
use egui::{Align2, Color32, CornerRadius, Mesh, Rect, Shape, Stroke, StrokeKind, Ui, UiBuilder, pos2, vec2};

#[derive(Default)]
enum GameScan {
    #[default]
    Idle,
    Running(Receiver<Vec<Installed>>),
    Done(Vec<Installed>),
}

#[derive(Default)]
pub struct ProfState {
    scan: GameScan,
    pick: Option<Receiver<Option<String>>>,
    filter: usize,
    search: String,
    adding: Option<String>,
    detail: Option<usize>,
    renaming: Option<(usize, String)>,
}

impl ProfState {
    pub fn open_add(&mut self) {
        self.adding = Some(String::new());
    }

    pub fn open_detail(&mut self, app: usize) {
        self.detail = Some(app);
    }
}

impl GhubApp {
    pub(crate) fn profiles_page(&mut self, ui: &mut Ui, win: Rect) {
        let cfg = self.cfg();
        let top = win.top() + 88.0;
        let col = win.left() + 338.0;
        ui.painter().line_segment([pos2(col, top + 1.0), pos2(col, win.bottom())], Stroke::new(1.0, pal().divider));

        // filters
        let counts = [cfg.apps.len(), cfg.apps.iter().filter(|a| a.enabled).count(), cfg.apps.iter().filter(|a| !a.enabled).count()];
        for (i, name) in ["All", "Active", "Disabled"].iter().enumerate() {
            let r = Rect::from_min_size(pos2(win.left() + 48.0, top + 34.0 + i as f32 * 38.0), vec2(269.0, 36.0));
            let resp = click(ui, r, ("filter", i));
            let on = self.prof.filter == i;
            if on {
                ui.painter().rect_filled(r, CornerRadius::same(6), pal().primary_tint);
            } else if resp.hovered() {
                ui.painter().rect_filled(r, CornerRadius::same(6), pal().surface);
            }
            let c = if on { pal().primary } else { pal().text };
            text(ui, pos2(r.left() + 12.0, r.center().y), Align2::LEFT_CENTER, name, Ts::new(Fam::Bold, 14.0, c).ls(0.3));
            if on {
                text(ui, pos2(r.right() - 14.0, r.center().y), Align2::RIGHT_CENTER, &counts[i].to_string(), Ts::new(Fam::Bold, 14.0, c));
            }
            if resp.clicked() {
                self.prof.filter = i;
                self.prof.detail = None;
            }
        }

        // profile switching
        let by = win.bottom() - 223.0;
        let t = text(ui, pos2(win.left() + 59.0, by), Align2::LEFT_CENTER, "Profile Switching", Ts::new(Fam::Reg, 18.0, pal().text).ls(0.2));
        info(ui.painter(), pos2(t.right() + 20.0, by), pal().text);
        hit(ui, Rect::from_center_size(pos2(t.right() + 20.0, by), vec2(18.0, 18.0)), "ps-info", egui::Sense::hover())
            .on_hover_text("Switch profiles automatically when a game or app with its own profile is in front.");
        if checkbox(ui, pos2(win.left() + 60.0, by + 42.0), "Enable profile switching", cfg.profile_switching, true, "ps-enable").clicked() {
            self.edit(|c| c.profile_switching = !c.profile_switching);
        }
        text(ui, pos2(win.left() + 59.0, by + 93.0), Align2::LEFT_CENTER, "Select persistent profile", Ts::new(Fam::Bold, 12.0, pal().text).ls(0.3));
        let dd = Rect::from_min_size(pos2(win.left() + 59.0, by + 105.0), vec2(270.0, 48.0));
        let label = match cfg.persistent {
            Some((a, p)) => (cfg.apps[a].name.clone(), cfg.apps[a].profiles[p].name.clone()),
            None => ("None".to_string(), String::new()),
        };
        if dropdown(ui, dd, &label.0, &label.1, "persist-dd").clicked() {
            self.open_overlay(ui.ctx(), Overlay::Persistent(dd));
        }

        let main = Rect::from_min_max(pos2(col + 64.0, top), pos2(win.right() - 44.0, win.bottom()));
        match self.prof.detail {
            Some(i) if i < cfg.apps.len() => self.app_detail(ui, main, i),
            _ => self.app_grid(ui, main),
        }
    }

    fn app_grid(&mut self, ui: &mut Ui, main: Rect) {
        let cfg = self.cfg();
        text(ui, pos2(main.left() - 1.0, main.top() + 49.0), Align2::LEFT_CENTER, "All Games & Apps", Ts::new(Fam::Bold, 24.0, pal().text).ls(0.0));
        para(
            ui,
            pos2(main.left() - 1.0, main.top() + 65.0),
            545.0,
            "Select a game or app to manage its profiles and macros. Disabling a game or app prevents its profiles from becoming active. Removing it deletes its profiles and macros from GHUB-X.",
            Ts::new(Fam::Reg, 13.0, pal().label).ls(0.4),
        );
        let sb = Rect::from_min_size(pos2(main.left(), main.top() + 150.0), vec2(249.0, 48.0));
        ui.painter().rect_stroke(sb, CornerRadius::same(6), Stroke::new(1.0, pal().n(0x4a)), StrokeKind::Inside);
        search(ui.painter(), pos2(sb.left() + 30.0, sb.center().y), Stroke::new(1.8, pal().text));
        let te = egui::TextEdit::singleline(&mut self.prof.search)
            .hint_text(egui::RichText::new("Search").color(pal().label).font(Ts::new(Fam::Reg, 15.0, pal().label).font()))
            .frame(egui::Frame::NONE)
            .font(Ts::new(Fam::Reg, 15.0, pal().text).font())
            .text_color(pal().text);
        let mut child = ui.new_child(UiBuilder::new().max_rect(sb));
        child.put(Rect::from_min_max(pos2(sb.left() + 52.0, sb.top() + 13.0), pos2(sb.right() - 10.0, sb.bottom() - 10.0)), te);
        let ab = Rect::from_min_size(pos2(sb.right() + 16.0, sb.top()), vec2(213.0, 48.0));
        let add = click(ui, ab, "add-app");
        ui.painter().rect_filled(ab, CornerRadius::same(6), if add.hovered() { pal().primary_hover } else { pal().primary });
        let pc = pos2(ab.left() + 32.0, ab.center().y);
        ui.painter().line_segment([pc - vec2(7.0, 0.0), pc + vec2(7.0, 0.0)], Stroke::new(2.0, pal().on_primary));
        ui.painter().line_segment([pc - vec2(0.0, 7.0), pc + vec2(0.0, 7.0)], Stroke::new(2.0, pal().on_primary));
        text(ui, pos2(ab.left() + 53.0, ab.center().y), Align2::LEFT_CENTER, "ADD GAMES & APPS", Ts::new(Fam::Bold, 14.0, pal().on_primary).ls(0.2));
        if add.clicked() {
            self.prof.adding = Some(self.prof.adding.take().map(|_| None).unwrap_or(Some(String::new()))).flatten();
        }

        let mut y = sb.bottom() + 24.0;
        if self.prof.adding.is_none() {
            self.prof.scan = GameScan::Idle;
        }
        if self.prof.adding.is_some() {
            y = self.add_app_form(ui, pos2(main.left(), y), main.width());
        }
        let q = self.prof.search.to_lowercase();
        let mut x = main.left();
        let mut open: Option<usize> = None;
        for (i, a) in cfg.apps.iter().enumerate() {
            let show = match self.prof.filter {
                1 => a.enabled,
                2 => !a.enabled,
                _ => true,
            } && (q.is_empty() || a.name.to_lowercase().contains(&q));
            if !show {
                continue;
            }
            if x + 192.0 > main.right() {
                x = main.left();
                y += 377.0;
            }
            let tile = Rect::from_min_size(pos2(x, y), vec2(192.0, 361.0));
            let r = click(ui, tile, ("tile", i));
            let current = cfg.current_app == i;
            let fill = if current { pal().primary_tint_lo } else if r.hovered() { pal().surface_hi } else { pal().surface };
            ui.painter().rect_filled(tile, CornerRadius::same(10), fill);
            if current {
                ui.painter().rect_stroke(tile, CornerRadius::same(10), Stroke::new(1.5, pal().primary), StrokeKind::Inside);
            }
            let img = Rect::from_min_size(tile.min + vec2(13.0, 13.0), vec2(166.0, 219.0));
            self.app_art(ui, img, a, i);
            text(ui, pos2(tile.left() + 13.0, tile.top() + 257.0), Align2::LEFT_CENTER, &a.name, Ts::new(Fam::Bold, 15.0, if a.enabled { pal().text } else { pal().muted }).ls(0.2));
            let sub = match &a.exe {
                Some(e) => e.clone(),
                None => "Default for every app".into(),
            };
            text(ui, pos2(tile.left() + 13.0, tile.top() + 279.0), Align2::LEFT_CENTER, &sub, Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.3));
            profile_icon(ui.painter(), pos2(tile.left() + 26.0, tile.top() + 306.0), pal().primary);
            text(ui, pos2(tile.left() + 44.0, tile.top() + 306.0), Align2::LEFT_CENTER, &format!("{}", a.profiles.len()), Ts::new(Fam::Bold, 12.0, pal().primary));
            if r.clicked() {
                open = Some(i);
            }
            x += 208.0;
        }
        if let Some(i) = open {
            self.edit(|c| c.current_app = i);
            self.prof.detail = Some(i);
        }
    }

    fn app_art(&self, ui: &Ui, img: Rect, a: &AppEntry, i: usize) {
        let mut m = Mesh::default();
        let (c0, c1) = if a.exe.is_none() {
            (Color32::from_rgb(0x2a, 0x2c, 0x31), Color32::from_rgb(0x0e, 0x0f, 0x11))
        } else {
            let h = (i as f32 * 0.17 + 0.55) % 1.0;
            let a = egui::ecolor::Hsva::new(h, 0.55, 0.35, 1.0).to_srgb();
            (Color32::from_rgb(a[0], a[1], a[2]), Color32::from_rgb(0x0e, 0x0f, 0x11))
        };
        for (p, c) in [(img.left_top(), c0), (img.right_top(), c0), (img.right_bottom(), c1), (img.left_bottom(), c1)] {
            m.colored_vertex(p, c);
        }
        m.add_triangle(0, 1, 2);
        m.add_triangle(0, 2, 3);
        ui.painter().add(Shape::mesh(m));
        if a.exe.is_none() {
            if let Some(t) = &self.tex.front {
                let h = img.height() * 0.84;
                let w = h * t.size_vec2().x / t.size_vec2().y;
                let r = Rect::from_center_size(img.center() + vec2(0.0, 6.0), vec2(w, h));
                let clip = ui.painter().with_clip_rect(img);
                clip.image(t.id(), r, Rect::from_min_max(pos2(0.0, 0.0), pos2(1.0, 1.0)), Color32::from_gray(150));
            }
        } else {
            let letter: String = a.name.chars().next().map(|c| c.to_uppercase().collect()).unwrap_or_default();
            text(ui, img.center(), Align2::CENTER_CENTER, &letter, Ts::new(Fam::Bold, 72.0, Color32::from_white_alpha(200)).ls(0.0));
        }
    }

    fn add_app_form(&mut self, ui: &mut Ui, origin: egui::Pos2, width: f32) -> f32 {
        if matches!(self.prof.scan, GameScan::Idle) {
            self.prof.scan = if self.shot.is_some() {
                GameScan::Done(games::scan(games::db_blocking()))
            } else {
                let (tx, rx) = crossbeam_channel::bounded(1);
                std::thread::spawn(move || {
                    let _ = tx.send(games::scan(games::db_blocking()));
                });
                GameScan::Running(rx)
            };
        }
        if let GameScan::Running(rx) = &self.prof.scan {
            match rx.try_recv() {
                Ok(list) => self.prof.scan = GameScan::Done(list),
                Err(_) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        let cfg = self.cfg();
        let folder = |d: &str| d.replace('/', "\\").trim_end_matches('\\').to_lowercase();
        let available: Vec<Installed> = match &self.prof.scan {
            GameScan::Done(list) => list
                .iter()
                .filter(|g| !cfg.apps.iter().any(|a| a.dir.as_deref().is_some_and(|d| folder(d) == folder(&g.dir)) || (g.ghub_id.is_some() && a.ghub_id == g.ghub_id)))
                .cloned()
                .collect(),
            _ => Vec::new(),
        };
        let list_h = available.len().clamp(1, 5) as f32 * 34.0;
        let w = width.min(560.0);
        let field_top = 58.0 + list_h + 8.0 + 14.0;
        let r = Rect::from_min_size(origin, vec2(w, field_top + 28.0 + 12.0 + 22.0 + 14.0));
        ui.painter().rect_filled(r, CornerRadius::same(10), pal().surface);
        text(ui, r.left_top() + vec2(16.0, 20.0), Align2::LEFT_CENTER, "ADD A GAME OR APP", Ts::new(Fam::Bold, 12.0, pal().text));
        text(ui, r.left_top() + vec2(16.0, 44.0), Align2::LEFT_CENTER, "DETECTED GAMES", t_small_label());

        let list_r = Rect::from_min_size(r.left_top() + vec2(16.0, 58.0), vec2(w - 32.0, list_h));
        let mut chosen: Option<Installed> = None;
        match &self.prof.scan {
            GameScan::Done(_) if !available.is_empty() => {
                let mut child = ui.new_child(UiBuilder::new().max_rect(list_r));
                egui::ScrollArea::vertical().id_salt("detected-games").max_height(list_h).auto_shrink([false, false]).show(&mut child, |ui| {
                    for (i, g) in available.iter().enumerate() {
                        let (row, _) = ui.allocate_exact_size(vec2(list_r.width() - 8.0, 34.0), egui::Sense::hover());
                        text(ui, pos2(row.left(), row.center().y), Align2::LEFT_CENTER, &g.name, Ts::new(Fam::Bold, 13.0, pal().text).ls(0.2));
                        let add = Rect::from_center_size(pos2(row.right() - 28.0, row.center().y), vec2(56.0, 22.0));
                        text(ui, pos2(add.left() - 14.0, row.center().y), Align2::RIGHT_CENTER, g.source, Ts::new(Fam::Reg, 11.0, pal().muted).ls(0.3));
                        if blue_button(ui, add, "ADD", ("game-add", i)).clicked() {
                            chosen = Some(g.clone());
                        }
                        ui.painter().line_segment([pos2(row.left(), row.bottom() - 0.5), pos2(row.right(), row.bottom() - 0.5)], Stroke::new(1.0, pal().row_line));
                    }
                });
            }
            state => {
                let msg = if matches!(state, GameScan::Done(_)) { "No other installed games found." } else { "Scanning\u{2026}" };
                text(ui, pos2(list_r.left(), list_r.top() + 17.0), Align2::LEFT_CENTER, msg, Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.4));
            }
        }
        let div_y = r.top() + 58.0 + list_h + 8.0;
        ui.painter().line_segment([pos2(r.left() + 16.0, div_y), pos2(r.right() - 16.0, div_y)], Stroke::new(1.0, pal().divider));

        let mut s = self.prof.adding.clone().unwrap_or_default();
        if let Some(rx) = &self.prof.pick {
            match rx.try_recv() {
                Ok(picked) => {
                    self.prof.pick = None;
                    if let Some(path) = picked {
                        s = path.rsplit(['\\', '/']).next().unwrap_or("").to_string();
                    }
                }
                Err(_) => ui.ctx().request_repaint_after(std::time::Duration::from_millis(100)),
            }
        }
        let fr = Rect::from_min_size(r.left_top() + vec2(16.0, field_top), vec2(w - 284.0, 28.0));
        let te = egui::TextEdit::singleline(&mut s)
            .hint_text(egui::RichText::new("Executable name, e.g. cs2.exe").color(pal().label))
            .frame(egui::Frame::NONE)
            .font(Ts::new(Fam::Reg, 13.0, pal().text).font())
            .text_color(pal().text);
        let mut child = ui.new_child(UiBuilder::new().max_rect(fr));
        child.put(fr.shrink2(vec2(2.0, 4.0)), te);
        ui.painter().line_segment([pos2(fr.left(), fr.bottom()), pos2(fr.right(), fr.bottom())], Stroke::new(1.0, pal().outline_hi));
        let idle = self.prof.pick.is_none();
        if dark_button(ui, Rect::from_min_size(pos2(r.right() - 260.0, fr.top()), vec2(80.0, 28.0)), "BROWSE", "app-browse", idle).clicked() && idle {
            let owner = crate::win::main_window_handle();
            let (tx, rx) = crossbeam_channel::bounded(1);
            std::thread::spawn(move || {
                let _ = tx.send(crate::win::pick_exe(owner));
            });
            self.prof.pick = Some(rx);
        }
        let add = blue_button(ui, Rect::from_min_size(pos2(r.right() - 172.0, fr.top()), vec2(76.0, 28.0)), "ADD", "app-add-ok").clicked();
        let cancel = dark_button(ui, Rect::from_min_size(pos2(r.right() - 88.0, fr.top()), vec2(72.0, 28.0)), "CANCEL", "app-add-cancel", true).clicked();
        // recently focused apps as suggestions
        let mut x = r.left() + 16.0;
        for (i, exe) in self.seen_exes.clone().iter().take(5).enumerate() {
            let g = galley(ui, exe, Ts::new(Fam::Reg, 11.0, pal().text).ls(0.2), f32::INFINITY);
            let cr = Rect::from_min_size(pos2(x, fr.bottom() + 12.0), vec2(g.size().x + 18.0, 22.0));
            if cr.right() > r.right() - 16.0 {
                break;
            }
            if pill(ui, cr, exe, false, ("sugg", i)).clicked() {
                s = exe.clone();
            }
            x = cr.right() + 8.0;
        }
        if let Some(game) = chosen {
            self.edit(|c| {
                let mut base = c.apps[0].profile().clone();
                base.name = "Default".into();
                c.apps.push(AppEntry { name: game.name.clone(), exe: None, dir: Some(game.dir.clone()), ghub_id: game.ghub_id.clone(), enabled: true, profiles: vec![base], active_profile: 0, macros: c.apps[0].macros.clone() });
                c.profile_switching = true;
            });
            self.prof.adding = Some(s);
            self.say("Added \u{2014} its profile activates when the game is in front");
        } else if cancel {
            self.prof.adding = None;
        } else if add && !s.trim().is_empty() {
            let exe = s.trim().trim_matches('"').rsplit(['\\', '/']).next().unwrap_or("").to_string();
            let mut name = exe.trim_end_matches(".exe").trim_end_matches(".EXE").to_string();
            if let Some(f) = name.get(..1) {
                name = f.to_uppercase() + &name[1..];
            }
            self.edit(|c| {
                let mut base = c.apps[0].profile().clone();
                base.name = "Default".into();
                c.apps.push(AppEntry { name, exe: Some(exe.clone()), dir: None, ghub_id: None, enabled: true, profiles: vec![base], active_profile: 0, macros: c.apps[0].macros.clone() });
                c.profile_switching = true;
            });
            self.prof.adding = None;
            self.say("Added \u{2014} its profile activates when the app is in front");
        } else {
            self.prof.adding = Some(s);
        }
        r.bottom() + 20.0
    }

    fn app_detail(&mut self, ui: &mut Ui, main: Rect, ai: usize) {
        let cfg = self.cfg();
        let app = cfg.apps[ai].clone();
        let back = click(ui, Rect::from_min_size(pos2(main.left() - 4.0, main.top() + 22.0), vec2(180.0, 24.0)), "detail-back");
        let bc = pos2(main.left() + 4.0, main.top() + 34.0);
        ui.painter().add(Shape::line(vec![bc + vec2(2.5, -5.0), bc + vec2(-2.5, 0.0), bc + vec2(2.5, 5.0)], Stroke::new(1.6, if back.hovered() { pal().primary } else { pal().text })));
        text(ui, pos2(main.left() + 16.0, main.top() + 34.0), Align2::LEFT_CENTER, "ALL GAMES & APPS", Ts::new(Fam::Bold, 12.0, if back.hovered() { pal().primary } else { pal().text }));
        if back.clicked() {
            self.prof.detail = None;
            self.prof.renaming = None;
        }
        text(ui, pos2(main.left(), main.top() + 76.0), Align2::LEFT_CENTER, &app.name, Ts::new(Fam::Bold, 24.0, pal().text).ls(0.0));
        let sub = match (&app.dir, &app.exe) {
            (Some(d), _) => format!("Folder: {d}"),
            (None, Some(e)) => format!("Executable: {e}"),
            _ => "Used whenever no other app's profile is active".into(),
        };
        let sub_r = text(ui, pos2(main.left(), main.top() + 102.0), Align2::LEFT_CENTER, &sub, Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.4));
        if app.ghub_id.is_some() {
            text(ui, pos2(sub_r.right() + 16.0, main.top() + 102.0), Align2::LEFT_CENTER, "G HUB game profile", Ts::new(Fam::Reg, 12.0, pal().muted).ls(0.4));
        }
        if app.exe.is_some() {
            text(ui, pos2(main.right() - 50.0, main.top() + 76.0), Align2::RIGHT_CENTER, "ENABLED", Ts::new(Fam::Bold, 12.0, pal().text));
            if switch(ui, pos2(main.right() - 18.0, main.top() + 76.0), app.enabled, pal().primary, "app-enabled").clicked() {
                self.edit(|c| c.apps[ai].enabled = !c.apps[ai].enabled);
            }
        }
        let mut y = main.top() + 140.0;
        text(ui, pos2(main.left(), y), Align2::LEFT_CENTER, "PROFILES", t_section());
        y += 20.0;
        let mut act: Option<(usize, u8)> = None; // 0 activate, 1 rename start, 2 duplicate, 3 delete
        let mut rename_commit: Option<(usize, String)> = None;
        for (pi, p) in app.profiles.iter().enumerate() {
            let row = Rect::from_min_size(pos2(main.left(), y), vec2(main.width(), 48.0));
            let r = hit(ui, row, ("prow", pi), egui::Sense::hover());
            ui.painter().rect_filled(row, CornerRadius::same(8), if r.hovered() { pal().surface_hi } else { pal().surface });
            if radio(ui, pos2(row.left() + 22.0, row.center().y), app.active_profile == pi, ("pact", pi)).clicked() {
                act = Some((pi, 0));
            }
            let renaming = matches!(&self.prof.renaming, Some((i, _)) if *i == pi);
            if renaming {
                let mut s = self.prof.renaming.as_ref().map(|r| r.1.clone()).unwrap_or_default();
                let fr = Rect::from_min_size(pos2(row.left() + 44.0, row.center().y - 12.0), vec2(220.0, 24.0));
                let te = egui::TextEdit::singleline(&mut s).frame(egui::Frame::NONE).font(Ts::new(Fam::Reg, 14.0, pal().text).font()).text_color(pal().text);
                let mut child = ui.new_child(UiBuilder::new().max_rect(fr));
                let resp = child.put(fr, te);
                resp.request_focus();
                ui.painter().line_segment([fr.left_bottom(), fr.right_bottom()], Stroke::new(1.0, pal().primary));
                if ui.ctx().input(|i| i.key_pressed(egui::Key::Enter)) || resp.lost_focus() {
                    rename_commit = Some((pi, s));
                } else {
                    self.prof.renaming = Some((pi, s));
                }
            } else {
                let tr = text(ui, pos2(row.left() + 44.0, row.center().y), Align2::LEFT_CENTER, &p.name, Ts::new(Fam::Reg, 14.0, pal().text).ls(0.3));
                if cfg.persistent == Some((ai, pi)) {
                    text(ui, pos2(tr.right() + 12.0, row.center().y), Align2::LEFT_CENTER, "PERSISTENT", Ts::new(Fam::Bold, 10.0, pal().amber));
                }
            }
            let mut bx = row.right() - 16.0;
            for (k, label) in [(3u8, "DELETE"), (2, "DUPLICATE"), (1, "RENAME")] {
                if k == 3 && app.profiles.len() <= 1 {
                    continue;
                }
                let g = galley(ui, label, Ts::new(Fam::Bold, 11.0, pal().text), f32::INFINITY);
                let br = Rect::from_min_size(pos2(bx - g.size().x - 12.0, row.center().y - 12.0), vec2(g.size().x + 12.0, 24.0));
                let b = click(ui, br, ("pbtn", pi, k));
                let c = if !r.hovered() && !b.hovered() { pal().ink(70) } else if b.hovered() { if k == 3 { pal().error } else { pal().primary } } else { pal().text };
                text(ui, br.center(), Align2::CENTER_CENTER, label, Ts::new(Fam::Bold, 11.0, c));
                if b.clicked() {
                    act = Some((pi, k));
                }
                bx = br.left() - 8.0;
            }
            y += 56.0;
        }
        let ap = Rect::from_min_size(pos2(main.left(), y + 4.0), vec2(160.0, 32.0));
        if dark_button(ui, ap, "+ ADD PROFILE", "add-profile", true).clicked() {
            self.edit(|c| {
                let n = c.apps[ai].profiles.len() + 1;
                let mut p = c.apps[ai].profile().clone();
                p.name = format!("Profile {n}");
                c.apps[ai].profiles.push(p);
            });
        }
        if app.exe.is_some() && dark_button(ui, Rect::from_min_size(pos2(main.right() - 200.0, y + 4.0), vec2(200.0, 32.0)), "REMOVE GAME OR APP", "rm-app", true).clicked() {
            self.edit(|c| {
                c.apps.remove(ai);
                c.current_app = 0;
                c.persistent = None;
            });
            self.prof.detail = None;
            self.say("Removed");
            return;
        }
        if let Some((pi, s)) = rename_commit {
            let s = s.trim().to_string();
            if !s.is_empty() {
                self.edit(|c| c.apps[ai].profiles[pi].name = s);
            }
            self.prof.renaming = None;
        }
        match act {
            Some((pi, 0)) => self.edit(|c| {
                c.apps[ai].active_profile = pi;
                c.current_app = ai;
            }),
            Some((pi, 1)) => self.prof.renaming = Some((pi, app.profiles[pi].name.clone())),
            Some((pi, 2)) => self.edit(|c| {
                let mut p = c.apps[ai].profiles[pi].clone();
                p.name = format!("{} copy", p.name);
                c.apps[ai].profiles.insert(pi + 1, p);
            }),
            Some((pi, _)) => self.edit(|c| {
                let a = &mut c.apps[ai];
                a.profiles.remove(pi);
                if a.active_profile >= pi && a.active_profile > 0 {
                    a.active_profile -= 1;
                }
                c.persistent = None;
            }),
            None => {}
        }
    }
}

/// G HUB dark dropdown ("Desktop - Default").
pub(crate) fn dropdown(ui: &Ui, r: Rect, first: &str, second: &str, id: &str) -> egui::Response {
    let resp = click(ui, r, id);
    ui.painter().rect_filled(r, CornerRadius::same(8), if resp.hovered() { pal().surface_hi } else { pal().n(0x1c) });
    let a = text(ui, pos2(r.left() + 17.0, r.center().y), Align2::LEFT_CENTER, first, Ts::new(Fam::Reg, 16.0, pal().text).ls(0.6));
    if !second.is_empty() {
        let d = text(ui, pos2(a.right() + 6.0, r.center().y), Align2::LEFT_CENTER, "-", Ts::new(Fam::Reg, 16.0, pal().text));
        text(ui, pos2(d.right() + 6.0, r.center().y), Align2::LEFT_CENTER, second, Ts::new(Fam::Bold, 16.0, pal().text).ls(0.6));
    }
    chevron(ui.painter(), pos2(r.right() - 29.0, r.center().y), 5.0, false, Stroke::new(1.6, pal().text));
    resp
}
