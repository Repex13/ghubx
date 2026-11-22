//! Windows capture for the host-driven effects: GDI screen sampling and WASAPI loopback audio.

use super::{Bands, Source};
use crate::profile::ScreenArea;
use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::Graphics::Gdi::{
    BITMAPINFO, BITMAPINFOHEADER, BI_RGB, COLORONCOLOR, CreateCompatibleBitmap, CreateCompatibleDC, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, GetDIBits, HALFTONE, HBITMAP, HDC, HGDIOBJ,
    ReleaseDC, SRCCOPY, BitBlt, SelectObject, SetBrushOrgEx, SetStretchBltMode, StretchBlt,
};
use windows::Win32::Media::Audio::Endpoints::IAudioMeterInformation;
use windows::Win32::Media::Audio::{
    AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, WAVEFORMATEX, eConsole, eRender,
};
use windows::Win32::System::Com::{CLSCTX_ALL, COINIT_MULTITHREADED, CoCreateInstance, CoInitializeEx, CoTaskMemFree};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXSCREEN, SM_CYSCREEN};

const W: i32 = 320;
const H: i32 = 180;
const QUIET: Duration = Duration::from_secs(3);
const RETRY: Duration = Duration::from_secs(1);

struct Gdi {
    screen: HDC,
    mem: HDC,
    bmp: HBITMAP,
    old: HGDIOBJ,
}

impl Gdi {
    fn new() -> Option<Self> {
        unsafe {
            let screen = GetDC(None);
            if screen.is_invalid() {
                return None;
            }
            let mem = CreateCompatibleDC(Some(screen));
            let bmp = CreateCompatibleBitmap(screen, W, H);
            if mem.is_invalid() || bmp.is_invalid() {
                if !bmp.is_invalid() {
                    let _ = DeleteObject(bmp.into());
                }
                if !mem.is_invalid() {
                    let _ = DeleteDC(mem);
                }
                ReleaseDC(None, screen);
                return None;
            }
            let old = SelectObject(mem, bmp.into());
            SetStretchBltMode(mem, COLORONCOLOR);
            Some(Self { screen, mem, bmp, old })
        }
    }
}

