//! OBS Studio actions through OBS's built-in WebSocket server (obs-websocket v5, 127.0.0.1).
//! The password is read from OBS's own settings file at every connect and never stored or logged.

use crate::profile::ObsAction;
use base64::Engine as _;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::net::{SocketAddr, TcpStream};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Sender, channel};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tungstenite::{Message, WebSocket};

const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
const IO_TIMEOUT: Duration = Duration::from_secs(3);

#[derive(Clone, Debug, PartialEq, Default)]
pub enum ObsStatus {
    /// Nothing tried yet.
    #[default]
    Unknown,
    /// Neither the settings file nor the OBS install folder exists.
    NotInstalled,
    /// OBS is installed but has no settings file yet.
    NeverStarted,
    /// `server_enabled` is false.
    ServerOff,
    /// Connection refused or timed out.
    NotRunning,
    WrongPassword,
    Connected { version: String },
}

#[derive(Clone, Debug, PartialEq, Default)]
pub struct ObsLists {
    pub scenes: Vec<String>,
    pub collections: Vec<String>,
    pub sources: Vec<String>,
    pub mixers: Vec<String>,
}

/// Where and how to connect, read from OBS's own settings file.
pub struct ObsEndpoint {
    pub port: u16,
    pub password: Option<String>,
}

/// `%APPDATA%\obs-studio`.
pub fn obs_dir() -> PathBuf {
    Path::new(&std::env::var("APPDATA").unwrap_or_else(|_| ".".into())).join("obs-studio")
}

fn install_marker() -> PathBuf {
    PathBuf::from(r"C:\Program Files\obs-studio")
}

pub fn read_endpoint(obs_dir: &Path) -> Result<ObsEndpoint, ObsStatus> {
    read_endpoint_in(obs_dir, &install_marker())
}

/// `install_marker`: a folder whose existence means OBS is installed (injectable for tests).
pub fn read_endpoint_in(obs_dir: &Path, install_marker: &Path) -> Result<ObsEndpoint, ObsStatus> {
    let file = obs_dir.join("plugin_config").join("obs-websocket").join("config.json");
    let Ok(text) = std::fs::read_to_string(&file) else {
        return Err(if install_marker.exists() { ObsStatus::NeverStarted } else { ObsStatus::NotInstalled });
    };
    let v: Value = serde_json::from_str(&text).map_err(|_| ObsStatus::NeverStarted)?;
    if !v.get("server_enabled").and_then(Value::as_bool).unwrap_or(false) {
        return Err(ObsStatus::ServerOff);
    }
    let port = v.get("server_port").and_then(Value::as_u64).unwrap_or(4455) as u16;
    let auth = v.get("auth_required").and_then(Value::as_bool).unwrap_or(false);
    let password = auth.then(|| v.get("server_password").and_then(Value::as_str).unwrap_or("").to_string());
    Ok(ObsEndpoint { port, password })
}

/// base64(sha256(base64(sha256(password + salt)) + challenge)).
pub fn auth_string(password: &str, salt: &str, challenge: &str) -> String {
    let b64 = base64::engine::general_purpose::STANDARD;
    let secret = b64.encode(Sha256::digest(format!("{password}{salt}")));
    b64.encode(Sha256::digest(format!("{secret}{challenge}")))
}

// ---------------------------------------------------------------- connection

struct Conn {
    ws: WebSocket<TcpStream>,
    version: String,
    seq: u64,
}

enum Fail {
    Status(ObsStatus),
    Msg(String),
}

fn recv_json(ws: &mut WebSocket<TcpStream>) -> Result<Option<Value>, tungstenite::Error> {
    match ws.read()? {
        Message::Text(t) => Ok(serde_json::from_str(t.as_str()).ok()),
        _ => Ok(None),
    }
}

