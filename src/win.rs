//! Windows platform layer: input injection, foreground app, process
//! control (G HUB), autostart, idle time and the wheel hook used for
//! scroll acceleration.

use std::sync::atomic::{AtomicBool, AtomicIsize, AtomicU32, Ordering};
use std::sync::OnceLock;
use std::time::Duration;
use windows::Win32::Foundation::{CloseHandle, ERROR_ALREADY_EXISTS, GetLastError, HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateBitmap, DeleteObject};
use windows::Win32::UI::Input::KeyboardAndMouse::*;
use windows::Win32::UI::Shell::*;
use windows::Win32::UI::WindowsAndMessaging::*;
use windows::core::{PCWSTR, w};

/// Marker in `dwExtraInfo` so our own hook ignores what we inject.
pub const INJECT_TAG: usize = 0x6748_5558; // "gHUX"

fn send(inputs: &[INPUT]) {
    unsafe {
        SendInput(inputs, std::mem::size_of::<INPUT>() as i32);
    }
}

fn mouse_input(dx: i32, dy: i32, data: i32, flags: MOUSE_EVENT_FLAGS) -> INPUT {
    INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 {
            mi: MOUSEINPUT { dx, dy, mouseData: data as u32, dwFlags: flags, time: 0, dwExtraInfo: INJECT_TAG },
        },
    }
}

/// Key press/release. Uses hardware scan codes (DirectInput/raw-input games
/// read those), falling back to the virtual key for keys without one.
pub fn key(vk: u16, down: bool) {
    if let Some(btn) = mouse_vk(vk) {
        return mouse(btn, down);
    }
    let sc = unsafe { MapVirtualKeyW(vk as u32, MAPVK_VK_TO_VSC_EX) };
    let mut flags = if down { KEYBD_EVENT_FLAGS(0) } else { KEYEVENTF_KEYUP };
    let (w_vk, w_scan) = if sc & 0xFF != 0 {
        flags |= KEYEVENTF_SCANCODE;
        if sc & 0xFF00 == 0xE000 {
            flags |= KEYEVENTF_EXTENDEDKEY;
        }
        (0, (sc & 0xFF) as u16)
    } else {
        (vk, 0)
    };
    send(&[INPUT {
        r#type: INPUT_KEYBOARD,
        Anonymous: INPUT_0 {
            ki: KEYBDINPUT { wVk: VIRTUAL_KEY(w_vk), wScan: w_scan, dwFlags: flags, time: 0, dwExtraInfo: INJECT_TAG },
        },
    }]);
}

/// Type text: one `SendInput` call per character (down + up for each UTF-16 unit); newline is
/// Enter, tab is Tab, carriage returns are skipped.
pub fn type_text(s: &str) {
    for ch in s.chars() {
        match ch {
            '\r' => {}
            '\n' | '\t' => {
                let vk = if ch == '\n' { 0x0D } else { 0x09 };
                key(vk, true);
                key(vk, false);
            }
            _ => {
                let mut units = [0u16; 2];
                let mut inputs = Vec::new();
                for &u in ch.encode_utf16(&mut units).iter() {
                    for flags in [KEYEVENTF_UNICODE, KEYEVENTF_UNICODE | KEYEVENTF_KEYUP] {
                        inputs.push(INPUT {
                            r#type: INPUT_KEYBOARD,
                            Anonymous: INPUT_0 { ki: KEYBDINPUT { wVk: VIRTUAL_KEY(0), wScan: u, dwFlags: flags, time: 0, dwExtraInfo: INJECT_TAG } },
                        });
                    }
                }
                send(&inputs);
            }
        }
    }
}

/// Open a file, folder, URL or program the way Explorer would (no shell parsing of the path),
/// started in its own folder.
pub fn launch(path: &str) {
    use windows::core::HSTRING;
    let dir = std::path::Path::new(path).parent().filter(|d| d.is_dir()).map(|d| HSTRING::from(d.as_os_str()));
    let dir = dir.as_ref().map_or(PCWSTR::null(), |d| PCWSTR(d.as_ptr()));
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from(path), None, dir, SW_SHOWNORMAL);
    }
}

/// VK_LBUTTON.. VK_XBUTTON2 -> mouse button index 0..4.
pub fn mouse_vk(vk: u16) -> Option<u8> {
    match vk {
        0x01 => Some(0),
        0x02 => Some(1),
        0x04 => Some(2),
        0x05 => Some(3),
        0x06 => Some(4),
        _ => None,
    }
}

/// Mouse button 0=left 1=right 2=middle 3=back 4=forward.
pub fn mouse(btn: u8, down: bool) {
    let (flags, data) = match (btn, down) {
        (0, true) => (MOUSEEVENTF_LEFTDOWN, 0),
        (0, false) => (MOUSEEVENTF_LEFTUP, 0),
        (1, true) => (MOUSEEVENTF_RIGHTDOWN, 0),
        (1, false) => (MOUSEEVENTF_RIGHTUP, 0),
        (2, true) => (MOUSEEVENTF_MIDDLEDOWN, 0),
        (2, false) => (MOUSEEVENTF_MIDDLEUP, 0),
        (3, true) => (MOUSEEVENTF_XDOWN, 1),
        (3, false) => (MOUSEEVENTF_XUP, 1),
        (4, true) => (MOUSEEVENTF_XDOWN, 2),
        (4, false) => (MOUSEEVENTF_XUP, 2),
        _ => return,
    };
    send(&[mouse_input(0, 0, data, flags)]);
}

pub fn mouse_move(dx: i32, dy: i32) {
    send(&[mouse_input(dx, dy, 0, MOUSEEVENTF_MOVE)]);
}

