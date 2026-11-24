//! G HUB's game database (read at runtime from the local G HUB install) and
//! a scan for installed games (Steam, Epic, Riot, GOG, Ubisoft, registry rules).

use crate::profile::{KeyCombo, MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_WIN, P180_CUSTOM, P180_FORTNITE, P180_GAMES, p180_counts_for, p180_preset_counts};
use serde_json::Value;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use windows::Win32::Foundation::ERROR_SUCCESS;
use windows::Win32::System::Registry::*;
use windows::core::HSTRING;

#[derive(Clone, Debug, PartialEq)]
pub enum Detect {
    Steam(String),
    Epic(String),
    Gog(String),
    Riot(String),
    Uplay(String),
    Registry { path: String, key: String, exe: Option<String> },
}

#[derive(Clone, Debug, PartialEq)]
pub struct GameCommand {
    pub category: String,
    pub name: String,
    pub keys: Vec<String>,
}

#[derive(Clone, Debug)]
pub struct Game {
    pub id: String,
    pub name: String,
    pub detect: Vec<Detect>,
    /// Categories in order of first appearance, with the tag colour if known.
    pub categories: Vec<(String, Option<[u8; 3]>)>,
    pub commands: Vec<GameCommand>,
}

#[derive(Clone, Debug, Default)]
pub struct GameDb {
    pub games: Vec<Game>,
}

fn text_of(v: &Value) -> Option<String> {
    match v {
        Value::String(s) => Some(s.clone()),
        Value::Number(n) => Some(n.to_string()),
        _ => None,
    }
}

fn field(v: &Value, k: &str) -> Option<String> {
    text_of(v.get(k)?)
}

fn parse_hex(s: &str) -> Option<[u8; 3]> {
    let n = u32::from_str_radix(s.trim_start_matches('#'), 16).ok()?;
    (s.trim_start_matches('#').len() == 6).then_some([(n >> 16) as u8, (n >> 8) as u8, n as u8])
}

fn parse_detect(d: &Value) -> Option<Detect> {
    if let Some(s) = d.get("steam") {
        return Some(Detect::Steam(field(s, "appId")?));
    }
    if let Some(s) = d.get("epicGames") {
        return Some(Detect::Epic(field(s, "appName")?));
    }
    if let Some(s) = d.get("gogGalaxy") {
        return Some(Detect::Gog(field(s, "productId")?));
    }
    if let Some(s) = d.get("riotGames") {
        return Some(Detect::Riot(field(s, "appName")?));
    }
    if let Some(s) = d.get("uplay") {
        return Some(Detect::Uplay(field(s, "appId")?));
    }
    let r = d.get("winRegistry")?;
    Some(Detect::Registry { path: field(r, "registryPath")?, key: field(r, "registryKey")?, exe: field(r, "executable") })
}

fn parse_game(a: &Value) -> Option<Game> {
    let colors: Vec<(String, [u8; 3])> = a
        .get("categoryColors")
        .and_then(Value::as_array)
        .map(|v| v.iter().filter_map(|c| Some((field(c, "tag")?, parse_hex(&field(c, "hex")?)?))).collect())
        .unwrap_or_default();
    let mut commands = Vec::new();
    let mut categories: Vec<(String, Option<[u8; 3]>)> = Vec::new();
    for c in a.get("commands").and_then(Value::as_array).into_iter().flatten() {
        let (Some(category), Some(name)) = (field(c, "category"), field(c, "name")) else { continue };
        let keys = c.get("keystroke").and_then(Value::as_array).map(|k| k.iter().filter_map(text_of).collect()).unwrap_or_default();
        if !categories.iter().any(|(n, _)| *n == category) {
            let color = colors.iter().find(|(t, _)| *t == category).map(|(_, c)| *c);
            categories.push((category.clone(), color));
        }
        commands.push(GameCommand { category, name, keys });
    }
    Some(Game {
        id: field(a, "applicationId")?,
        name: field(a, "name")?,
        detect: a.get("detection").and_then(Value::as_array).map(|d| d.iter().filter_map(parse_detect).collect()).unwrap_or_default(),
        categories,
        commands,
    })
}

impl GameDb {
    pub fn parse(json: &str) -> Option<Self> {
        let root: Value = serde_json::from_str(json).ok()?;
        let games = root.get("applications")?.as_array()?.iter().filter_map(parse_game).collect();
        Some(Self { games })
    }

    /// G HUB's `applications.json`, from the install or any depot.
    pub fn load() -> Option<Self> {
        crate::ui::assets::applications_candidates().iter().find_map(|(p, _)| Self::parse(&std::fs::read_to_string(p).ok()?))
    }

    pub fn by_id(&self, id: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.id == id)
    }

    pub fn by_name(&self, name: &str) -> Option<&Game> {
        self.games.iter().find(|g| g.name.eq_ignore_ascii_case(name))
    }

    fn find(&self, f: impl Fn(&Detect) -> bool) -> Option<&Game> {
        self.games.iter().find(|g| g.detect.iter().any(&f))
    }
}

static DB: OnceLock<Option<GameDb>> = OnceLock::new();

/// Load the database on a background thread.
pub fn preload() {
    std::thread::spawn(|| {
        DB.get_or_init(GameDb::load);
    });
}

/// The database, loading it now if the background load has not finished.
pub fn db_blocking() -> Option<&'static GameDb> {
    DB.get_or_init(GameDb::load).as_ref()
}