impl Drop for Gdi {
    fn drop(&mut self) {
        unsafe {
            SelectObject(self.mem, self.old);
            let _ = DeleteObject(self.bmp.into());
            let _ = DeleteDC(self.mem);
            ReleaseDC(Some(HWND::default()), self.screen);
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum Fmt {
    F32,
    I16,
}

struct Loopback {
    client: IAudioClient,
    capture: IAudioCaptureClient,
    fmt: Fmt,
    channels: usize,
    bands: Bands,
    text: String,
}

impl Loopback {
    fn open(device: &windows::Win32::Media::Audio::IMMDevice) -> Option<Self> {
        unsafe {
            let client: IAudioClient = device.Activate(CLSCTX_ALL, None).ok()?;
            let wf = client.GetMixFormat().ok()?;
            let (tag, bits, channels, rate) = ((*wf).wFormatTag, (*wf).wBitsPerSample, (*wf).nChannels as usize, (*wf).nSamplesPerSec);
            let float = match tag {
                3 => Some(true),
                1 => Some(false),
                0xFFFE => {
                    // WAVEFORMATEXTENSIBLE: the sub-format GUID starts 8 bytes after the 18 byte header + 2 + 4
                    let sub = std::ptr::read_unaligned((wf as *const u8).add(24) as *const u32);
                    match sub {
                        3 => Some(true),
                        1 => Some(false),
                        _ => None,
                    }
                }
                _ => None,
            };
            let fmt = match (float, bits) {
                (Some(true), 32) => Some(Fmt::F32),
                (Some(false), 16) => Some(Fmt::I16),
                _ => None,
            };
            let Some(fmt) = fmt else {
                CoTaskMemFree(Some(wf as *const _));
                return None;
            };
            let init = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, wf as *const WAVEFORMATEX, None);
            CoTaskMemFree(Some(wf as *const _));
            init.ok()?;
            let capture: IAudioCaptureClient = client.GetService().ok()?;
            client.Start().ok()?;
            let text = format!("{rate} Hz {channels} ch {}", if fmt == Fmt::F32 { "float32" } else { "int16" });
            Some(Self { client, capture, fmt, channels: channels.max(1), bands: Bands::new(rate), text })
        }
    }

    fn drain(&mut self) -> Option<(f32, f32)> {
        unsafe {
            loop {
                let n = self.capture.GetNextPacketSize().ok()?;
                if n == 0 {
                    break;
                }
                let mut data = std::ptr::null_mut::<u8>();
                let (mut frames, mut flags) = (0u32, 0u32);
                self.capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None).ok()?;
                let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null();
                for f in 0..frames as usize {
                    let mut mono = 0.0f32;
                    if !silent {
                        for c in 0..self.channels {
                            let i = f * self.channels + c;
                            mono += match self.fmt {
                                Fmt::F32 => *(data as *const f32).add(i),
                                Fmt::I16 => *(data as *const i16).add(i) as f32 / 32768.0,
                            };
                        }
                        mono /= self.channels as f32;
                    }
                    self.bands.feed(mono);
                }
                self.capture.ReleaseBuffer(frames).ok()?;
            }
        }
        Some(self.bands.take())
    }
}

impl Drop for Loopback {
    fn drop(&mut self) {
        unsafe {
            let _ = self.client.Stop();
        }
    }
}

pub struct WinSource {
    gdi: Option<Gdi>,
    lut: [f32; 256],
    meter_device: Option<(windows::Win32::Media::Audio::IMMDevice, IAudioMeterInformation, String)>,
    last_check: Option<Instant>,
    loopback: Option<Loopback>,
    last_loud: Option<Instant>,
    last_try: Option<Instant>,
    last_open: Option<Instant>,
    events: Vec<String>,
}

impl WinSource {
    pub fn new() -> Self {
        unsafe {
            let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        }
        let mut lut = [0.0; 256];
        for (i, v) in lut.iter_mut().enumerate() {
            let c = i as f32 / 255.0;
            *v = if c <= 0.04045 { c / 12.92 } else { ((c + 0.055) / 1.055).powf(2.4) };
        }
        Self { gdi: None, lut, meter_device: None, last_check: None, loopback: None, last_loud: None, last_try: None, last_open: None, events: Vec::new() }
    }

    fn setup_meter(&mut self) -> bool {
        if self.meter_device.is_some() {
            return true;
        }
        if self.last_try.is_some_and(|t| t.elapsed() < RETRY) {
            return false;
        }
        self.last_try = Some(Instant::now());
        self.meter_device = resolve_default();
        self.meter_device.is_some()
    }

    /// While quiet: follow a change of the default render endpoint (speakers to headset).
    fn recheck_device(&mut self, now: Instant) {
        if self.last_check.is_some_and(|t| now.duration_since(t) < Duration::from_secs(2)) {
            return;
        }
        self.last_check = Some(now);
        if let Some(new) = resolve_default()
            && self.meter_device.as_ref().is_none_or(|(_, _, id)| *id != new.2)
        {
            self.close_loopback();
            self.meter_device = Some(new);
        }
    }

    fn close_loopback(&mut self) {
        if self.loopback.take().is_some() {
            self.events.push("loopback closed".into());
        }
    }
}

impl Default for WinSource {
    fn default() -> Self {
        Self::new()
    }
}

impl Source for WinSource {
    fn screen(&mut self, area: ScreenArea) -> Option<[u8; 3]> {
        if self.gdi.is_none() {
            self.gdi = Gdi::new();
        }
        let r = self.try_screen(area);
        if r.is_none() {
            self.gdi = None;
        }
        r
    }

    fn audio(&mut self) -> Option<(f32, f32)> {
        self.audio_inner()
    }