/// Vertical wheel; positive = up. 120 = one notch.
pub fn wheel(delta: i32) {
    send(&[mouse_input(0, 0, delta, MOUSEEVENTF_WHEEL)]);
}

/// Horizontal wheel; positive = right.
pub fn hwheel(delta: i32) {
    send(&[mouse_input(0, 0, delta, MOUSEEVENTF_HWHEEL)]);
}

/// Whether a virtual key is held right now (`GetAsyncKeyState`; Lua's IsKeyPressed).
pub fn is_vk_down(vk: u16) -> bool {
    unsafe { (GetAsyncKeyState(vk as i32) as u16 & 0x8000) != 0 }
}

/// Whether a toggle key (caps/num/scroll lock) is latched on (`GetKeyState` low bit; Lua's IsKeyLockOn).
pub fn is_vk_toggled(vk: i32) -> bool {
    unsafe { (GetKeyState(vk) & 1) != 0 }
}

/// Cursor position in screen coordinates (Lua's GetMousePosition).
pub fn cursor_pos() -> (i32, i32) {
    let mut pt = POINT::default();
    unsafe {
        let _ = GetCursorPos(&mut pt);
    }
    (pt.x, pt.y)
}

/// Move the cursor in screen coordinates (Lua's SetMousePosition); a no-op in passive mode.
pub fn set_cursor_pos(x: i32, y: i32) {
    if PASSIVE.load(Ordering::Relaxed) {
        return;
    }
    unsafe {
        let _ = SetCursorPos(x, y);
    }
}

/// Full image path of the foreground window's process.
pub fn foreground_path() -> Option<String> {
    use windows::Win32::System::Threading::{
        OpenProcess, PROCESS_NAME_WIN32, PROCESS_QUERY_LIMITED_INFORMATION, QueryFullProcessImageNameW,
    };
    unsafe {
        let hwnd = GetForegroundWindow();
        if hwnd.0.is_null() {
            return None;
        }
        let mut pid = 0u32;
        GetWindowThreadProcessId(hwnd, Some(&mut pid));
        if pid == 0 {
            return None;
        }
        let h = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid).ok()?;
        let mut buf = [0u16; 1024];
        let mut len = buf.len() as u32;
        let ok = QueryFullProcessImageNameW(h, PROCESS_NAME_WIN32, windows::core::PWSTR(buf.as_mut_ptr()), &mut len);
        let _ = CloseHandle(h);
        ok.ok()?;
        Some(String::from_utf16_lossy(&buf[..len as usize]))
    }
}

/// (pid, exe name) for every running process.
pub fn processes() -> Vec<(u32, String)> {
    use windows::Win32::System::Diagnostics::ToolHelp::*;
    let mut out = Vec::new();
    unsafe {
        let Ok(snap) = CreateToolhelp32Snapshot(TH32CS_SNAPPROCESS, 0) else { return out };
        let mut e = PROCESSENTRY32W { dwSize: std::mem::size_of::<PROCESSENTRY32W>() as u32, ..Default::default() };
        let mut ok = Process32FirstW(snap, &mut e).is_ok();
        while ok {
            let n = e.szExeFile.iter().position(|&c| c == 0).unwrap_or(e.szExeFile.len());
            out.push((e.th32ProcessID, String::from_utf16_lossy(&e.szExeFile[..n])));
            ok = Process32NextW(snap, &mut e).is_ok();
        }
        let _ = CloseHandle(snap);
    }
    out
}

/// G HUB's device service; while it runs, it owns the mouse.
pub fn ghub_running() -> bool {
    processes().iter().any(|(_, n)| n.eq_ignore_ascii_case("lghub_agent.exe"))
}

/// Close G HUB's user-session processes (tray first so it can't respawn the
/// agent). The updater service is left alone. Returns how many were closed.
pub fn close_ghub() -> usize {
    use windows::Win32::System::Threading::{OpenProcess, PROCESS_TERMINATE, TerminateProcess};
    let order = ["lghub_system_tray.exe", "lghub.exe", "lghub_agent.exe"];
    let procs = processes();
    let mut n = 0;
    for name in order {
        for (pid, _) in procs.iter().filter(|(_, p)| p.eq_ignore_ascii_case(name)) {
            unsafe {
                if let Ok(h) = OpenProcess(PROCESS_TERMINATE, false, *pid) {
                    if TerminateProcess(h, 0).is_ok() {
                        n += 1;
                    }
                    let _ = CloseHandle(h);
                }
            }
        }
    }
    n
}

/// Start G HUB again (the tray app launches the agent).
pub fn start_ghub() {
    let tray = r"C:\Program Files\LGHUB\system_tray\lghub_system_tray.exe";
    let exe = if std::path::Path::new(tray).exists() { tray } else { r"C:\Program Files\LGHUB\lghub.exe" };
    let _ = std::process::Command::new(exe).spawn();
}

/// Milliseconds since the last keyboard/mouse input anywhere.
pub fn idle_ms() -> u32 {
    let mut li = LASTINPUTINFO { cbSize: std::mem::size_of::<LASTINPUTINFO>() as u32, dwTime: 0 };
    unsafe {
        if !GetLastInputInfo(&mut li).as_bool() {
            return 0;
        }
        windows::Win32::System::SystemInformation::GetTickCount().wrapping_sub(li.dwTime)
    }
}

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";