fn connect(endpoint: &dyn Fn() -> Result<ObsEndpoint, ObsStatus>) -> Result<Conn, ObsStatus> {
    let ep = endpoint()?;
    let addr: SocketAddr = ([127, 0, 0, 1], ep.port).into();
    let stream = TcpStream::connect_timeout(&addr, CONNECT_TIMEOUT).map_err(|_| ObsStatus::NotRunning)?;
    let _ = stream.set_read_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_write_timeout(Some(IO_TIMEOUT));
    let _ = stream.set_nodelay(true);
    let (mut ws, _) = tungstenite::client(format!("ws://127.0.0.1:{}", ep.port), stream).map_err(|_| ObsStatus::NotRunning)?;
    let hello = loop {
        match recv_json(&mut ws) {
            Ok(Some(v)) if v["op"] == 0 => break v,
            Ok(_) => {}
            Err(_) => return Err(ObsStatus::NotRunning),
        }
    };
    let d = &hello["d"];
    let version = d["obsStudioVersion"].as_str().unwrap_or("").to_string();
    let mut identify = json!({"rpcVersion": 1, "eventSubscriptions": 0});
    let mut authenticated = false;
    if let Some(a) = d.get("authentication").filter(|a| a.is_object()) {
        let Some(pw) = &ep.password else { return Err(ObsStatus::WrongPassword) };
        authenticated = true;
        identify["authentication"] = json!(auth_string(pw, a["salt"].as_str().unwrap_or(""), a["challenge"].as_str().unwrap_or("")));
    }
    ws.send(Message::text(json!({"op": 1, "d": identify}).to_string())).map_err(|_| ObsStatus::NotRunning)?;
    loop {
        match recv_json(&mut ws) {
            Ok(Some(v)) if v["op"] == 2 => break,
            Ok(_) => {}
            // OBS answers a wrong password by closing the socket (code 4009)
            Err(_) => return Err(if authenticated { ObsStatus::WrongPassword } else { ObsStatus::NotRunning }),
        }
    }
    Ok(Conn { ws, version, seq: 0 })
}

impl Conn {
    fn request(&mut self, kind: &str, data: Value) -> Result<Value, Fail> {
        self.seq += 1;
        let id = format!("ghubx-{}", self.seq);
        let msg = json!({"op": 6, "d": {"requestType": kind, "requestId": id, "requestData": data}});
        self.ws.send(Message::text(msg.to_string())).map_err(|_| Fail::Status(ObsStatus::NotRunning))?;
        loop {
            match recv_json(&mut self.ws) {
                Ok(Some(v)) if v["op"] == 7 && v["d"]["requestId"] == id => {
                    let st = &v["d"]["requestStatus"];
                    if st["result"].as_bool().unwrap_or(false) {
                        return Ok(v["d"]["responseData"].clone());
                    }
                    let comment = st["comment"].as_str().filter(|c| !c.is_empty()).map(str::to_string);
                    return Err(Fail::Msg(comment.unwrap_or_else(|| format!("OBS refused the request (code {})", st["code"].as_i64().unwrap_or(0)))));
                }
                Ok(_) => {}
                Err(_) => return Err(Fail::Status(ObsStatus::NotRunning)),
            }
        }
    }

    fn special_input(&mut self, key: &str, missing: &str) -> Result<String, Fail> {
        let r = self.request("GetSpecialInputs", Value::Null)?;
        match r[key].as_str() {
            Some(n) if !n.is_empty() => Ok(n.to_string()),
            _ => Err(Fail::Msg(missing.into())),
        }
    }

    fn mute(&mut self, input: &str, how: Option<bool>) -> Result<(), Fail> {
        match how {
            Some(m) => self.request("SetInputMute", json!({"inputName": input, "inputMuted": m}))?,
            None => self.request("ToggleInputMute", json!({"inputName": input}))?,
        };
        Ok(())
    }

