//! On-screen display: a small click-through pop-up for DPI and profile
//! changes. `render` is pure CPU; `show` drives a layered window on its own thread.

use crate::ui::assets::font_bytes;
use crate::ui::palette::{Palette, pal};
use ab_glyph::{Font, FontArc, PxScale, ScaleFont, point};
use egui::Color32;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::{Mutex, Once, OnceLock};
use std::time::{Duration, Instant};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, SIZE, WPARAM};
use windows::Win32::Graphics::Gdi::*;
use windows::Win32::UI::HiDpi::{DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2, GetDpiForMonitor, MDT_EFFECTIVE_DPI, SetProcessDpiAwarenessContext};
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::w;

#[derive(Clone, Debug, PartialEq)]
pub enum OsdKind {
    Dpi { value: u16, levels: u8, active: u8 },
    Profile { name: String },
}

/// Premultiplied BGRA, top-down.
pub struct OsdImage {
    pub w: u32,
    pub h: u32,
    pub bgra: Vec<u8>,
}

const HEIGHT: f32 = 72.0;
const DPI_WIDTH: f32 = 264.0;
const PAD: f32 = 22.0;
const FADE_IN_MS: u128 = 100;
const HOLD_MS: u128 = 1400;
const FADE_OUT_MS: u128 = 300;

struct Fonts {
    reg: FontArc,
    bold: FontArc,
}

fn fonts() -> Option<&'static Fonts> {
    static FONTS: OnceLock<Option<Fonts>> = OnceLock::new();
    FONTS
        .get_or_init(|| {
            let load = |key: &str| FontArc::try_from_vec(font_bytes(key)?).ok();
            Some(Fonts { reg: load("reg")?, bold: load("bold")? })
        })
        .as_ref()
}

fn px_scale(font: &FontArc, px: f32) -> PxScale {
    PxScale::from(px * font.height_unscaled() / font.units_per_em().unwrap_or(font.height_unscaled()))
}

fn text_width(font: &FontArc, px: f32, ls: f32, s: &str) -> f32 {
    let sf = font.as_scaled(px_scale(font, px));
    let mut x = 0.0;
    let mut prev = None;
    for ch in s.chars() {
        let id = sf.glyph_id(ch);
        if let Some(p) = prev {
            x += sf.kern(p, id);
        }
        x += sf.h_advance(id) + ls;
        prev = Some(id);
    }
    x
}

fn truncate(font: &FontArc, px: f32, ls: f32, s: &str, max: f32) -> String {
    if text_width(font, px, ls, s) <= max {
        return s.to_string();
    }
    let mut chars: Vec<char> = s.chars().collect();
    while !chars.is_empty() {
        chars.pop();
        let t: String = chars.iter().collect::<String>().trim_end().to_string() + "\u{2026}";
        if text_width(font, px, ls, &t) <= max {
            return t;
        }
    }
    "\u{2026}".to_string()
}

struct Face<'a> {
    font: &'a FontArc,
    px: f32,
    ls: f32,
    color: Color32,
}

struct Canvas {
    w: usize,
    h: usize,
    px: Vec<f32>,
}

impl Canvas {
    fn blend(&mut self, x: usize, y: usize, c: [u8; 4], cover: f32) {
        let a = c[3] as f32 / 255.0 * cover;
        if a <= 0.0 {
            return;
        }
        let i = (y * self.w + x) * 4;
        let src = [c[0] as f32 / 255.0 * a, c[1] as f32 / 255.0 * a, c[2] as f32 / 255.0 * a, a];
        for k in 0..4 {
            self.px[i + k] = src[k] + self.px[i + k] * (1.0 - a);
        }
    }

    fn text(&mut self, face: Face, x: f32, top: f32, s: &str) {
        let Face { font, px, ls, color } = face;
        let sf = font.as_scaled(px_scale(font, px));
        let col = color.to_srgba_unmultiplied();
        let baseline = top + sf.ascent();
        let mut pen = x;
        let mut prev = None;
        for ch in s.chars() {
            let id = sf.glyph_id(ch);
            if let Some(p) = prev {
                pen += sf.kern(p, id);
            }
            let glyph = id.with_scale_and_position(sf.scale(), point(pen, baseline));
            if let Some(og) = font.outline_glyph(glyph) {
                let b = og.px_bounds();
                og.draw(|gx, gy, cov| {
                    let (xx, yy) = (b.min.x as i32 + gx as i32, b.min.y as i32 + gy as i32);
                    if xx >= 0 && yy >= 0 && (xx as usize) < self.w && (yy as usize) < self.h {
                        self.blend(xx as usize, yy as usize, col, cov);
                    }
                });
            }
            pen += sf.h_advance(id) + ls;
            prev = Some(id);
        }
    }
}