/// Add/remove GHUB-X from the per-user Run key (`--minimized` starts it in the tray).
pub fn set_autostart(on: bool, minimized: bool) -> bool {
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;
    unsafe {
        if on {
            let Ok(exe) = std::env::current_exe() else { return false };
            let flag = if minimized { " --minimized" } else { "" };
            let cmd: Vec<u16> = format!("\"{}\"{flag}", exe.display()).encode_utf16().chain([0]).collect();
            RegSetKeyValueW(
                HKEY_CURRENT_USER,
                &HSTRING::from(RUN_KEY),
                &HSTRING::from("GHUB-X"),
                REG_SZ.0,
                Some(cmd.as_ptr().cast()),
                (cmd.len() * 2) as u32,
            )
            .is_ok()
        } else {
            let r = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &HSTRING::from("GHUB-X"));
            r.is_ok() || r == windows::Win32::Foundation::ERROR_FILE_NOT_FOUND
        }
    }
}

// ------------------------------------------------------------ tray / background

pub const WM_TRAY: u32 = WM_APP + 1;
pub const WM_TRAY_SHOW: u32 = WM_APP + 2;
pub const WM_TRAY_QUIT: u32 = WM_APP + 3;
static DEV: AtomicBool = AtomicBool::new(false);

/// `--dev`: a second, harmless instance with its own mutex, tray class and tooltip.
pub fn set_dev(on: bool) {
    DEV.store(on, Ordering::Relaxed);
}

pub fn is_dev() -> bool {
    DEV.load(Ordering::Relaxed)
}

fn tray_class() -> PCWSTR {
    if is_dev() { w!("GHUBX_TRAY_DEV") } else { w!("GHUBX_TRAY") }
}
const TRAY_ID: u32 = 1;
const MENU_OPEN: usize = 1;
const MENU_QUIT: usize = 2;
const ICON_SIZE: usize = 32;

static MAIN_HWND: AtomicIsize = AtomicIsize::new(0);
static TRAY_HWND: AtomicIsize = AtomicIsize::new(0);
static TRAY_ICON: AtomicIsize = AtomicIsize::new(0);
static TASKBAR_CREATED: AtomicU32 = AtomicU32::new(0);
static TRAY_READY: AtomicBool = AtomicBool::new(false);
static HIDDEN: AtomicBool = AtomicBool::new(false);
static BALLOON_SHOWN: AtomicBool = AtomicBool::new(false);
static CALLBACKS: OnceLock<TrayCallbacks> = OnceLock::new();

struct TrayCallbacks {
    repaint: Box<dyn Fn() + Send + Sync>,
    quit: Box<dyn Fn() + Send + Sync>,
}

/// Square RGBA app icon (tray 32, window 64): a #1196FF disc with a white X,
/// drawn on a 32-unit grid and scaled to `size`.
pub fn app_icon_rgba(size: usize) -> Vec<u8> {
    const SS: usize = 4;
    let k = size as f32 / 32.0;
    let c = 16.0f32;
    let seg = |px: f32, py: f32, (ax, ay): (f32, f32), (bx, by): (f32, f32)| {
        let (dx, dy) = (bx - ax, by - ay);
        let t = (((px - ax) * dx + (py - ay) * dy) / (dx * dx + dy * dy)).clamp(0.0, 1.0);
        ((px - ax - t * dx).powi(2) + (py - ay - t * dy).powi(2)).sqrt()
    };
    let (a, b, d, e) = ((10.0, 10.0), (22.0, 22.0), (22.0, 10.0), (10.0, 22.0));
    let mut out = vec![0u8; size * size * 4];
    for y in 0..size {
        for x in 0..size {
            let (mut disc, mut cross) = (0.0f32, 0.0f32);
            for sy in 0..SS {
                for sx in 0..SS {
                    let px = (x as f32 + (sx as f32 + 0.5) / SS as f32) / k;
                    let py = (y as f32 + (sy as f32 + 0.5) / SS as f32) / k;
                    if ((px - c).powi(2) + (py - c).powi(2)).sqrt() <= c - 0.5 {
                        disc += 1.0;
                        if seg(px, py, a, b) <= 1.0 || seg(px, py, d, e) <= 1.0 {
                            cross += 1.0;
                        }
                    }
                }
            }
            let alpha = disc / (SS * SS) as f32;
            let white = if disc > 0.0 { cross / disc } else { 0.0 };
            let mix = |base: f32| (base + (255.0 - base) * white).round() as u8;
            let i = (y * size + x) * 4;
            out[i..i + 4].copy_from_slice(&[mix(0x11 as f32), mix(0x96 as f32), 255, (alpha * 255.0).round() as u8]);
        }
    }
    out
}

fn create_icon() -> Option<HICON> {
    let rgba = app_icon_rgba(ICON_SIZE);
    let mut bgra = Vec::with_capacity(rgba.len());
    for p in rgba.as_chunks::<4>().0 {
        let a = p[3] as u32;
        let pm = |v: u8| (v as u32 * a / 255) as u8;
        bgra.extend_from_slice(&[pm(p[2]), pm(p[1]), pm(p[0]), p[3]]);
    }
    let mask = [0u8; ICON_SIZE * ICON_SIZE / 8];
    unsafe {
        let color = CreateBitmap(ICON_SIZE as i32, ICON_SIZE as i32, 1, 32, Some(bgra.as_ptr().cast()));
        let mono = CreateBitmap(ICON_SIZE as i32, ICON_SIZE as i32, 1, 1, Some(mask.as_ptr().cast()));
        let info = ICONINFO { fIcon: true.into(), xHotspot: 0, yHotspot: 0, hbmMask: mono, hbmColor: color };
        let icon = CreateIconIndirect(&info).ok();
        let _ = DeleteObject(color.into());
        let _ = DeleteObject(mono.into());
        icon
    }
}

fn copy_wide<const N: usize>(dst: &mut [u16; N], s: &str) {
    for (d, c) in dst.iter_mut().zip(s.encode_utf16().take(N - 1)) {
        *d = c;
    }
}