/// The loaded database; `None` until loaded or when G HUB's file is missing.
pub fn db() -> Option<&'static GameDb> {
    DB.get().and_then(|d| d.as_ref())
}

// ------------------------------------------------------------------ keys

fn modifier(token: &str) -> Option<(u16, u8)> {
    Some(match token {
        "LCTRL" => (0xA2, MOD_CTRL),
        "RCTRL" => (0xA3, MOD_CTRL),
        "LSHIFT" => (0xA0, MOD_SHIFT),
        "RSHIFT" => (0xA1, MOD_SHIFT),
        "LALT" => (0xA4, MOD_ALT),
        "RALT" => (0xA5, MOD_ALT),
        "LGUI" => (0x5B, MOD_WIN),
        _ => return None,
    })
}

fn token_vk(t: &str) -> Option<u16> {
    if t.len() == 1 {
        let c = t.as_bytes()[0];
        if c.is_ascii_uppercase() || c.is_ascii_digit() {
            return Some(c as u16);
        }
    }
    if let Some(n) = t.strip_prefix('F').and_then(|n| n.parse::<u16>().ok())
        && (1..=24).contains(&n)
    {
        return Some(0x6F + n);
    }
    if let Some(n) = t.strip_prefix("NUM").and_then(|n| n.parse::<u16>().ok())
        && n <= 9
    {
        return Some(0x60 + n);
    }
    Some(match t {
        "NUMENTER" | "ENTER" => 0x0D,
        "NUMLOCK" => 0x90,
        "NUMMINUS" => 0x6D,
        "NUMPERIOD" => 0x6E,
        "NUMPLUS" => 0x6B,
        "NUMSLASH" => 0x6F,
        "NUMSTAR" => 0x6A,
        "APPKEY" => 0x5D,
        "BACKSLASH" | "HASH" => 0xDC,
        "BACKSPACE" => 0x08,
        "CAPSLOCK" => 0x14,
        "COMMA" => 0xBC,
        "DELETE" => 0x2E,
        "DOWN" => 0x28,
        "END" => 0x23,
        "EQUAL" => 0xBB,
        "ESCAPE" => 0x1B,
        "HOME" => 0x24,
        "INSERT" => 0x2D,
        "LBRACKET" => 0xDB,
        "LEFT" => 0x25,
        "MAIL" => 0xB4,
        "MINUS" => 0xBD,
        "NON_US_SLASH" => 0xE2,
        "PAGEDOWN" => 0x22,
        "PAGEUP" => 0x21,
        "PAUSE" => 0x13,
        "PERIOD" => 0xBE,
        "PRINTSCREEN" => 0x2C,
        "QUOTE" => 0xDE,
        "RBRACKET" => 0xDD,
        "RIGHT" => 0x27,
        "SCROLLLOCK" => 0x91,
        "SEMICOLON" => 0xBA,
        "SLASH" => 0xBF,
        "SPACEBAR" => 0x20,
        "TAB" => 0x09,
        "TILDE" => 0xC0,
        "UP" => 0x26,
        _ => return None,
    })
}

/// A G HUB keystroke as a GHUB-X key combo. `None` for more than one plain key
/// or unknown tokens.
pub fn key_combo(tokens: &[String]) -> Option<KeyCombo> {
    if tokens.is_empty() {
        return None;
    }
    let mut mods = 0u8;
    let mut keys = Vec::new();
    for t in tokens {
        match modifier(t) {
            Some((_, bit)) => mods |= bit,
            None => keys.push(token_vk(t)?),
        }
    }
    match keys.as_slice() {
        [vk] => Some(KeyCombo { vk: *vk, mods }),
        [] => {
            let (last, rest) = tokens.split_last()?;
            let rest_mods = rest.iter().filter_map(|t| modifier(t)).fold(0, |m, (_, bit)| m | bit);
            Some(KeyCombo { vk: modifier(last)?.0, mods: rest_mods })
        }
        _ => None,
    }
}

// ------------------------------------------------------------------ scan

#[derive(Clone, Debug)]
pub struct Installed {
    pub name: String,
    pub dir: String,
    pub source: &'static str,
    pub ghub_id: Option<String>,
}

fn quoted(line: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut cur: Option<String> = None;
    let mut chars = line.chars();
    while let Some(c) = chars.next() {
        match (&mut cur, c) {
            (None, '"') => cur = Some(String::new()),
            (Some(_), '"') => out.extend(cur.take()),
            (Some(s), '\\') => match chars.next() {
                Some('\\') => s.push('\\'),
                Some('"') => s.push('"'),
                Some(o) => {
                    s.push('\\');
                    s.push(o);
                }
                None => {}
            },
            (Some(s), c) => s.push(c),
            _ => {}
        }
    }
    out
}

/// Every `"path"` value of Steam's `libraryfolders.vdf`.
pub fn parse_library_folders(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| {
            let q = quoted(l);
            (q.len() >= 2 && q[0].eq_ignore_ascii_case("path")).then(|| q[1].clone())
        })
        .collect()
}

