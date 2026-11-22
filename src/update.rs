//! Update checking and self-apply against GitHub Releases
//! (`https://github.com/Repex13/ghubx`, built by `release.yml` on `v*` tags).
//!
//! Same shape as `obs::Handle` / `lua::Handle`: one worker thread behind a
//! channel, shared state the UI polls. The worker checks once ~10 s after
//! start, then every 12 h, and on `UpdateMsg::Check` (Settings CHECK NOW).
//! Downloads and the exe swap only ever run on explicit user clicks — the
//! periodic poll only compares versions. `Handle::fixed()` is the no-op for
//! observer mode (`--dev`, `--shot`, scripted runs) and any run whose
//! `GHUBX_CONFIG_DIR` points at a test config.
//!
//! Apply path: the running exe is renamed to `<name>.old.exe` (Windows allows
//! renaming a running image), the verified download takes its place, the new
//! exe is spawned with `--from-update` (it waits for this process to release
//! the single-instance mutex; `--minimized` is forwarded when the window is
//! hidden), and the normal tray quit runs so the engine releases the mouse.

use crate::notify::{Feed, NoteKind};
use sha2::{Digest, Sha256};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Latest-release endpoint (60 req/h unauthenticated — the poll uses two).
const API_URL: &str = "https://api.github.com/repos/Repex13/ghubx/releases/latest";
/// First automatic check after startup.
const FIRST_CHECK: Duration = Duration::from_secs(10);
/// Automatic re-check interval.
const EVERY: Duration = Duration::from_secs(12 * 60 * 60);

/// A release asset pair (`ghubx.exe` + `ghubx.exe.sha256`).
#[derive(Clone, Debug, PartialEq)]
pub struct Release {
    pub tag: String,
    pub version: (u32, u32, u32),
    pub exe_url: String,
    pub sha_url: String,
}

/// Commands for the update worker.
#[derive(Debug)]
pub enum UpdateMsg {
    /// Run the release check now (Settings CHECK NOW).
    Check,
    /// Download and verify this release (Settings UPDATE AND RESTART).
    Download(Release),
    /// Swap the exe and restart (Settings RESTART NOW; needs `Ready`).
    Apply,
    /// Stop the worker.
    Quit,
}

/// What the Settings APP section shows; the feed notes are pushed separately.
#[derive(Clone, Debug, Default, PartialEq)]
pub enum UpdateState {
    #[default]
    Idle,
    Checking,
    UpToDate,
    Available(Release),
    Downloading,
    Ready {
        path: PathBuf,
        tag: String,
    },
    Failed(String),
    /// The new exe was swapped in and spawned; the app is about to quit.
    Applied,
}

#[derive(Default)]
struct Shared {
    state: UpdateState,
    last_check: Option<Instant>,
    /// Version already announced in the feed, so "Update available" is pushed
    /// once per version, not on every 12 h poll.
    last_announced: Option<(u32, u32, u32)>,
}

fn lock(s: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Owns the worker channel and the shared state; cloning shares both.
#[derive(Clone)]
pub struct Handle {
    tx: Option<Sender<UpdateMsg>>,
    shared: Arc<Mutex<Shared>>,
}

impl Handle {
    /// Worker thread, unless this run may not touch the network for real:
    /// observer mode (`--dev`, `--shot`, scripted) or a `GHUBX_CONFIG_DIR`
    /// test config both get a fixed no-op handle.
    pub fn spawn(feed: Feed) -> Self {
        if crate::engine::observer_mode() || std::env::var_os("GHUBX_CONFIG_DIR").is_some() {
            return Self::fixed();
        }
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = channel::<UpdateMsg>();
        let s = shared.clone();
        std::thread::Builder::new()
            .name("update".into())
            .spawn(move || run(rx, s, feed))
            .ok();
        Self { tx: Some(tx), shared }
    }

    /// No thread, no network: observer mode and tests.
    pub fn fixed() -> Self {
        Self { tx: None, shared: Arc::new(Mutex::new(Shared::default())) }
    }

    /// Queue a command; a fixed handle drops it.
    pub fn send(&self, m: UpdateMsg) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(m);
        }
    }

    pub fn check_now(&self) {
        self.send(UpdateMsg::Check);
    }

    pub fn download(&self, r: Release) {
        self.send(UpdateMsg::Download(r));
    }

    pub fn apply(&self) {
        self.send(UpdateMsg::Apply);
    }

    pub fn quit(&self) {
        self.send(UpdateMsg::Quit);
    }

    pub fn state(&self) -> UpdateState {
        lock(&self.shared).state.clone()
    }
}