fn tray_data(hwnd: HWND) -> NOTIFYICONDATAW {
    NOTIFYICONDATAW { cbSize: std::mem::size_of::<NOTIFYICONDATAW>() as u32, hWnd: hwnd, uID: TRAY_ID, ..Default::default() }
}

fn add_tray_icon(hwnd: HWND) {
    let mut nid = tray_data(hwnd);
    nid.uFlags = NIF_MESSAGE | NIF_ICON | NIF_TIP;
    nid.uCallbackMessage = WM_TRAY;
    nid.hIcon = HICON(TRAY_ICON.load(Ordering::Relaxed) as *mut _);
    copy_wide(&mut nid.szTip, if is_dev() { "GHUB-X (dev)" } else { "GHUB-X" });
    unsafe {
        let _ = Shell_NotifyIconW(NIM_ADD, &nid);
    }
}

/// Remove the tray icon (callable from any thread).
pub fn remove_tray_icon() {
    let h = TRAY_HWND.load(Ordering::Relaxed);
    if h != 0 {
        let nid = tray_data(HWND(h as *mut _));
        unsafe {
            let _ = Shell_NotifyIconW(NIM_DELETE, &nid);
        }
    }
}

fn tray_balloon(msg: &str) {
    let h = TRAY_HWND.load(Ordering::Relaxed);
    if h == 0 {
        return;
    }
    let mut nid = tray_data(HWND(h as *mut _));
    nid.uFlags = NIF_INFO;
    nid.dwInfoFlags = NIIF_INFO;
    copy_wide(&mut nid.szInfoTitle, "GHUB-X");
    copy_wide(&mut nid.szInfo, msg);
    unsafe {
        let _ = Shell_NotifyIconW(NIM_MODIFY, &nid);
    }
}

fn main_hwnd() -> Option<HWND> {
    let h = MAIN_HWND.load(Ordering::Relaxed);
    (h != 0).then_some(HWND(h as *mut _))
}

/// True once the tray window exists (a hidden main window is only reachable then).
pub fn tray_ready() -> bool {
    TRAY_READY.load(Ordering::Relaxed)
}

/// The main window's handle (0 before the tray started).
pub fn main_window_handle() -> isize {
    MAIN_HWND.load(Ordering::Relaxed)
}

fn file_dialog(owner: isize, title: &str, default_name: Option<&str>, filter_name: &str, pattern: &str, save: bool) -> Option<String> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, GetSaveFileNameW, OFN_FILEMUSTEXIST, OFN_NOCHANGEDIR, OFN_OVERWRITEPROMPT, OFN_PATHMUSTEXIST, OPENFILENAMEW};
    let (title, filter_name, pattern, default_name) = (title.to_string(), filter_name.to_string(), pattern.to_string(), default_name.map(str::to_string));
    std::thread::spawn(move || unsafe {
        let mut file = [0u16; 1024];
        if let Some(d) = &default_name {
            for (slot, c) in file.iter_mut().zip(d.encode_utf16().take(1023)) {
                *slot = c;
            }
        }
        let mut filter: Vec<u16> = format!("{filter_name} ({pattern})\0{pattern}\0All files (*.*)\0*.*\0\0").encode_utf16().collect();
        filter.push(0);
        let title: Vec<u16> = format!("{title}\0").encode_utf16().collect();
        let ext: Vec<u16> = format!("{}\0", pattern.rsplit('.').next().unwrap_or("")).encode_utf16().collect();
        let mut ofn = OPENFILENAMEW {
            lStructSize: std::mem::size_of::<OPENFILENAMEW>() as u32,
            hwndOwner: HWND(owner as *mut _),
            lpstrFilter: PCWSTR(filter.as_ptr()),
            lpstrFile: windows::core::PWSTR(file.as_mut_ptr()),
            nMaxFile: file.len() as u32,
            lpstrTitle: PCWSTR(title.as_ptr()),
            lpstrDefExt: if save { PCWSTR(ext.as_ptr()) } else { PCWSTR::null() },
            Flags: if save { OFN_OVERWRITEPROMPT | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR } else { OFN_FILEMUSTEXIST | OFN_PATHMUSTEXIST | OFN_NOCHANGEDIR },
            ..Default::default()
        };
        let ok = if save { GetSaveFileNameW(&mut ofn) } else { GetOpenFileNameW(&mut ofn) };
        ok.as_bool().then(|| {
            let end = file.iter().position(|&c| c == 0).unwrap_or(file.len());
            String::from_utf16_lossy(&file[..end])
        })
    })
    .join()
    .ok()
    .flatten()
}

/// Native "open file" dialog; blocks until closed. Returns the full path.
pub fn pick_file(owner: isize, title: &str, filter_name: &str, pattern: &str) -> Option<String> {
    file_dialog(owner, title, None, filter_name, pattern, false)
}

/// Native "save file" dialog; blocks until closed. Returns the full path.
pub fn save_file(owner: isize, title: &str, default_name: &str, filter_name: &str, pattern: &str) -> Option<String> {
    file_dialog(owner, title, Some(default_name), filter_name, pattern, true)
}

/// Open dialog for `*.exe`.
pub fn pick_exe(owner: isize) -> Option<String> {
    pick_file(owner, "Choose a game or app", "Programs", "*.exe")
}

/// VK and whether Shift is needed to type `ch` on the current layout
/// (`None` for characters that are not on it or need Ctrl/Alt).
pub fn char_key(ch: char) -> Option<(u16, bool)> {
    let unit = u16::try_from(ch as u32).ok()?;
    let r = unsafe { VkKeyScanW(unit) };
    if r == -1 || (r >> 8) & 0b110 != 0 {
        return None;
    }
    Some(((r & 0xFF) as u16, (r >> 8) & 1 != 0))
}

pub fn main_hidden() -> bool {
    HIDDEN.load(Ordering::Relaxed)
}