    fn release(&mut self) {
        self.release_inner();
    }

    fn describe(&self) -> String {
        "meter first, loopback of the default render endpoint opened on demand (closed after 3 s of silence)".into()
    }

    fn events(&mut self) -> Vec<String> {
        std::mem::take(&mut self.events)
    }
}

fn resolve_default() -> Option<(windows::Win32::Media::Audio::IMMDevice, IAudioMeterInformation, String)> {
    unsafe {
        let en: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL).ok()?;
        let dev = en.GetDefaultAudioEndpoint(eRender, eConsole).ok()?;
        let id = dev.GetId().ok()?;
        let name = id.to_string().unwrap_or_default();
        CoTaskMemFree(Some(id.0 as *const _));
        let meter: IAudioMeterInformation = dev.Activate(CLSCTX_ALL, None).ok()?;
        Some((dev, meter, name))
    }
}

/// 1:1 linear-light average of an area of the primary screen (every pixel).
pub fn exact_screen(area: ScreenArea) -> Option<[u8; 3]> {
    let lut = WinSource::new().lut;
    unsafe {
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        let (x0, y0, x1, y1) = area.rect();
        let (sx, sy) = ((x0 * sw as f32) as i32, (y0 * sh as f32) as i32);
        let (cw, ch) = ((((x1 - x0) * sw as f32) as i32).max(1), (((y1 - y0) * sh as f32) as i32).max(1));
        let screen = GetDC(None);
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, cw, ch);
        let old = SelectObject(mem, bmp.into());
        let ok = BitBlt(mem, 0, 0, cw, ch, Some(screen), sx, sy, SRCCOPY).is_ok();
        SelectObject(mem, old);
        let mut bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: cw, biHeight: -ch, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        let mut px = vec![0u8; (cw * ch * 4) as usize];
        let lines = if ok { GetDIBits(mem, bmp, 0, ch as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS) } else { 0 };
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if lines == 0 {
            return None;
        }
        let mut sum = [0.0f64; 3];
        for p in px.as_chunks::<4>().0 {
            sum[0] += lut[p[2] as usize] as f64;
            sum[1] += lut[p[1] as usize] as f64;
            sum[2] += lut[p[0] as usize] as f64;
        }
        let n = (cw * ch) as f64;
        Some(sum.map(|s| {
            let l = (s / n) as f32;
            let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
            (c * 255.0).round().clamp(0.0, 255.0) as u8
        }))
    }
}

/// The primary screen scaled to fit `max_w` x `max_h` (aspect kept, HALFTONE
/// stretch), as RGBA bytes. Read-only: the sample editor's background and a probe.
pub fn screen_picture(max_w: u32, max_h: u32) -> Option<(Vec<u8>, u32, u32)> {
    unsafe {
        let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
        if sw <= 0 || sh <= 0 || max_w == 0 || max_h == 0 {
            return None;
        }
        let k = (max_w as f32 / sw as f32).min(max_h as f32 / sh as f32).min(1.0);
        let (w, h) = ((sw as f32 * k).round().max(1.0) as i32, (sh as f32 * k).round().max(1.0) as i32);
        let screen = GetDC(None);
        if screen.is_invalid() {
            return None;
        }
        let mem = CreateCompatibleDC(Some(screen));
        let bmp = CreateCompatibleBitmap(screen, w, h);
        if mem.is_invalid() || bmp.is_invalid() {
            if !bmp.is_invalid() {
                let _ = DeleteObject(bmp.into());
            }
            if !mem.is_invalid() {
                let _ = DeleteDC(mem);
            }
            ReleaseDC(None, screen);
            return None;
        }
        let old = SelectObject(mem, bmp.into());
        SetStretchBltMode(mem, HALFTONE);
        let _ = SetBrushOrgEx(mem, 0, 0, None);
        let ok = StretchBlt(mem, 0, 0, w, h, Some(screen), 0, 0, sw, sh, SRCCOPY).as_bool();
        // the bitmap is selected into `mem`; GetDIBits needs it deselected on some drivers
        SelectObject(mem, old);
        let mut bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        let mut px = vec![0u8; (w * h * 4) as usize];
        let lines = if ok { GetDIBits(mem, bmp, 0, h as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS) } else { 0 };
        let _ = DeleteObject(bmp.into());
        let _ = DeleteDC(mem);
        ReleaseDC(None, screen);
        if lines == 0 {
            return None;
        }
        // BGRA -> RGBA
        for p in px.as_chunks_mut::<4>().0 {
            p.swap(0, 2);
        }
        Some((px, w as u32, h as u32))
    }
}