/// `(appid, name, installdir)` of a Steam `appmanifest_*.acf`.
pub fn parse_acf(text: &str) -> Option<(String, String, String)> {
    let (mut id, mut name, mut dir) = (None, None, None);
    for l in text.lines() {
        let q = quoted(l);
        if q.len() < 2 {
            continue;
        }
        match q[0].to_ascii_lowercase().as_str() {
            "appid" => id = Some(q[1].clone()),
            "name" => name = Some(q[1].clone()),
            "installdir" => dir = Some(q[1].clone()),
            _ => {}
        }
    }
    Some((id?, name?, dir?))
}

/// `(display name, install location, ids)` of an Epic `.item` manifest.
pub fn parse_epic_item(json: &str) -> Option<(String, String, Vec<String>)> {
    let v: Value = serde_json::from_str(json).ok()?;
    let name = field(&v, "DisplayName")?;
    let loc = field(&v, "InstallLocation").filter(|l| !l.is_empty())?;
    let ids = ["AppName", "CatalogItemId", "MainGameAppName", "CatalogNamespace"].iter().filter_map(|k| field(&v, k)).filter(|s| !s.is_empty()).collect();
    Some((name, loc, ids))
}

/// `product_install_full_path` of a Riot `*.product_settings.yaml`.
pub fn parse_riot_install(text: &str) -> Option<String> {
    let line = text.lines().find(|l| l.trim_start().starts_with("product_install_full_path:"))?;
    let v = line.split_once(':')?.1.trim().trim_matches(|c| c == '"' || c == '\'');
    (!v.is_empty()).then(|| v.to_string())
}

fn reg_string(root: HKEY, subkey: &str, value: &str, view: REG_ROUTINE_FLAGS) -> Option<String> {
    let (sub, val) = (HSTRING::from(subkey), HSTRING::from(value));
    let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ | view;
    let mut len = 0u32;
    unsafe {
        if RegGetValueW(root, &sub, &val, flags, None, None, Some(&mut len)) != ERROR_SUCCESS || len == 0 {
            return None;
        }
        let mut buf = vec![0u16; len as usize / 2 + 1];
        let mut len2 = (buf.len() * 2) as u32;
        if RegGetValueW(root, &sub, &val, flags, None, Some(buf.as_mut_ptr().cast()), Some(&mut len2)) != ERROR_SUCCESS {
            return None;
        }
        let end = buf.iter().position(|&c| c == 0).unwrap_or(buf.len());
        let s = String::from_utf16_lossy(&buf[..end]);
        (!s.is_empty()).then_some(s)
    }
}

fn reg_any_view(root: HKEY, subkey: &str, value: &str) -> Option<String> {
    reg_string(root, subkey, value, RRF_SUBKEY_WOW6464KEY).or_else(|| reg_string(root, subkey, value, RRF_SUBKEY_WOW6432KEY))
}

fn norm_key(dir: &str) -> String {
    dir.replace('/', "\\").trim_end_matches('\\').to_lowercase()
}

const STEAM_SKIP: [&str; 5] = ["Redistributable", "Steamworks", "Steam Linux Runtime", "Proton", "SteamVR"];

fn scan_steam(db: Option<&GameDb>) -> Vec<Installed> {
    let mut libs: Vec<String> = Vec::new();
    let steam = reg_any_view(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath").map(|p| p.replace('/', "\\"));
    if let Some(s) = &steam
        && let Ok(t) = std::fs::read_to_string(Path::new(s).join(r"steamapps\libraryfolders.vdf"))
    {
        libs.extend(parse_library_folders(&t));
    }
    libs.extend(steam);
    let mut seen = Vec::new();
    let mut out = Vec::new();
    for lib in libs {
        if seen.contains(&norm_key(&lib)) {
            continue;
        }
        seen.push(norm_key(&lib));
        let apps = Path::new(&lib).join("steamapps");
        for e in std::fs::read_dir(&apps).into_iter().flatten().flatten() {
            let fname = e.file_name().to_string_lossy().to_string();
            if !(fname.starts_with("appmanifest_") && fname.ends_with(".acf")) {
                continue;
            }
            let Some((appid, name, dir)) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_acf(&t)) else { continue };
            if STEAM_SKIP.iter().any(|s| name.contains(s)) {
                continue;
            }
            let folder = apps.join("common").join(dir);
            if folder.is_dir() {
                let ghub_id = db.and_then(|d| d.find(|x| matches!(x, Detect::Steam(a) if *a == appid))).map(|g| g.id.clone());
                out.push(Installed { name, dir: folder.display().to_string(), source: "Steam", ghub_id });
            }
        }
    }
    out
}

fn program_data() -> PathBuf {
    PathBuf::from(std::env::var("ProgramData").unwrap_or_else(|_| r"C:\ProgramData".into()))
}

fn scan_epic(db: Option<&GameDb>) -> Vec<Installed> {
    let dir = program_data().join(r"Epic\EpicGamesLauncher\Data\Manifests");
    let mut out = Vec::new();
    for e in std::fs::read_dir(dir).into_iter().flatten().flatten() {
        if e.path().extension().is_none_or(|x| x != "item") {
            continue;
        }
        let Some((name, loc, ids)) = std::fs::read_to_string(e.path()).ok().and_then(|t| parse_epic_item(&t)) else { continue };
        if !Path::new(&loc).is_dir() {
            continue;
        }
        let ghub_id = db.and_then(|d| d.find(|x| matches!(x, Detect::Epic(a) if ids.contains(a)))).map(|g| g.id.clone());
        out.push(Installed { name, dir: loc, source: "Epic Games", ghub_id });
    }
    out
}