/// Show and focus the main window (raw Win32: eframe runs no frames while it is hidden).
pub fn show_main() {
    let Some(h) = main_hwnd() else { return };
    unsafe {
        let _ = ShowWindow(h, SW_SHOW);
        if IsIconic(h).as_bool() {
            let _ = ShowWindow(h, SW_RESTORE);
        }
        let _ = SetForegroundWindow(h);
    }
    HIDDEN.store(false, Ordering::Relaxed);
    if let Some(cb) = CALLBACKS.get() {
        (cb.repaint)();
    }
}

/// Hide the main window into the tray; the first hide per run shows a balloon.
pub fn hide_main() {
    let Some(h) = main_hwnd().filter(|_| tray_ready()) else { return };
    unsafe {
        let _ = ShowWindow(h, SW_HIDE);
    }
    HIDDEN.store(true, Ordering::Relaxed);
    if !BALLOON_SHOWN.swap(true, Ordering::Relaxed) {
        tray_balloon("GHUB-X is still running. Right-click the tray icon to quit.");
    }
}

fn tray_quit() {
    if let Some(cb) = CALLBACKS.get() {
        (cb.quit)();
    }
}

fn tray_menu(hwnd: HWND) {
    unsafe {
        let Ok(menu) = CreatePopupMenu() else { return };
        let _ = AppendMenuW(menu, MF_STRING, MENU_OPEN, w!("Open GHUB-X"));
        let _ = AppendMenuW(menu, MF_STRING, MENU_QUIT, w!("Quit GHUB-X"));
        let mut pt = POINT::default();
        let _ = GetCursorPos(&mut pt);
        let _ = SetForegroundWindow(hwnd);
        let cmd = TrackPopupMenu(menu, TPM_RETURNCMD | TPM_RIGHTBUTTON | TPM_BOTTOMALIGN, pt.x, pt.y, Some(0), hwnd, None);
        let _ = PostMessageW(Some(hwnd), WM_NULL, WPARAM(0), LPARAM(0));
        let _ = DestroyMenu(menu);
        match cmd.0 as usize {
            MENU_OPEN => show_main(),
            MENU_QUIT => tray_quit(),
            _ => {}
        }
    }
}

unsafe extern "system" fn tray_proc(hwnd: HWND, msg: u32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    match msg {
        WM_TRAY => {
            match (lp.0 as u32) & 0xFFFF {
                WM_LBUTTONUP | WM_LBUTTONDBLCLK => show_main(),
                WM_RBUTTONUP => tray_menu(hwnd),
                _ => {}
            }
            LRESULT(0)
        }
        WM_TRAY_SHOW => {
            show_main();
            LRESULT(0)
        }
        WM_TRAY_QUIT => {
            tray_quit();
            LRESULT(0)
        }
        WM_ENDSESSION => {
            if wp.0 != 0 {
                tray_quit();
            }
            LRESULT(0)
        }
        m if m != 0 && m == TASKBAR_CREATED.load(Ordering::Relaxed) => {
            add_tray_icon(hwnd);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, msg, wp, lp) },
    }
}

/// Tray icon on its own thread with a message-only window. `repaint` wakes the
/// UI after a show; `quit` runs the full shutdown and must not return.
pub fn start_tray(
    hwnd: isize,
    start_hidden: bool,
    repaint: impl Fn() + Send + Sync + 'static,
    quit: impl Fn() + Send + Sync + 'static,
) {
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    MAIN_HWND.store(hwnd, Ordering::Relaxed);
    if CALLBACKS.set(TrayCallbacks { repaint: Box::new(repaint), quit: Box::new(quit) }).is_err() {
        return;
    }
    let (ready_tx, ready_rx) = crossbeam_channel::bounded::<()>(1);
    let spawned = std::thread::Builder::new().name("tray".into()).spawn(move || unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW { lpfnWndProc: Some(tray_proc), hInstance: module.into(), lpszClassName: tray_class(), ..Default::default() };
        RegisterClassW(&class);
        let Ok(hwnd) = CreateWindowExW(
            WS_EX_TOOLWINDOW,
            tray_class(),
            w!("GHUB-X tray"),
            WS_POPUP,
            0,
            0,
            0,
            0,
            None,
            None,
            Some(module.into()),
            None,
        ) else {
            return;
        };
        TRAY_HWND.store(hwnd.0 as isize, Ordering::Relaxed);
        TASKBAR_CREATED.store(RegisterWindowMessageW(w!("TaskbarCreated")), Ordering::Relaxed);
        if let Some(icon) = create_icon() {
            TRAY_ICON.store(icon.0 as isize, Ordering::Relaxed);
        }
        add_tray_icon(hwnd);
        TRAY_READY.store(true, Ordering::Relaxed);
        let _ = ready_tx.send(());
        let mut m = MSG::default();
        while GetMessageW(&mut m, None, 0, 0).as_bool() {
            let _ = TranslateMessage(&m);
            DispatchMessageW(&m);
        }
    });
    if spawned.is_ok() {
        let _ = ready_rx.recv_timeout(Duration::from_secs(2));
    }
    if start_hidden {
        HIDDEN.store(tray_ready(), Ordering::Relaxed);
        std::thread::spawn(|| {
            // eframe shows the window after its first frame; hide it again right
            // then, and bring it from its off-screen start position to the centre.
            for _ in 0..10_000 {
                if let Some(h) = main_hwnd() {
                    unsafe {
                        if IsWindowVisible(h).as_bool() {
                            if tray_ready() {
                                let _ = ShowWindow(h, SW_HIDE);
                            }
                            center_on_work_area(h);
                            return;
                        }
                    }
                }
                std::thread::sleep(Duration::from_millis(1));
            }
        });
    }
}