    fn source(&mut self, name: &str, how: Option<bool>) -> Result<(), Fail> {
        let scene = self.request("GetSceneList", Value::Null)?["currentProgramSceneName"].as_str().unwrap_or("").to_string();
        let id = self.request("GetSceneItemId", json!({"sceneName": scene, "sourceName": name}))?["sceneItemId"].clone();
        let on = match how {
            Some(v) => v,
            None => !self.request("GetSceneItemEnabled", json!({"sceneName": scene, "sceneItemId": id}))?["sceneItemEnabled"].as_bool().unwrap_or(false),
        };
        self.request("SetSceneItemEnabled", json!({"sceneName": scene, "sceneItemId": id, "sceneItemEnabled": on}))?;
        Ok(())
    }

    fn run(&mut self, a: &ObsAction) -> Result<(), Fail> {
        use ObsAction::*;
        let plain = |c: &mut Conn, k: &str| c.request(k, Value::Null).map(|_| ());
        match a {
            StartStream => plain(self, "StartStream"),
            StopStream => plain(self, "StopStream"),
            ToggleStream => plain(self, "ToggleStream"),
            StartRecord => plain(self, "StartRecord"),
            StopRecord => plain(self, "StopRecord"),
            ToggleRecord => plain(self, "ToggleRecord"),
            StartBuffer => plain(self, "StartReplayBuffer"),
            StopBuffer => plain(self, "StopReplayBuffer"),
            ToggleBuffer => plain(self, "ToggleReplayBuffer"),
            SaveBuffer => plain(self, "SaveReplayBuffer"),
            MuteDesktop | UnmuteDesktop | ToggleDesktopMute => {
                let n = self.special_input("desktop1", "OBS has no desktop audio input")?;
                self.mute(&n, match a { MuteDesktop => Some(true), UnmuteDesktop => Some(false), _ => None })
            }
            MuteMic | UnmuteMic | ToggleMicMute => {
                let n = self.special_input("mic1", "OBS has no microphone input")?;
                self.mute(&n, match a { MuteMic => Some(true), UnmuteMic => Some(false), _ => None })
            }
            Scene(s) => self.request("SetCurrentProgramScene", json!({"sceneName": s})).map(|_| ()),
            Collection(s) => self.request("SetCurrentSceneCollection", json!({"sceneCollectionName": s})).map(|_| ()),
            SourceOn(s) => self.source(s, Some(true)),
            SourceOff(s) => self.source(s, Some(false)),
            SourceToggle(s) => self.source(s, None),
            MixerMute(s) => self.mute(s, Some(true)),
            MixerUnmute(s) => self.mute(s, Some(false)),
            MixerToggle(s) => self.mute(s, None),
        }
    }

    fn lists(&mut self) -> Result<ObsLists, Fail> {
        let names = |v: &Value, arr: &str, key: &str| -> Vec<String> { v[arr].as_array().map(|a| a.iter().filter_map(|x| x[key].as_str().map(str::to_string)).collect()).unwrap_or_default() };
        let scenes = self.request("GetSceneList", Value::Null)?;
        let cols = self.request("GetSceneCollectionList", Value::Null)?;
        let inputs = self.request("GetInputList", Value::Null)?;
        let cur = scenes["currentProgramSceneName"].as_str().unwrap_or("").to_string();
        let items = if cur.is_empty() { Value::Null } else { self.request("GetSceneItemList", json!({"sceneName": cur}))? };
        Ok(ObsLists {
            scenes: names(&scenes, "scenes", "sceneName"),
            collections: cols["sceneCollections"].as_array().map(|a| a.iter().filter_map(|x| x.as_str().map(str::to_string)).collect()).unwrap_or_default(),
            sources: names(&items, "sceneItems", "sourceName"),
            mixers: names(&inputs, "inputs", "inputName"),
        })
    }
}

// ---------------------------------------------------------------- handle

#[derive(Default)]
struct Shared {
    status: ObsStatus,
    lists: ObsLists,
    error: Option<String>,
}

enum Cmd {
    Run(ObsAction),
    Refresh,
}

