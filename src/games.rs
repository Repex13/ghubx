//! G HUB's game database (read at runtime from the local G HUB install) and
//! a scan for installed games (Steam, Epic, Riot, GOG, Ubisoft, registry rules).

use crate::profile::{KeyCombo, MOD_ALT, MOD_CTRL, MOD_SHIFT, MOD_WIN};
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

#[cfg(test)]
mod tests {
    use super::*;

    fn toks(t: &[&str]) -> Vec<String> {
        t.iter().map(|s| s.to_string()).collect()
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