fn center_on_work_area(h: HWND) {
    use windows::Win32::Foundation::RECT;
    unsafe {
        let mut work = RECT::default();
        let mut win = RECT::default();
        if SystemParametersInfoW(SPI_GETWORKAREA, 0, Some((&mut work as *mut RECT).cast()), SYSTEM_PARAMETERS_INFO_UPDATE_FLAGS(0)).is_err()
            || GetWindowRect(h, &mut win).is_err()
        {
            return;
        }
        let x = work.left + ((work.right - work.left) - (win.right - win.left)) / 2;
        let y = work.top + ((work.bottom - work.top) - (win.bottom - win.top)) / 2;
        let _ = SetWindowPos(h, None, x, y, 0, 0, SWP_NOSIZE | SWP_NOZORDER | SWP_NOACTIVATE);
    }
}

/// Take the single-instance mutex. A second instance wakes the first one's
/// window and returns false.
pub fn single_instance() -> bool {
    use windows::Win32::System::Threading::CreateMutexW;
    unsafe {
        let Ok(_mutex) = CreateMutexW(None, true, if is_dev() { w!("Local\\GHUBX-single-instance-dev") } else { w!("Local\\GHUBX-single-instance") }) else { return true };
        if GetLastError() != ERROR_ALREADY_EXISTS {
            return true;
        }
        for _ in 0..30 {
            if let Ok(h) = FindWindowW(tray_class(), PCWSTR::null()) {
                let _ = AllowSetForegroundWindow(ASFW_ANY);
                let _ = PostMessageW(Some(h), WM_TRAY_SHOW, WPARAM(0), LPARAM(0));
                break;
            }
            std::thread::sleep(Duration::from_millis(100));
        }
        false
    }
}

// ------------------------------------------------------------ Windows startup entries

const RUN_APPROVED: &str = r"Software\Microsoft\Windows\CurrentVersion\Explorer\StartupApproved\Run";

/// `StartupApproved` value: enabled = `02 00 00 00` + zeros; disabled = `03 00 00 00` + FILETIME.
pub fn startup_blob(on: bool, filetime: u64) -> [u8; 12] {
    let mut b = [0u8; 12];
    if on {
        b[0] = 2;
    } else {
        b[0] = 3;
        b[4..].copy_from_slice(&filetime.to_le_bytes());
    }
    b
}

/// Explorer treats an even first byte (or no data) as enabled.
pub fn startup_enabled(bytes: &[u8]) -> bool {
    bytes.first().is_none_or(|b| b % 2 == 0)
}

fn reg_binary(subkey: &str, name: &str) -> Option<Option<Vec<u8>>> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;
    let (sub, val) = (HSTRING::from(subkey), HSTRING::from(name));
    let mut len = 0u32;
    unsafe {
        if RegGetValueW(HKEY_CURRENT_USER, &sub, &val, RRF_RT_ANY, None, None, Some(&mut len)) != ERROR_SUCCESS {
            return None;
        }
        let mut buf = vec![0u8; len as usize];
        let mut len2 = len;
        let ok = RegGetValueW(HKEY_CURRENT_USER, &sub, &val, RRF_RT_REG_BINARY, None, Some(buf.as_mut_ptr().cast()), Some(&mut len2)) == ERROR_SUCCESS;
        Some(ok.then(|| {
            buf.truncate(len2 as usize);
            buf
        }))
    }
}

/// `None` when the Run entry `name` does not exist; else whether Explorer lets it start.
pub fn startup_approved(name: &str) -> Option<bool> {
    reg_binary(RUN_KEY, name)?;
    Some(reg_binary(RUN_APPROVED, name).flatten().is_none_or(|b| startup_enabled(&b)))
}

/// Enable or disable a Run entry the way Task Manager's Startup tab does.
pub fn set_startup_approved(name: &str, on: bool) -> bool {
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;
    let secs = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let blob = startup_blob(on, (secs + 11_644_473_600) * 10_000_000);
    unsafe {
        RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_APPROVED), &HSTRING::from(name), REG_BINARY.0, Some(blob.as_ptr().cast()), blob.len() as u32).is_ok()
    }
}

// ------------------------------------------------------------ scroll acceleration

static ACCEL_ON: AtomicBool = AtomicBool::new(false);
/// Boost * 100 (extra notches per notch at full speed).
static ACCEL_BOOST: AtomicU32 = AtomicU32::new(300);
static WHEEL_TX: OnceLock<crossbeam_channel::Sender<(i32, u32)>> = OnceLock::new();

static PASSIVE: AtomicBool = AtomicBool::new(false);

/// Passive (observer) mode: no wheel hook and no scroll acceleration, so a
/// dev/shot instance can never touch the user's real scrolling.
pub fn set_passive(on: bool) {
    PASSIVE.store(on, Ordering::Relaxed);
    if on {
        ACCEL_ON.store(false, Ordering::Relaxed);
    }
}

pub fn scroll_accel_enabled() -> bool {
    ACCEL_ON.load(Ordering::Relaxed)
}

pub fn set_scroll_accel(on: bool, boost: f32) {
    ACCEL_ON.store(on && !PASSIVE.load(Ordering::Relaxed), Ordering::Relaxed);
    ACCEL_BOOST.store((boost.clamp(0.0, 10.0) * 100.0) as u32, Ordering::Relaxed);
}

unsafe extern "system" fn wheel_hook(code: i32, wp: WPARAM, lp: LPARAM) -> LRESULT {
    if code >= 0 && wp.0 as u32 == WM_MOUSEWHEEL && ACCEL_ON.load(Ordering::Relaxed) {
        let info = unsafe { &*(lp.0 as *const MSLLHOOKSTRUCT) };
        if info.flags & LLMHF_INJECTED == 0 && info.dwExtraInfo != INJECT_TAG {
            let delta = (info.mouseData >> 16) as u16 as i16 as i32;
            if let Some(tx) = WHEEL_TX.get() {
                let _ = tx.try_send((delta, info.time));
            }
        }
    }
    unsafe { CallNextHookEx(None, code, wp, lp) }
}