fn scan_riot(db: Option<&GameDb>) -> Vec<Installed> {
    let root = program_data().join(r"Riot Games\Metadata");
    let mut out = Vec::new();
    for e in std::fs::read_dir(root).into_iter().flatten().flatten() {
        let app = e.file_name().to_string_lossy().to_string();
        if app == "riot_client" {
            continue;
        }
        let Some(dir) = std::fs::read_to_string(e.path().join(format!("{app}.product_settings.yaml"))).ok().and_then(|t| parse_riot_install(&t)) else { continue };
        if !Path::new(&dir).is_dir() {
            continue;
        }
        let game = db.and_then(|d| d.find(|x| matches!(x, Detect::Riot(a) if *a == app)));
        out.push(Installed { name: game.map_or_else(|| app.clone(), |g| g.name.clone()), dir, source: "Riot Games", ghub_id: game.map(|g| g.id.clone()) });
    }
    out
}

fn scan_registry(db: &GameDb) -> Vec<Installed> {
    let mut out = Vec::new();
    for g in &db.games {
        for d in &g.detect {
            let found = match d {
                Detect::Registry { path, key, .. } => {
                    let (hive, sub) = path.split_once('/').unwrap_or((path, ""));
                    let root = if hive.eq_ignore_ascii_case("HKEY_CURRENT_USER") { HKEY_CURRENT_USER } else { HKEY_LOCAL_MACHINE };
                    reg_any_view(root, &sub.replace('/', "\\"), key).map(|p| ("Registry", p))
                }
                Detect::Gog(id) => {
                    let sub = format!(r"SOFTWARE\WOW6432Node\GOG.com\Games\{id}");
                    reg_string(HKEY_LOCAL_MACHINE, &sub, "path", RRF_SUBKEY_WOW6464KEY).map(|p| ("GOG Galaxy", p))
                }
                Detect::Uplay(id) => {
                    let sub = format!(r"SOFTWARE\WOW6432Node\Ubisoft\Launcher\Installs\{id}");
                    reg_string(HKEY_LOCAL_MACHINE, &sub, "InstallDir", RRF_SUBKEY_WOW6464KEY).map(|p| ("Ubisoft Connect", p))
                }
                _ => None,
            };
            if let Some((source, dir)) = found.filter(|(_, d)| Path::new(d).is_dir()) {
                out.push(Installed { name: g.name.clone(), dir, source, ghub_id: Some(g.id.clone()) });
            }
        }
    }
    out
}

/// Installed games: de-duplicated by folder, games G HUB knows first, then by name.
pub fn scan(db: Option<&GameDb>) -> Vec<Installed> {
    let mut all = scan_steam(db);
    all.extend(scan_epic(db));
    all.extend(scan_riot(db));
    if let Some(d) = db {
        all.extend(scan_registry(d));
    }
    let mut seen = Vec::new();
    all.retain(|g| {
        let k = norm_key(&g.dir);
        let fresh = !seen.contains(&k);
        seen.push(k);
        fresh
    });
    all.sort_by(|a, b| b.ghub_id.is_some().cmp(&a.ghub_id.is_some()).then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase())));
    all
}

// ------------------------------------------------------------------ game sensitivity

/// A game's own mouse sensitivity and what it means for Perfect 180 (read-only).
#[derive(Clone, Debug, PartialEq)]
pub struct GameSens {
    pub game: &'static str,
    /// Index into `P180_GAMES`.
    pub preset: usize,
    pub sens: f32,
    pub yaw: f32,
    pub counts: i32,
    /// Where the value was read from — the settings file's basename (and for
    /// Fortnite, the property that won), e.g. `ClientSettings.Sav: MouseSensitivityX`.
    pub source: String,
}

const CS2_ID: &str = "ca572e6a-c76d-4e34-bd64-3665c0d0b0c1";
const VALORANT_ID: &str = "48574816-5d5d-4e5b-b616-9c3af2c15bbf";
const FORTNITE_ID: &str = "b2fd8db8-b15c-49f0-91a8-7839ee611aaa";

/// `(sensitivity, m_yaw)` from `cs2_user_convars_0_slot0.vcfg`; `m_yaw` defaults to 0.022.
pub fn parse_cs2_convars(text: &str) -> Option<(f32, f32)> {
    let (mut sens, mut yaw) = (None, None);
    for l in text.lines() {
        let q = quoted(l);
        if q.len() >= 2 {
            match q[0].as_str() {
                "sensitivity" => sens = q[1].parse::<f32>().ok(),
                "m_yaw" => yaw = q[1].parse::<f32>().ok(),
                _ => {}
            }
        }
    }
    Some((sens?, yaw.unwrap_or(0.022)))
}

/// `MouseSensitivity` from `RiotUserSettings.ini`, rounded to 3 decimals.
pub fn parse_valorant_sens(text: &str) -> Option<f32> {
    let v = text.lines().find_map(|l| l.trim().strip_prefix("EAresFloatSettingName::MouseSensitivity="))?;
    let v: f32 = v.trim().parse().ok()?;
    Some((v * 1000.0).round() / 1000.0)
}

fn newest(paths: impl IntoIterator<Item = PathBuf>) -> Option<PathBuf> {
    paths.into_iter().filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p))).max_by_key(|(t, _)| *t).map(|(_, p)| p)
}