fn run(rx: Receiver<UpdateMsg>, shared: Arc<Mutex<Shared>>, feed: Feed) {
    let mut wake = Instant::now() + FIRST_CHECK;
    loop {
        let wait = wake.saturating_duration_since(Instant::now());
        match rx.recv_timeout(wait) {
            Ok(UpdateMsg::Quit) | Err(std::sync::mpsc::RecvTimeoutError::Disconnected) => break,
            Ok(UpdateMsg::Check) | Err(std::sync::mpsc::RecvTimeoutError::Timeout) => {
                check(&shared, &feed);
                wake = Instant::now() + EVERY;
            }
            Ok(UpdateMsg::Download(r)) => download(&shared, &feed, &r),
            Ok(UpdateMsg::Apply) => apply(&shared, &feed),
        }
    }
}

// ------------------------------------------------------------------- states

fn check(shared: &Arc<Mutex<Shared>>, feed: &Feed) {
    lock(shared).state = UpdateState::Checking;
    let mut g = lock(shared);
    g.last_check = Some(Instant::now());
    match latest_release() {
        Ok(Some(r)) if ver_gt(r.version, current_ver()) => {
            if g.last_announced != Some(r.version) {
                g.last_announced = Some(r.version);
                feed.push(format!("Update available: {}", r.tag), NoteKind::Update);
            }
            g.state = UpdateState::Available(r);
        }
        Ok(_) => g.state = UpdateState::UpToDate,
        Err(e) => {
            g.state = UpdateState::Failed(e.clone());
            feed.push(format!("Update check failed: {e}"), NoteKind::Warn);
        }
    }
}

fn download(shared: &Arc<Mutex<Shared>>, feed: &Feed, r: &Release) {
    lock(shared).state = UpdateState::Downloading;
    match fetch_release(r) {
        Ok(path) => {
            lock(shared).state = UpdateState::Ready { path, tag: r.tag.clone() };
            feed.push("Update downloaded — restart to apply", NoteKind::Update);
        }
        Err(e) => {
            lock(shared).state = UpdateState::Failed(e.clone());
            feed.push(format!("Update failed: {e}"), NoteKind::Warn);
        }
    }
}

fn apply(shared: &Arc<Mutex<Shared>>, feed: &Feed) {
    let ready = match &lock(shared).state {
        UpdateState::Ready { path, tag } => Some((path.clone(), tag.clone())),
        _ => None,
    };
    let Some((path, _tag)) = ready else { return };
    match apply_update(&path) {
        Ok(_old) => {
            lock(shared).state = UpdateState::Applied;
            // The normal quit path (tray Quit -> ui::Exit::run) releases the
            // mouse even when the window is hidden and no frames are pumped.
            crate::win::post_tray_quit();
        }
        Err(e) => {
            lock(shared).state = UpdateState::Failed(e.clone());
            feed.push(format!("Update failed: {e}"), NoteKind::Warn);
        }
    }
}

// ------------------------------------------------------------------- real work

/// `<base>/updates`: the parent of the saved-copy dir (`%LOCALAPPDATA%\ghubx`
/// normally; `GHUBX_CACHE_DIR` moves it).
pub fn updates_dir() -> PathBuf {
    let cache = crate::ui::assets::cache_dir();
    cache.parent().map_or_else(|| cache.join("updates"), |p| p.join("updates"))
}