/// Install the low-level wheel hook (once). Fast flicks get extra notches:
/// boost scales with how far the notch rate exceeds ~12 notches/s.
/// Returns false when skipped because of passive mode.
pub fn start_scroll_hook() -> bool {
    if PASSIVE.load(Ordering::Relaxed) {
        return false;
    }
    if WHEEL_TX.get().is_some() {
        return true;
    }
    let (tx, rx) = crossbeam_channel::bounded::<(i32, u32)>(256);
    if WHEEL_TX.set(tx).is_err() {
        return true;
    }
    std::thread::Builder::new()
        .name("wheel-accel".into())
        .spawn(move || {
            let mut last_t = 0u32;
            let mut speed = 0f32; // notches per second, smoothed
            let mut carry = 0f32;
            for (delta, t) in rx {
                let dt = t.wrapping_sub(last_t).max(1) as f32;
                last_t = t;
                let inst = 1000.0 / dt;
                speed = if dt > 250.0 { 0.0 } else { speed * 0.6 + inst * 0.4 };
                let boost = ACCEL_BOOST.load(Ordering::Relaxed) as f32 / 100.0;
                let factor = ((speed - 12.0) / 28.0).clamp(0.0, 1.0) * boost;
                carry += factor * delta.signum() as f32;
                let extra = carry.trunc() as i32;
                carry -= extra as f32;
                if extra != 0 {
                    wheel(extra * 120);
                }
            }
        })
        .ok();
    std::thread::Builder::new()
        .name("wheel-hook".into())
        .spawn(|| unsafe {
            let Ok(hook) = SetWindowsHookExW(WH_MOUSE_LL, Some(wheel_hook), None, 0) else { return };
            let mut msg = MSG::default();
            while GetMessageW(&mut msg, None, 0, 0).as_bool() {}
            let _ = UnhookWindowsHookEx(hook);
        })
        .ok();
    true
}

// ---------------------------------------------------------------- Windows Dynamic Lighting

const WDL_ROOT: &str = r"Software\Microsoft\Lighting\Devices";
static WDL_FAKE: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

fn wdl_device_key() -> Option<String> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::*;
    use windows::core::{HSTRING, PWSTR};
    unsafe {
        let mut root = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, &HSTRING::from(WDL_ROOT), None, KEY_READ, &mut root) != ERROR_SUCCESS {
            return None;
        }
        let mut found = None;
        for i in 0.. {
            let mut buf = [0u16; 512];
            let mut len = buf.len() as u32;
            if RegEnumKeyExW(root, i, Some(PWSTR(buf.as_mut_ptr())), &mut len, None, None, None, None) != ERROR_SUCCESS {
                break;
            }
            let name = String::from_utf16_lossy(&buf[..len as usize]);
            if name.contains("HID_DEVICE_SYSTEM_VHF") {
                found = Some(format!(r"{WDL_ROOT}\{name}"));
                break;
            }
        }
        let _ = RegCloseKey(root);
        found
    }
}

/// Windows Dynamic Lighting ("AmbientLightingEnabled") of the mouse's device key; `None` without one.
/// In passive mode (`--dev`, `--shot`) an in-memory value that starts `Some(false)`; the registry is never touched.
pub fn wdl_state() -> Option<bool> {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;
    if PASSIVE.load(Ordering::Relaxed) {
        return Some(WDL_FAKE.load(Ordering::Relaxed));
    }
    let key = wdl_device_key()?;
    let mut v = 0u32;
    let mut len = 4u32;
    unsafe {
        let r = RegGetValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from("AmbientLightingEnabled"), RRF_RT_REG_DWORD, None, Some(&mut v as *mut u32 as *mut _), Some(&mut len));
        (r == ERROR_SUCCESS).then_some(v != 0)
    }
}

/// Turn Windows Dynamic Lighting on or off for the mouse; false when it could not be written.
pub fn set_wdl(on: bool) -> bool {
    use windows::Win32::Foundation::ERROR_SUCCESS;
    use windows::Win32::System::Registry::*;
    use windows::core::HSTRING;
    if PASSIVE.load(Ordering::Relaxed) {
        WDL_FAKE.store(on, Ordering::Relaxed);
        return true;
    }
    let Some(key) = wdl_device_key() else { return false };
    let v = u32::from(on);
    unsafe { RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(key), &HSTRING::from("AmbientLightingEnabled"), REG_DWORD.0, Some(&v as *const u32 as *const _), 4) == ERROR_SUCCESS }
}

pub fn open_wdl_settings() {
    if PASSIVE.load(Ordering::Relaxed) {
        return;
    }
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;
    use windows::core::{HSTRING, w};
    unsafe {
        ShellExecuteW(None, w!("open"), &HSTRING::from("ms-settings:personalization-lighting"), None, None, SW_SHOWNORMAL);
    }
}

// ---------------------------------------------------------------- HTTP (WinINet)