impl WinSource {
    fn try_screen(&self, area: ScreenArea) -> Option<[u8; 3]> {
        let g = self.gdi.as_ref()?;
        unsafe {
            let (sw, sh) = (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN));
            if sw <= 0 || sh <= 0 {
                return None;
            }
            let (x0, y0, x1, y1) = area.rect();
            let (sx, sy) = ((x0 * sw as f32) as i32, (y0 * sh as f32) as i32);
            let (cw, ch) = ((((x1 - x0) * sw as f32) as i32).max(1), (((y1 - y0) * sh as f32) as i32).max(1));
            if !StretchBlt(g.mem, 0, 0, W, H, Some(g.screen), sx, sy, cw, ch, SRCCOPY).as_bool() {
                return None;
            }
            let mut bi = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: W, biHeight: -H, biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
            let mut px = vec![0u8; (W * H * 4) as usize];
            // the bitmap is selected into `mem`; GetDIBits needs it deselected on some drivers
            SelectObject(g.mem, g.old);
            let lines = GetDIBits(g.mem, g.bmp, 0, H as u32, Some(px.as_mut_ptr() as *mut _), &mut bi, DIB_RGB_COLORS);
            SelectObject(g.mem, g.bmp.into());
            if lines == 0 {
                return None;
            }
            let mut sum = [0.0f64; 3];
            for p in px.as_chunks::<4>().0 {
                sum[0] += self.lut[p[2] as usize] as f64;
                sum[1] += self.lut[p[1] as usize] as f64;
                sum[2] += self.lut[p[0] as usize] as f64;
            }
            let n = (W * H) as f64;
            Some(sum.map(|s| {
                let l = (s / n) as f32;
                let c = if l <= 0.0031308 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
                (c * 255.0).round().clamp(0.0, 255.0) as u8
            }))
        }
    }

    fn audio_inner(&mut self) -> Option<(f32, f32)> {
        if !self.setup_meter() {
            return None;
        }
        let now = Instant::now();
        let peak = match &self.meter_device {
            Some((_, m, _)) => unsafe { m.GetPeakValue() },
            None => return None,
        };
        let Ok(peak) = peak else {
            self.close_loopback();
            self.meter_device = None;
            return None;
        };
        if peak > 0.0 {
            self.last_loud = Some(now);
        }
        let quiet = self.last_loud.is_none_or(|t| now.duration_since(t) > QUIET);
        if quiet {
            self.close_loopback();
            self.recheck_device(now);
            return Some((0.0, 0.0));
        }
        if self.loopback.is_none() && self.last_open.is_none_or(|t| t.elapsed() >= RETRY) {
            self.last_open = Some(now);
            if let Some((dev, _, _)) = &self.meter_device
                && let Some(lb) = Loopback::open(dev)
            {
                self.events.push(format!("loopback opened ({})", lb.text));
                self.loopback = Some(lb);
            }
        }
        match &mut self.loopback {
            Some(lb) => match lb.drain() {
                Some(v) => Some(v),
                None => {
                    self.close_loopback();
                    self.meter_device = None;
                    Some((peak, peak))
                }
            },
            None => Some((peak, peak)),
        }
    }

    fn release_inner(&mut self) {
        self.close_loopback();
        self.meter_device = None;
        self.last_loud = None;
    }
}