/// GET the latest release and parse it. `Ok(None)` means GitHub answered 404 —
/// the repo simply has no releases yet, which is "up to date", not a failure.
/// Network + JSON only; probe and the worker both use this.
pub fn latest_release() -> Result<Option<Release>, String> {
    let body = match crate::win::http_get(API_URL, "application/vnd.github+json") {
        Ok(b) => b,
        Err(e) if e.ends_with("HTTP 404") => return Ok(None),
        Err(e) => return Err(e),
    };
    let text = String::from_utf8(body).map_err(|_| "release response is not UTF-8".to_string())?;
    parse_release(&text).map(Some).ok_or_else(|| "no usable release in the response".to_string())
}

/// Download `ghubx.exe` + its `.sha256`, write the exe to
/// `<updates>/ghubx.new.exe` and verify it. On a checksum mismatch the file is
/// deleted again.
pub fn fetch_release(r: &Release) -> Result<PathBuf, String> {
    let exe = crate::win::http_get(&r.exe_url, "application/octet-stream")?;
    let dir = updates_dir();
    std::fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let path = dir.join("ghubx.new.exe");
    std::fs::write(&path, &exe).map_err(|e| e.to_string())?;
    let result = (|| {
        let sha = crate::win::http_get(&r.sha_url, "text/plain")?;
        let text = String::from_utf8(sha).map_err(|_| "checksum file is not text".to_string())?;
        let want = first_sha_token(&text).ok_or_else(|| "no sha256 in the checksum file".to_string())?;
        match verify_sha256(&path, &want) {
            Ok(true) => Ok(path.clone()),
            Ok(false) => Err("sha256 mismatch — the download was deleted".to_string()),
            Err(e) => Err(format!("couldn't verify the download: {e}")),
        }
    })();
    if result.is_err() {
        let _ = std::fs::remove_file(&path);
    }
    result
}

/// Swap the exe: `current` -> `<name>.old.exe`, `new_file` -> `current`, then
/// spawn the new exe (`--from-update` makes it wait for this process to exit;
/// `--minimized` keeps it in the tray when the window was hidden). On a spawn
/// failure everything is moved back. Returns the `.old.exe` path.
fn apply_update(new_file: &Path) -> Result<PathBuf, String> {
    let exe = std::env::current_exe().map_err(|e| e.to_string())?;
    let old = swap_exe(&exe, new_file).map_err(|e| format!("couldn't replace the exe: {e}"))?;
    let mut cmd = std::process::Command::new(&exe);
    cmd.arg("--from-update");
    if crate::win::main_hidden() {
        cmd.arg("--minimized");
    }
    if let Err(e) = cmd.spawn() {
        let _ = std::fs::rename(&exe, new_file);
        let _ = std::fs::rename(&old, &exe);
        return Err(format!("couldn't start the new version: {e}"));
    }
    Ok(old)
}

// ------------------------------------------------------------------- pure parts

/// "v0.2.0" -> (0, 2, 0): the leading `v` is stripped, a `-suffix` and any
/// extra components are ignored, missing ones count as 0.
pub fn parse_ver(tag: &str) -> Option<(u32, u32, u32)> {
    let s = tag.trim().trim_start_matches(['v', 'V']);
    let s = s.split(['-', '+']).next().unwrap_or(s);
    let mut it = s.split('.');
    let a = it.next()?.parse::<u32>().ok()?;
    let b = it.next().map_or(Ok(0), str::parse).ok()?;
    let c = it.next().map_or(Ok(0), str::parse).ok()?;
    Some((a, b, c))
}

/// `remote` is strictly newer than `local`.
pub fn ver_gt(remote: (u32, u32, u32), local: (u32, u32, u32)) -> bool {
    remote > local
}