fn dirs_in(dir: &Path) -> Vec<PathBuf> {
    std::fs::read_dir(dir).into_iter().flatten().flatten().map(|e| e.path()).collect()
}

fn cs2_file() -> Option<PathBuf> {
    let steam = reg_any_view(HKEY_CURRENT_USER, r"Software\Valve\Steam", "SteamPath")?.replace('/', "\\");
    newest(dirs_in(&Path::new(&steam).join("userdata")).into_iter().map(|u| u.join(r"730\local\cfg\cs2_user_convars_0_slot0.vcfg")).filter(|p| p.is_file()))
}

fn valorant_file() -> Option<PathBuf> {
    let root = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join(r"VALORANT\Saved\Config");
    newest(dirs_in(&root).into_iter().map(|c| c.join(r"Windows\RiotUserSettings.ini")).filter(|p| p.is_file()))
}

/// [`parse_fortnite_sens`] plus the name of the key that won (provenance for
/// READ FROM GAME's `from …` caption).
fn fortnite_ini_sens(text: &str) -> Option<(&'static str, f32)> {
    let (mut x, mut y, mut legacy) = (None, None, None);
    for l in text.lines() {
        let Some((k, v)) = l.split_once('=') else { continue };
        let Ok(v) = v.trim().parse::<f32>() else { continue };
        match k.trim() {
            "MouseSensitivityX" => x = Some(v),
            "MouseSensitivityY" => y = Some(v),
            "MouseSensitivity" => legacy = Some(v),
            _ => {}
        }
    }
    let r4 = |v: f32| (v * 10000.0).round() / 10000.0;
    [("MouseSensitivityX", x), ("MouseSensitivityY", y), ("MouseSensitivity", legacy)].into_iter().find_map(|(k, v)| v.filter(|&v| v > 0.0).map(|v| (k, r4(v))))
}

/// `MouseSensitivityX`/`Y` (then the legacy `MouseSensitivity`, when positive)
/// from Fortnite's `GameUserSettings.ini` — the decimal the new UI accepts,
/// rounded to 4 decimals. X is what a horizontal turn uses; Y is the fallback.
pub fn parse_fortnite_sens(text: &str) -> Option<f32> {
    fortnite_ini_sens(text).map(|(_, v)| v)
}

/// The f32 of a FloatProperty `key` in a UE `ClientSettings.Sav`: each
/// property is `[i32 len][name\0][i32 len][type\0][tag][value]`, and the tag's
/// layout changed between Fortnite versions, so the value is located as the
/// 4 bytes right before the next length-prefixed ASCII name.
fn sav_float(data: &[u8], key: &str) -> Option<f32> {
    let mut name = key.as_bytes().to_vec();
    name.push(0);
    let after_name = data.windows(name.len()).position(|w| w == name)? + name.len();
    let scope = data.get(after_name..after_name + 96)?;
    let tag = &scope[scope.windows(14).position(|w| w == b"FloatProperty\0")? + 14..];
    for o in 9..tag.len().saturating_sub(4).min(48) {
        let n = i32::from_le_bytes(tag[o..o + 4].try_into().unwrap());
        if !(2..=64).contains(&n) {
            continue;
        }
        let n = n as usize;
        let Some(s) = tag.get(o + 4..o + 4 + n) else { break };
        if s[n - 1] == 0 && s[..n - 1].iter().all(|c| c.is_ascii_graphic() || *c == b' ') {
            return Some(f32::from_le_bytes(tag[o - 4..o].try_into().unwrap()));
        }
    }
    None
}

/// `MouseSensitivityX`/`Y` (then legacy `MouseSensitivity`) of a UE `.Sav`
/// blob plus which key won; values of 0 or less mean "not set".
fn fortnite_sav_key(data: &[u8]) -> Option<(&'static str, f32)> {
    let r4 = |v: f32| (v * 10000.0).round() / 10000.0;
    ["MouseSensitivityX", "MouseSensitivityY", "MouseSensitivity"].iter().find_map(|k| sav_float(data, k).filter(|&v| v > 0.0).map(|v| (*k, r4(v))))
}



/// `(sens, key, file)`: newest first of `GameUserSettings.ini` and every
/// `ClientSettings.Sav` (`Saved\Config` and per-account `Saved\Cloud\*`);
/// the first one that actually holds a sensitivity wins.
fn fortnite_file() -> Option<(f32, &'static str, PathBuf)> {
    let root = PathBuf::from(std::env::var("LOCALAPPDATA").ok()?).join("FortniteGame").join("Saved");
    let mut paths = vec![root.join(r"Config\WindowsClient\GameUserSettings.ini"), root.join(r"Config\ClientSettings.Sav")];
    paths.extend(dirs_in(&root.join("Cloud")).into_iter().map(|c| c.join("ClientSettings.Sav")));
    let mut cand: Vec<(std::time::SystemTime, PathBuf)> =
        paths.into_iter().filter(|p| p.is_file()).filter_map(|p| Some((std::fs::metadata(&p).ok()?.modified().ok()?, p))).collect();
    cand.sort_by_key(|(t, _)| *t);
    for (_, p) in cand.into_iter().rev() {
        let bytes = std::fs::read(&p).ok()?;
        let sens = if p.extension().is_some_and(|e| e.eq_ignore_ascii_case("sav")) {
            fortnite_sav_key(&bytes)
        } else {
            fortnite_ini_sens(&String::from_utf8_lossy(&bytes))
        };
        if let Some((k, s)) = sens {
            return Some((s, k, p));
        }
    }
    None
}