pub fn render(kind: &OsdKind, pal: &Palette, scale: f32) -> OsdImage {
    let fonts = fonts();
    let (title, value, width) = match kind {
        OsdKind::Dpi { value, .. } => ("DPI", value.to_string(), DPI_WIDTH),
        OsdKind::Profile { name } => {
            let (shown, w) = match fonts {
                Some(f) => {
                    let full = text_width(&f.bold, 26.0, 0.5, name);
                    let w = (2.0 * PAD + full).clamp(220.0, 440.0);
                    (truncate(&f.bold, 26.0, 0.5, name, w - 2.0 * PAD), w)
                }
                None => (name.clone(), (2.0 * PAD + name.chars().count() as f32 * 16.0).clamp(220.0, 440.0)),
            };
            ("PROFILE", shown, w)
        }
    };
    let (w, h) = ((width * scale).round() as usize, (HEIGHT * scale).round() as usize);
    let mut c = Canvas { w, h, px: vec![0.0; w * h * 4] };

    let fill = pal.surface.to_srgba_unmultiplied();
    let border = pal.ink(46).to_srgba_unmultiplied();
    let r = 12.0 * scale;
    let (cx, cy) = (w as f32 / 2.0, h as f32 / 2.0);
    for y in 0..h {
        for x in 0..w {
            let (qx, qy) = ((x as f32 + 0.5 - cx).abs() - (cx - r), (y as f32 + 0.5 - cy).abs() - (cy - r));
            let d = (qx.max(0.0).powi(2) + qy.max(0.0).powi(2)).sqrt() + qx.max(qy).min(0.0) - r;
            let inside = (0.5 - d).clamp(0.0, 1.0);
            if inside > 0.0 {
                c.blend(x, y, [fill[0], fill[1], fill[2], (0.96 * 255.0) as u8], inside);
                let ring = (d + scale + 0.5).clamp(0.0, 1.0);
                c.blend(x, y, border, inside * ring);
            }
        }
    }

    if let Some(f) = fonts {
        c.text(Face { font: &f.reg, px: 11.0 * scale, ls: 1.2 * scale, color: pal.label }, PAD * scale, 15.0 * scale, title);
        let color = if matches!(kind, OsdKind::Dpi { .. }) { pal.amber } else { pal.text };
        c.text(Face { font: &f.bold, px: 26.0 * scale, ls: 0.5 * scale, color }, PAD * scale, 31.0 * scale, &value);
    }

    if let OsdKind::Dpi { levels, active, .. } = kind {
        let n = (*levels).clamp(1, 5) as usize;
        let active = (*active as usize).min(n - 1);
        let x0 = width - PAD - (n * 8 + (n - 1) * 8) as f32;
        for i in 0..n {
            let col = if i == active { pal.amber } else { pal.track }.to_srgba_unmultiplied();
            let (dx, dy) = ((x0 + 4.0 + 16.0 * i as f32) * scale, 36.0 * scale);
            let rad = 4.0 * scale;
            for y in (dy - rad - 1.0).floor().max(0.0) as usize..((dy + rad + 1.0).ceil() as usize).min(h) {
                for x in (dx - rad - 1.0).floor().max(0.0) as usize..((dx + rad + 1.0).ceil() as usize).min(w) {
                    let d = ((x as f32 + 0.5 - dx).powi(2) + (y as f32 + 0.5 - dy).powi(2)).sqrt() - rad;
                    c.blend(x, y, col, (0.5 - d).clamp(0.0, 1.0));
                }
            }
        }
    }

    let mut bgra = Vec::with_capacity(w * h * 4);
    for p in c.px.as_chunks::<4>().0 {
        let q = |v: f32| (v * 255.0 + 0.5).clamp(0.0, 255.0) as u8;
        bgra.extend_from_slice(&[q(p[2]), q(p[1]), q(p[0]), q(p[3])]);
    }
    OsdImage { w: w as u32, h: h as u32, bgra }
}