/// This build's version.
pub fn current_ver() -> (u32, u32, u32) {
    parse_ver(env!("CARGO_PKG_VERSION")).unwrap_or((0, 0, 0))
}

/// Parse the GitHub `releases/latest` JSON into a [`Release`] (needs both
/// `ghubx.exe` and `ghubx.exe.sha256` assets). Pure; unit-tested.
pub fn parse_release(json: &str) -> Option<Release> {
    let v: serde_json::Value = serde_json::from_str(json).ok()?;
    let tag = v.get("tag_name")?.as_str()?.to_string();
    let version = parse_ver(&tag)?;
    let mut exe = None;
    let mut sha = None;
    for a in v.get("assets")?.as_array()? {
        let name = a.get("name").and_then(|n| n.as_str());
        let url = a.get("browser_download_url").and_then(|u| u.as_str());
        match (name, url) {
            (Some("ghubx.exe"), Some(u)) => exe = Some(u.to_string()),
            (Some("ghubx.exe.sha256"), Some(u)) => sha = Some(u.to_string()),
            _ => {}
        }
    }
    Some(Release { tag, version, exe_url: exe?, sha_url: sha? })
}

/// First 64-hex token in a `<sha256>  <name>` checksum file.
pub fn first_sha_token(text: &str) -> Option<String> {
    text.split_whitespace()
        .find(|t| t.len() == 64 && t.chars().all(|c| c.is_ascii_hexdigit()))
        .map(str::to_string)
}

/// `path`'s SHA-256 equals the 64-hex `expected` (case-insensitive).
pub fn verify_sha256(path: &Path, expected: &str) -> std::io::Result<bool> {
    let bytes = std::fs::read(path)?;
    let digest = Sha256::digest(&bytes);
    let Some(want) = hex_32(expected) else {
        return Ok(false);
    };
    Ok(digest[..] == want[..])
}

fn hex_32(s: &str) -> Option<[u8; 32]> {
    let s = s.trim();
    if s.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (i, b) in out.iter_mut().enumerate() {
        *b = u8::from_str_radix(&s[i * 2..i * 2 + 2], 16).ok()?;
    }
    Some(out)
}

/// The `<name>.old.exe` path next to `exe` (`ghubx.exe` -> `ghubx.old.exe`).
pub fn old_exe_path(exe: &Path) -> PathBuf {
    exe.with_extension("old.exe")
}