/// Queues OBS commands for one worker thread; never blocks the caller.
#[derive(Clone)]
pub struct Handle {
    tx: Option<Sender<Cmd>>,
    shared: Arc<Mutex<Shared>>,
}

fn lock(s: &Mutex<Shared>) -> std::sync::MutexGuard<'_, Shared> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

fn why(s: &ObsStatus) -> &'static str {
    match s {
        ObsStatus::NotInstalled => "OBS Studio is not installed",
        ObsStatus::NeverStarted => "OBS has not been started yet",
        ObsStatus::ServerOff => "the OBS WebSocket server is off",
        ObsStatus::WrongPassword => "OBS refused the password",
        _ => "OBS is not running",
    }
}

impl Handle {
    pub fn spawn() -> Self {
        Self::spawn_at(|| read_endpoint(&obs_dir()))
    }

    /// No thread, no network: observer mode and tests.
    pub fn fixed(status: ObsStatus) -> Self {
        Self { tx: None, shared: Arc::new(Mutex::new(Shared { status, ..Default::default() })) }
    }

    pub fn spawn_at(endpoint: impl Fn() -> Result<ObsEndpoint, ObsStatus> + Send + 'static) -> Self {
        let shared = Arc::new(Mutex::new(Shared::default()));
        let (tx, rx) = channel::<Cmd>();
        let s = shared.clone();
        std::thread::Builder::new()
            .name("obs".into())
            .spawn(move || {
                let mut conn: Option<Conn> = None;
                for cmd in rx {
                    if conn.is_none() {
                        match connect(&endpoint) {
                            Ok(c) => {
                                lock(&s).status = ObsStatus::Connected { version: c.version.clone() };
                                conn = Some(c);
                            }
                            Err(st) => {
                                let mut g = lock(&s);
                                if let Cmd::Run(a) = &cmd {
                                    g.error = Some(format!("{}: {}", crate::catalog::obs_name(a), why(&st)));
                                }
                                g.status = st;
                                continue;
                            }
                        }
                    }
                    let c = conn.as_mut().expect("connected");
                    let res = match &cmd {
                        Cmd::Run(a) => c.run(a).map(|_| None),
                        Cmd::Refresh => c.lists().map(Some),
                    };
                    match res {
                        Ok(lists) => {
                            let mut g = lock(&s);
                            if let Some(l) = lists {
                                g.lists = l;
                            } else {
                                g.error = None;
                            }
                        }
                        Err(Fail::Msg(m)) => {
                            let mut g = lock(&s);
                            if let Cmd::Run(a) = &cmd {
                                g.error = Some(format!("{}: {m}", crate::catalog::obs_name(a)));
                            }
                        }
                        Err(Fail::Status(st)) => {
                            conn = None;
                            let mut g = lock(&s);
                            if let Cmd::Run(a) = &cmd {
                                g.error = Some(format!("{}: {}", crate::catalog::obs_name(a), why(&st)));
                            }
                            g.status = st;
                        }
                    }
                }
            })
            .ok();
        Self { tx: Some(tx), shared }
    }

    /// Queue an action.
    pub fn run(&self, a: ObsAction) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Run(a));
        }
    }

    /// Queue: connect if needed, update status and lists.
    pub fn refresh(&self) {
        if let Some(tx) = &self.tx {
            let _ = tx.send(Cmd::Refresh);
        }
    }

    pub fn status(&self) -> ObsStatus {
        lock(&self.shared).status.clone()
    }

    pub fn lists(&self) -> ObsLists {
        lock(&self.shared).lists.clone()
    }

    /// The last failed action, e.g. "Toggle Streaming: OBS is not running".
    pub fn last_error(&self) -> Option<String> {
        lock(&self.shared).error.clone()
    }

    /// Like `last_error`, but forgets it (so a toast shows it once).
    pub fn take_error(&self) -> Option<String> {
        lock(&self.shared).error.take()
    }
}

#[cfg(test)]
mod tests;