// ------------------------------------------------------------------ window

const WM_OSD_SHOW: u32 = WM_APP + 1;
const TIMER_ID: usize = 1;

static OSD_HWND: AtomicIsize = AtomicIsize::new(0);
static PENDING: Mutex<Option<(OsdKind, Palette)>> = Mutex::new(None);
static START: Once = Once::new();
static STATE: Mutex<Option<Instant>> = Mutex::new(None);

/// Show the pop-up (any thread; the OSD thread starts on first use).
pub fn show(kind: OsdKind) {
    START.call_once(|| {
        let (tx, rx) = crossbeam_channel::bounded::<()>(1);
        let _ = std::thread::Builder::new().name("osd".into()).spawn(move || osd_thread(tx));
        let _ = rx.recv_timeout(Duration::from_secs(2));
    });
    let h = OSD_HWND.load(Ordering::Relaxed);
    if h == 0 {
        return;
    }
    *PENDING.lock().unwrap() = Some((kind, pal()));
    unsafe {
        let _ = PostMessageW(Some(HWND(h as *mut _)), WM_OSD_SHOW, WPARAM(0), LPARAM(0));
    }
}

fn blend(alpha: u8) -> BLENDFUNCTION {
    BLENDFUNCTION { BlendOp: AC_SRC_OVER as u8, BlendFlags: 0, SourceConstantAlpha: alpha, AlphaFormat: AC_SRC_ALPHA as u8 }
}

fn osd_thread(ready: crossbeam_channel::Sender<()>) {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    unsafe {
        let _ = SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW { lpfnWndProc: Some(osd_proc), hInstance: module.into(), lpszClassName: w!("GHUBX_OSD"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_LAYERED | WS_EX_TRANSPARENT | WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE,
            w!("GHUBX_OSD"),
            w!("GHUB-X OSD"),
            WS_POPUP,
            0,
            0,
            1,
            1,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return;
        };
        OSD_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        let _ = ready.send(());
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    }
}

fn alpha_at(elapsed: u128) -> Option<u8> {
    let a = if elapsed < FADE_IN_MS {
        elapsed as f32 / FADE_IN_MS as f32
    } else if elapsed < FADE_IN_MS + HOLD_MS {
        1.0
    } else if elapsed < FADE_IN_MS + HOLD_MS + FADE_OUT_MS {
        1.0 - (elapsed - FADE_IN_MS - HOLD_MS) as f32 / FADE_OUT_MS as f32
    } else {
        return None;
    };
    Some((a * 255.0).round() as u8)
}

unsafe fn present(hwnd: HWND, kind: &OsdKind, palette: &Palette) {
    unsafe {
        let fg = GetForegroundWindow();
        let mon = MonitorFromWindow(fg, MONITOR_DEFAULTTOPRIMARY);
        let (mut dx, mut dy) = (96u32, 96u32);
        let _ = GetDpiForMonitor(mon, MDT_EFFECTIVE_DPI, &mut dx, &mut dy);
        let scale = dx as f32 / 96.0;
        let mut mi = MONITORINFO { cbSize: std::mem::size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(mon, &mut mi);
        let img = render(kind, palette, scale);
        let (w, h) = (img.w as i32, img.h as i32);
        let work = mi.rcWork;
        let pos = POINT { x: work.left + ((work.right - work.left) - w) / 2, y: work.bottom - (72.0 * scale) as i32 - h };

        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmi = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() },
            ..Default::default()
        };
        let mut bits: *mut std::ffi::c_void = std::ptr::null_mut();
        if let Ok(bmp) = CreateDIBSection(Some(mem), &bmi, DIB_RGB_COLORS, &mut bits, None, 0)
            && !bits.is_null()
        {
            std::ptr::copy_nonoverlapping(img.bgra.as_ptr(), bits.cast::<u8>(), img.bgra.len());
            let old = SelectObject(mem, bmp.into());
            let (size, src) = (SIZE { cx: w, cy: h }, POINT { x: 0, y: 0 });
            let _ = UpdateLayeredWindow(hwnd, Some(screen), Some(&pos), Some(&size), Some(mem), Some(&src), COLORREF(0), Some(&blend(0)), ULW_ALPHA);
            SelectObject(mem, old);
            let _ = DeleteObject(bmp.into());
        }
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), pos.x, pos.y, w, h, SWP_NOACTIVATE);
    }
}