/// `basename` or `basename: key` for [`GameSens::source`].
fn sens_source(file: &Path, key: Option<&str>) -> String {
    let base = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    match key {
        Some(k) => format!("{base}: {k}"),
        None => base,
    }
}

/// Sensitivity of a supported game plus the file it came from (read-only).
pub fn game_sensitivity_file(ghub_id: Option<&str>, name: &str) -> Option<(GameSens, PathBuf)> {
    if ghub_id == Some(CS2_ID) || name == "Counter-Strike 2" {
        let file = cs2_file()?;
        let (sens, yaw) = parse_cs2_convars(&std::fs::read_to_string(&file).ok()?)?;
        let preset = if (yaw - 0.022).abs() < 1e-6 { 0 } else { P180_CUSTOM };
        return Some((GameSens { game: "Counter-Strike 2", preset, sens, yaw, counts: p180_counts_for(yaw, sens)?, source: sens_source(&file, None) }, file));
    }
    if ghub_id == Some(VALORANT_ID) || name.eq_ignore_ascii_case("Valorant") {
        let file = valorant_file()?;
        let sens = parse_valorant_sens(&std::fs::read_to_string(&file).ok()?)?;
        return Some((GameSens { game: "Valorant", preset: 1, sens, yaw: 0.07, counts: p180_counts_for(0.07, sens)?, source: sens_source(&file, None) }, file));
    }
    if ghub_id == Some(FORTNITE_ID) || name.eq_ignore_ascii_case("Fortnite") {
        // the file holds the decimal (0.0345); `p180_preset_counts` maps it to
        // the percentage the preset maths expects, same as the typed field
        let (sens, key, file) = fortnite_file()?;
        return Some((GameSens { game: "Fortnite", preset: P180_FORTNITE, sens, yaw: P180_GAMES[P180_FORTNITE].1, counts: p180_preset_counts(P180_FORTNITE, sens)?, source: sens_source(&file, Some(key)) }, file));
    }
    None
}

pub fn game_sensitivity(ghub_id: Option<&str>, name: &str) -> Option<GameSens> {
    game_sensitivity_file(ghub_id, name).map(|(g, _)| g)
}

/// `(ghub_id, name)` a `game_sensitivity` parser knows, per Perfect 180 preset
/// index — the link maps the pill to the game, not the other way around.
fn preset_game(preset: usize) -> Option<(&'static str, &'static str)> {
    match preset {
        0 => Some((CS2_ID, "Counter-Strike 2")),
        1 => Some((VALORANT_ID, "Valorant")),
        P180_FORTNITE => Some((FORTNITE_ID, "Fortnite")),
        _ => None,
    }
}