/// Rename `current` to `<name>.old.exe`, then move `new_file` onto `current`.
/// If the move fails, `current` is restored. Explicit paths so a unit test can
/// run it against temp files. Returns the backup path.
pub fn swap_exe(current: &Path, new_file: &Path) -> std::io::Result<PathBuf> {
    let old = old_exe_path(current);
    if old.exists() {
        std::fs::remove_file(&old)?;
    }
    std::fs::rename(current, &old)?;
    match std::fs::rename(new_file, current) {
        Ok(()) => Ok(old),
        Err(e) => {
            let _ = std::fs::rename(&old, current);
            Err(e)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_ver_handles_tags() {
        assert_eq!(parse_ver("v0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_ver("0.2.0"), Some((0, 2, 0)));
        assert_eq!(parse_ver("v1.2.3-rc1"), Some((1, 2, 3)));
        assert_eq!(parse_ver("v2"), Some((2, 0, 0)));
        assert_eq!(parse_ver("v1.2.3.4"), Some((1, 2, 3)));
        assert_eq!(parse_ver("garbage"), None);
    }

    #[test]
    fn ver_gt_compares_tuples() {
        assert!(ver_gt((0, 2, 0), (0, 1, 0)));
        assert!(ver_gt((1, 0, 0), (0, 9, 9)));
        assert!(!ver_gt((0, 1, 0), (0, 1, 0)));
        assert!(!ver_gt((0, 1, 0), (0, 2, 0)));
    }

    #[test]
    fn parse_release_reads_assets() {
        let json = r#"{
            "tag_name": "v0.2.0",
            "assets": [
                {"name": "ghubx.exe", "browser_download_url": "https://example/ghubx.exe"},
                {"name": "ghubx.exe.sha256", "browser_download_url": "https://example/ghubx.exe.sha256"},
                {"name": "notes.txt", "browser_download_url": "https://example/notes.txt"}
            ]
        }"#;
        let r = parse_release(json).unwrap();
        assert_eq!(r.tag, "v0.2.0");
        assert_eq!(r.version, (0, 2, 0));
        assert_eq!(r.exe_url, "https://example/ghubx.exe");
        assert_eq!(r.sha_url, "https://example/ghubx.exe.sha256");
        // missing the checksum asset -> not usable
        let no_sha = r#"{"tag_name": "v0.2.0", "assets": [{"name": "ghubx.exe", "browser_download_url": "u"}]}"#;
        assert_eq!(parse_release(no_sha), None);
        assert_eq!(parse_release("not json"), None);
        assert_eq!(parse_release(r#"{"tag_name": "v0.2.0"}"#), None);
    }

    #[test]
    fn first_sha_token_finds_the_digest() {
        let h = "a".repeat(64);
        assert_eq!(first_sha_token(&format!("{h}  ghubx.exe\n")), Some(h.clone()));
        assert_eq!(first_sha_token(&format!("leading text\n{h}  ghubx.exe")), Some(h));
        assert_eq!(first_sha_token("too short 1234"), None);
        assert_eq!(first_sha_token(&"z".repeat(64)), None); // not hex
        assert_eq!(first_sha_token(""), None);
    }

    fn tmp_dir(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("ghubx-upd-test-{}-{}", name, std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn verify_sha256_checks_the_file() {
        let d = tmp_dir("sha");
        let f = d.join("a.exe");
        std::fs::write(&f, b"hello gh ub").unwrap();
        let want: String = Sha256::digest(b"hello gh ub").iter().map(|b| format!("{b:02x}")).collect();
        assert!(verify_sha256(&f, &want).unwrap());
        assert!(verify_sha256(&f, &want.to_uppercase()).unwrap());
        assert!(!verify_sha256(&f, &"0".repeat(64)).unwrap());
        assert!(!verify_sha256(&f, "not-hex").unwrap());
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn swap_exe_moves_and_rolls_back() {
        let d = tmp_dir("swap");
        let cur = d.join("ghubx.exe");
        let new = d.join("ghubx.new.exe");
        std::fs::write(&cur, b"old").unwrap();
        std::fs::write(&new, b"new").unwrap();
        // happy path: old content is parked at .old.exe, new takes the name
        let old = swap_exe(&cur, &new).unwrap();
        assert_eq!(old, d.join("ghubx.old.exe"));
        assert_eq!(std::fs::read(&cur).unwrap(), b"new");
        assert_eq!(std::fs::read(&old).unwrap(), b"old");
        // second swap clears a stale .old.exe first
        std::fs::write(&new, b"newer").unwrap();
        swap_exe(&cur, &new).unwrap();
        assert_eq!(std::fs::read(&cur).unwrap(), b"newer");
        // failure path: new file missing -> current gets the old bytes back
        let _ = std::fs::remove_file(&new);
        assert!(swap_exe(&cur, &new).is_err());
        assert_eq!(std::fs::read(&cur).unwrap(), b"newer");
        assert!(!old.exists()); // the rollback consumed the backup
        let _ = std::fs::remove_dir_all(&d);
    }

    #[test]
    fn updates_dir_sits_next_to_the_saved_copy() {
        let d = updates_dir();
        assert_eq!(d.file_name().unwrap(), "updates");
    }

    #[test]
    fn hex_32_decodes_exactly_64_hex() {
        assert!(hex_32(&"a".repeat(64)).is_some());
        assert!(hex_32(&"G".repeat(64)).is_none());
        assert!(hex_32("abcd").is_none());
    }
}