unsafe extern "system" fn osd_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_OSD_SHOW => {
            let Some((kind, palette)) = PENDING.lock().unwrap().take() else { return LRESULT(0) };
            let was_visible = STATE.lock().unwrap().is_some();
            unsafe {
                present(hwnd, &kind, &palette);
                if !was_visible {
                    let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
                }
                let _ = SetTimer(Some(hwnd), TIMER_ID, 16, None);
            }
            let skip = if was_visible { FADE_IN_MS } else { 0 };
            *STATE.lock().unwrap() = Some(Instant::now() - Duration::from_millis(skip as u64));
            apply_alpha(hwnd);
            LRESULT(0)
        }
        WM_TIMER if wp.0 == TIMER_ID => {
            apply_alpha(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

fn apply_alpha(hwnd: HWND) {
    let mut state = STATE.lock().unwrap();
    let Some(t0) = *state else { return };
    unsafe {
        match alpha_at(t0.elapsed().as_millis()) {
            Some(a) => {
                let _ = UpdateLayeredWindow(hwnd, None, None, None, None, None, COLORREF(0), Some(&blend(a)), ULW_ALPHA);
            }
            None => {
                let _ = KillTimer(Some(hwnd), TIMER_ID);
                let _ = ShowWindow(hwnd, SW_HIDE);
                *state = None;
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dpi(value: u16, levels: u8, active: u8) -> OsdKind {
        OsdKind::Dpi { value, levels, active }
    }

    fn px(img: &OsdImage, x: u32, y: u32) -> [u8; 4] {
        let i = ((y * img.w + x) * 4) as usize;
        [img.bgra[i + 2], img.bgra[i + 1], img.bgra[i], img.bgra[i + 3]]
    }

    fn unpremul(p: [u8; 4]) -> [i32; 3] {
        let a = p[3].max(1) as f32 / 255.0;
        [0, 1, 2].map(|k| (p[k] as f32 / a).round().min(255.0) as i32)
    }

    fn near(a: [i32; 3], c: Color32, tol: i32) -> bool {
        [c.r(), c.g(), c.b()].iter().zip(a).all(|(&c, a)| (c as i32 - a).abs() <= tol)
    }

    #[test]
    fn dpi_osd_size_and_corners() {
        let img = render(&dpi(1600, 3, 1), &Palette::ghub_dark(), 1.0);
        assert_eq!((img.w, img.h), (264, 72));
        assert_eq!(px(&img, 0, 0)[3], 0);
        assert!(px(&img, 132, 4)[3] >= 240);
    }

    #[test]
    fn scale_doubles_bitmap() {
        let img = render(&dpi(1600, 3, 1), &Palette::ghub_dark(), 2.0);
        assert_eq!((img.w, img.h), (528, 144));
    }

    #[test]
    fn active_dot_is_amber() {
        let p = Palette::ghub_dark();
        let img = render(&dpi(1600, 3, 1), &p, 1.0);
        let (n, w) = (3u32, img.w);
        let dot = |i: u32| unpremul(px(&img, w - 22 - (n * 8 + (n - 1) * 8) + 4 + 16 * i, 36));
        assert!(near(dot(1), p.amber, 12), "{:?}", dot(1));
        assert!(near(dot(0), p.track, 12), "{:?}", dot(0));
    }

    #[test]
    fn profile_width_is_clamped() {
        if fonts().is_none() {
            return;
        }
        let p = Palette::ghub_dark();
        assert_eq!(render(&OsdKind::Profile { name: "A".into() }, &p, 1.0).w, 220);
        assert_eq!(render(&OsdKind::Profile { name: "x".repeat(80) }, &p, 1.0).w, 440);
    }

    #[test]
    fn value_text_is_drawn() {
        if font_bytes("bold").is_none() {
            return;
        }
        let p = Palette::ghub_dark();
        let img = render(&dpi(1600, 3, 1), &p, 1.0);
        let mut n = 0;
        for y in 31..60 {
            for x in 22..150 {
                if near(unpremul(px(&img, x, y)), p.amber, 40) {
                    n += 1;
                }
            }
        }
        assert!(n >= 150, "{n}");
    }
}