/// The sensitivity behind a Perfect 180 preset's pill: `None` when the preset
/// is CUSTOM, has no parser, or the settings file is absent.
pub fn preset_sensitivity(preset: usize) -> Option<GameSens> {
    let (id, name) = preset_game(preset)?;
    game_sensitivity(Some(id), name)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_cs2_convars() {
        let t = "\"config\"\n{\n\t\"convars\"\n\t{\n\t\t\"m_pitch\"\t\t\"0.022\"\n\t\t\"m_yaw\"\t\t\"0.022\"\n\t\t\"sensitivity\"\t\t\"0.51\"\n\t\t\"zoom_sensitivity_ratio\"\t\t\"1\"\n\t}\n}\n";
        assert_eq!(parse_cs2_convars(t), Some((0.51, 0.022)));
        let no_yaw: String = t.lines().filter(|l| !l.contains("m_yaw")).collect::<Vec<_>>().join("\n");
        assert_eq!(parse_cs2_convars(&no_yaw), Some((0.51, 0.022)));
        assert_eq!(parse_cs2_convars("\"zoom_sensitivity_ratio\"\t\t\"1\"\n"), None);
    }

    #[test]
    fn parses_valorant_sens() {
        let t = "[/Script/X]\nFoo=1\nEAresFloatSettingName::MouseSensitivity=0.159999996\nBar=2\n";
        assert_eq!(parse_valorant_sens(t), Some(0.16));
        assert_eq!(parse_valorant_sens("Foo=1\n"), None);
    }

    #[test]
    fn parses_fortnite_sens() {
        let t = "[/Script/FortniteGame.FortGameUserSettings]\nbUseLegacyInput=False\nMouseSensitivityX=0.138\nMouseSensitivityY=0.12\nMouseTargetingSensitivity=0.55\n";
        // X is the axis a horizontal turn uses
        assert_eq!(parse_fortnite_sens(t), Some(0.138));
        // the key that won is reported for the `from …` caption
        assert_eq!(fortnite_ini_sens(t), Some(("MouseSensitivityX", 0.138)));
        // Y is the fallback when X is missing
        assert_eq!(parse_fortnite_sens("MouseSensitivityY=0.12\n"), Some(0.12));
        assert_eq!(fortnite_ini_sens("MouseSensitivityX=-1\nMouseSensitivityY=0.12\n"), Some(("MouseSensitivityY", 0.12)));
        // the legacy single value counts only when positive
        assert_eq!(parse_fortnite_sens("MouseSensitivity=-1\n"), None);
        assert_eq!(parse_fortnite_sens("MouseSensitivityX=-1\nMouseSensitivity=0.55\n"), Some(0.55));
        assert_eq!(fortnite_ini_sens("MouseSensitivityX=-1\nMouseSensitivity=0.55\n"), Some(("MouseSensitivity", 0.55)));
        assert_eq!(parse_fortnite_sens("bDisableMouseAcceleration=False\n"), None);
    }

    /// Minimal `ClientSettings.Sav` blob: the value is the last f32 before the
    /// next length-prefixed name, whatever the tag's exact layout.
    fn sav_prop(name: &str, tag: &[u8], value: f32) -> Vec<u8> {
        let mut v = Vec::new();
        v.extend((name.len() as i32 + 1).to_le_bytes());
        v.extend(name.as_bytes());
        v.push(0);
        v.extend(14i32.to_le_bytes());
        v.extend(b"FloatProperty\0");
        v.extend(tag);
        v.extend(value.to_le_bytes());
        v
    }

    #[test]
    fn parses_fortnite_sav() {
        // two tag layouts seen on this PC: i64 size + flag (current) and an
        // older file with an extra zeroed i32 in front of the i32 size
        let mut d = sav_prop("MouseSensitivity", &[4, 0, 0, 0, 0, 0, 0, 0, 0], -1.0);
        d.extend(sav_prop("MouseSensitivityX", &[0, 0, 0, 0, 0, 4, 0, 0, 0, 0], 0.0145));
        d.extend(sav_prop("MouseSensitivityY", &[0, 0, 0, 0, 0, 4, 0, 0, 0, 0], 0.0145));
        d.extend(13i32.to_le_bytes());
        d.extend(b"NextProperty\0");
        // the winning key is reported too (READ FROM GAME's `from …` caption)
        assert_eq!(fortnite_sav_key(&d), Some(("MouseSensitivityX", 0.0145)));
        // legacy -1 = unset -> keep looking (X/Y missing here)
        assert_eq!(fortnite_sav_key(&sav_prop("MouseSensitivity", &[4, 0, 0, 0, 0, 0, 0, 0, 0], -1.0)), None);
        assert_eq!(fortnite_sav_key(b"no keys at all"), None);
        // Y wins when X is absent; the legacy key is the last resort. The
        // trailing zeroes stand in for the rest of a real .sav (sav_float
        // looks ~96 bytes past each name for the following property).
        let mut d = sav_prop("MouseSensitivity", &[4, 0, 0, 0, 0, 0, 0, 0, 0], 0.5);
        d.extend(sav_prop("MouseSensitivityY", &[0, 0, 0, 0, 0, 4, 0, 0, 0, 0], 0.02));
        d.extend(13i32.to_le_bytes());
        d.extend(b"NextProperty\0");
        d.extend([0u8; 120]);
        assert_eq!(fortnite_sav_key(&d), Some(("MouseSensitivityY", 0.02)));
        let mut d = sav_prop("MouseSensitivity", &[4, 0, 0, 0, 0, 0, 0, 0, 0], 0.5);
        d.extend(13i32.to_le_bytes());
        d.extend(b"NextProperty\0");
        d.extend([0u8; 120]);
        assert_eq!(fortnite_sav_key(&d), Some(("MouseSensitivity", 0.5)));
    }

    #[test]
    fn fortnite_real_file_when_present() {
        let Some((sens, key, file)) = fortnite_file() else { return };
        println!("fortnite sens {sens} from {} ({key})", file.display());
        assert!((0.001..=1.0).contains(&sens), "{sens}");
        let g = game_sensitivity(Some(FORTNITE_ID), "").unwrap();
        assert_eq!(g.sens, sens);
        // provenance names the winning file and property
        assert_eq!(g.source, format!("{}: {key}", file.file_name().unwrap().to_string_lossy()));
    }

    #[test]
    fn sensitivity_gives_half_turn_counts() {
        assert_eq!(p180_counts_for(0.022, 0.51), Some(16043));
        assert_eq!(p180_counts_for(0.07, 0.16), Some(16071));
    }

    #[test]
    fn parses_library_folders() {
        let t = "\"libraryfolders\"\n{\n\t\"0\"\n\t{\n\t\t\"path\"\t\t\"C:\\\\Program Files (x86)\\\\Steam\"\n\t\t\"label\"\t\t\"\"\n\t\t\"apps\"\n\t\t{\n\t\t\t\"730\"\t\t\"71799502864\"\n\t\t}\n\t}\n}\n";
        assert_eq!(parse_library_folders(t), vec!["C:\\Program Files (x86)\\Steam".to_string()]);
    }

    #[test]
    fn parses_acf() {
        let t = "\"AppState\"\n{\n\t\"appid\"\t\t\"730\"\n\t\"universe\"\t\t\"1\"\n\t\"name\"\t\t\"Counter-Strike 2\"\n\t\"StateFlags\"\t\t\"4\"\n\t\"installdir\"\t\t\"Counter-Strike Global Offensive\"\n}";
        assert_eq!(parse_acf(t), Some(("730".into(), "Counter-Strike 2".into(), "Counter-Strike Global Offensive".into())));
    }

    #[test]
    fn parses_epic_item() {
        let t = r#"{"AppName": "Fortnite", "DisplayName": "Fortnite", "InstallLocation": "D:\\Epic Games\\Fortnite", "CatalogNamespace": "fn", "CatalogItemId": "4fe75bbc5a674f4f9b356b5c90567da5", "MainGameAppName": "Fortnite"}"#;
        let (name, dir, ids) = parse_epic_item(t).unwrap();
        assert_eq!((name.as_str(), dir.as_str()), ("Fortnite", r"D:\Epic Games\Fortnite"));
        assert!(ids.contains(&"Fortnite".to_string()) && ids.contains(&"4fe75bbc5a674f4f9b356b5c90567da5".to_string()));
    }

    #[test]
    fn parses_riot_install() {
        let t = "product_id: \"valorant.live\"\nproduct_install_full_path: \"C:/Riot Games/VALORANT/live\"\nother: 1\n";
        assert_eq!(parse_riot_install(t).as_deref(), Some("C:/Riot Games/VALORANT/live"));
    }

    #[test]
    fn key_combo_rules() {
        assert_eq!(key_combo(&toks(&["W"])), Some(KeyCombo { vk: 0x57, mods: 0 }));
        assert_eq!(key_combo(&toks(&["LCTRL", "C"])), Some(KeyCombo { vk: 0x43, mods: MOD_CTRL }));
        assert_eq!(key_combo(&toks(&["LSHIFT"])), Some(KeyCombo { vk: 0xA0, mods: 0 }));
        assert_eq!(key_combo(&toks(&["LCTRL", "LSHIFT"])), Some(KeyCombo { vk: 0xA0, mods: MOD_CTRL }));
        assert_eq!(key_combo(&toks(&["W", "A"])), None);
        assert_eq!(key_combo(&toks(&["NOPE"])), None);
        let mut all: Vec<String> = ('A'..='Z').chain('0'..='9').map(String::from).collect();
        all.extend((1..=24).map(|n| format!("F{n}")));
        all.extend((0..=9).map(|n| format!("NUM{n}")));
        all.extend(
            "NUMENTER NUMLOCK NUMMINUS NUMPERIOD NUMPLUS NUMSLASH NUMSTAR APPKEY BACKSLASH BACKSPACE CAPSLOCK COMMA DELETE DOWN END ENTER EQUAL ESCAPE HASH HOME INSERT LBRACKET LEFT MAIL MINUS NON_US_SLASH PAGEDOWN PAGEUP PAUSE PERIOD PRINTSCREEN QUOTE RBRACKET RIGHT SCROLLLOCK SEMICOLON SLASH SPACEBAR TAB TILDE UP LCTRL RCTRL LSHIFT RSHIFT LALT RALT LGUI"
                .split(' ')
                .map(String::from),
        );
        assert_eq!(all.len(), 118);
        for t in &all {
            assert!(key_combo(std::slice::from_ref(t)).is_some(), "{t}");
        }
    }

    #[test]
    fn db_parses_sample() {
        let json = r##"{"applications": [
            {"applicationId": "id-1", "name": "Game One",
             "detection": [{"steam": {"appId": "730"}}, {"riotGames": {"appName": "valorant.live"}},
                           {"winRegistry": {"executable": "r5apex.exe", "registryKey": "Install Dir", "registryPath": "HKEY_LOCAL_MACHINE/SOFTWARE/Respawn/Apex"}},
                           {"glob": {"x": "y"}}],
             "categoryColors": [{"hex": "#e40c0c", "tag": "Movement"}, {"hex": "#00ff10", "tag": "Combat"}],
             "commands": [{"category": "Movement", "keystroke": ["W"], "name": "Forward"},
                          {"category": "Combat", "keystroke": ["LCTRL", "C"], "name": "Crouch"},
                          {"category": "Movement", "keystroke": ["S"], "name": "Back"}]},
            {"applicationId": "id-2", "name": "Game Two", "detection": [], "commands": []}]}"##;
        let db = GameDb::parse(json).unwrap();
        assert_eq!(db.games.len(), 2);
        let g = db.by_id("id-1").unwrap();
        assert_eq!(g.name, "Game One");
        assert_eq!(
            g.detect,
            vec![
                Detect::Steam("730".into()),
                Detect::Riot("valorant.live".into()),
                Detect::Registry { path: "HKEY_LOCAL_MACHINE/SOFTWARE/Respawn/Apex".into(), key: "Install Dir".into(), exe: Some("r5apex.exe".into()) }
            ]
        );
        assert_eq!(g.categories, vec![("Movement".to_string(), Some([0xe4, 0x0c, 0x0c])), ("Combat".to_string(), Some([0x00, 0xff, 0x10]))]);
        assert_eq!(g.commands.len(), 3);
        assert!(db.by_id("id-2").unwrap().commands.is_empty());
    }

    #[test]
    fn real_db_when_present() {
        let Some(db) = GameDb::load() else { return };
        assert!(db.games.len() >= 800, "{}", db.games.len());
        assert!(db.by_name("Counter-Strike 2").unwrap().detect.contains(&Detect::Steam("730".into())));
        let (mut total, mut mapped) = (0, 0);
        for c in db.games.iter().flat_map(|g| &g.commands) {
            total += 1;
            mapped += key_combo(&c.keys).is_some() as usize;
        }
        assert!(mapped * 100 >= total * 95, "{mapped}/{total}");
    }
}