/// GET `url` through WinINet (HTTP and HTTPS; redirects are followed). The
/// session's User-Agent is `ghubx/<version>` (GitHub refuses requests without
/// one) and `accept` becomes the `Accept` header. The body is capped at 64 MB;
/// connect timeout 5 s, send/receive 10 s.
pub fn http_get(url: &str, accept: &str) -> Result<Vec<u8>, String> {
    use windows::Win32::Networking::WinInet::*;
    use windows::core::HSTRING;
    const MAX_BODY: usize = 64 * 1024 * 1024;
    unsafe {
        let agent = HSTRING::from(format!("ghubx/{}", env!("CARGO_PKG_VERSION")));
        let h = InternetOpenW(&agent, INTERNET_OPEN_TYPE_PRECONFIG.0, PCWSTR::null(), PCWSTR::null(), 0);
        if h.is_null() {
            return Err(format!("InternetOpenW failed: {}", std::io::Error::last_os_error()));
        }
        for (opt, ms) in [(INTERNET_OPTION_CONNECT_TIMEOUT, 5_000u32), (INTERNET_OPTION_SEND_TIMEOUT, 10_000), (INTERNET_OPTION_RECEIVE_TIMEOUT, 10_000)] {
            let _ = InternetSetOptionW(Some(h.cast_const()), opt, Some((&ms as *const u32).cast()), 4);
        }
        let headers: Vec<u16> = format!("Accept: {accept}\r\n").encode_utf16().collect();
        let url_w = HSTRING::from(url);
        let req = InternetOpenUrlW(h, &url_w, Some(headers.as_slice()), INTERNET_FLAG_RELOAD | INTERNET_FLAG_NO_CACHE_WRITE | INTERNET_FLAG_PRAGMA_NOCACHE, None);
        let out = if req.is_null() {
            Err(format!("can't open {url}: {}", std::io::Error::last_os_error()))
        } else {
            let r = (|| {
                let mut code = 0u32;
                let mut len = std::mem::size_of_val(&code) as u32;
                HttpQueryInfoW(req, HTTP_QUERY_STATUS_CODE | HTTP_QUERY_FLAG_NUMBER, Some((&mut code as *mut u32).cast()), &mut len, None)
                    .map_err(|e| format!("HTTP status query failed: {e}"))?;
                if code != 200 {
                    return Err(format!("{url}: HTTP {code}"));
                }
                let mut body = Vec::new();
                loop {
                    let mut buf = [0u8; 16 * 1024];
                    let mut n = 0u32;
                    InternetReadFile(req, buf.as_mut_ptr().cast(), buf.len() as u32, &mut n).map_err(|e| format!("read failed: {e}"))?;
                    if n == 0 {
                        break;
                    }
                    body.extend_from_slice(&buf[..n as usize]);
                    if body.len() > MAX_BODY {
                        return Err("download is too large".to_string());
                    }
                }
                Ok(body)
            })();
            let _ = InternetCloseHandle(req);
            r
        };
        let _ = InternetCloseHandle(h);
        out
    }
}

/// Ask the tray window to run the normal quit (config save, engine Quit, icon
/// removal). Worker threads use this when the main loop may not pump frames —
/// e.g. the updater applying a new exe while the window sits hidden in the tray.
pub fn post_tray_quit() {
    let h = TRAY_HWND.load(Ordering::Relaxed);
    if h != 0 {
        unsafe {
            let _ = PostMessageW(Some(HWND(h as *mut _)), WM_TRAY_QUIT, WPARAM(0), LPARAM(0));
        }
    }
}

/// `--from-update`: take the single-instance mutex once the previous version's
/// process has let it go. Unlike `single_instance` this never wakes an older
/// instance (it is already exiting); it just polls for up to 20 s.
pub fn take_instance_after_update() -> bool {
    use windows::Win32::System::Threading::CreateMutexW;
    let deadline = std::time::Instant::now() + Duration::from_secs(20);
    unsafe {
        loop {
            let Ok(m) = CreateMutexW(None, true, if is_dev() { w!("Local\\GHUBX-single-instance-dev") } else { w!("Local\\GHUBX-single-instance") }) else {
                return true;
            };
            if GetLastError() != ERROR_ALREADY_EXISTS {
                // Held for the process lifetime, like `single_instance`.
                return true;
            }
            // Do not keep the mutex alive: the old process must be able to drop it.
            let _ = CloseHandle(m);
            if std::time::Instant::now() >= deadline {
                return false;
            }
            std::thread::sleep(Duration::from_millis(200));
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn char_key_maps_ascii() {
        if char_key('a') != Some((0x41, false)) {
            return;
        }
        assert_eq!(char_key('A'), Some((0x41, true)));
        assert_eq!(char_key('1'), Some((0x31, false)));
    }

    #[test]
    fn startup_blob_and_flag() {
        let on = startup_blob(true, 0x0102030405060708);
        assert_eq!(on, [2, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(startup_enabled(&on));
        let off = startup_blob(false, 0x0102030405060708);
        assert_eq!(off, [3, 0, 0, 0, 8, 7, 6, 5, 4, 3, 2, 1]);
        assert!(!startup_enabled(&off));
        assert!(startup_enabled(&[]));
    }

    #[test]
    fn startup_approved_round_trip() {
        use windows::Win32::System::Registry::*;
        use windows::core::HSTRING;
        let name = "GHUBX-selftest";
        let x: Vec<u16> = "x".encode_utf16().chain([0]).collect();
        unsafe {
            let _ = RegSetKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &HSTRING::from(name), REG_SZ.0, Some(x.as_ptr().cast()), (x.len() * 2) as u32);
        }
        assert_eq!(startup_approved(name), Some(true));
        assert!(set_startup_approved(name, false));
        assert_eq!(startup_approved(name), Some(false));
        assert!(set_startup_approved(name, true));
        assert_eq!(startup_approved(name), Some(true));
        unsafe {
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_KEY), &HSTRING::from(name));
            let _ = RegDeleteKeyValueW(HKEY_CURRENT_USER, &HSTRING::from(RUN_APPROVED), &HSTRING::from(name));
        }
        assert_eq!(startup_approved("GHUBX-missing-entry"), None);
    }
}
