use super::*;
use std::net::TcpListener;
use std::sync::atomic::{AtomicU16, Ordering};
use tungstenite::protocol::CloseFrame;
use tungstenite::protocol::frame::coding::CloseCode;

type Log = Arc<Mutex<Vec<(String, Value)>>>;
type Answer = fn(&str, &Value) -> (bool, String, Value);

struct Fake {
    port: u16,
    log: Log,
    identifies: Arc<Mutex<Vec<Option<String>>>>,
}

fn default_answer(kind: &str, _data: &Value) -> (bool, String, Value) {
    let data = match kind {
        "GetSpecialInputs" => json!({"desktop1": "Desktop Audio", "mic1": "Mic/Aux"}),
        "GetSceneList" => json!({"currentProgramSceneName": "Live", "scenes": [{"sceneName": "Live"}, {"sceneName": "BRB"}]}),
        "GetSceneItemId" => json!({"sceneItemId": 7}),
        "GetSceneItemEnabled" => json!({"sceneItemEnabled": true}),
        "GetSceneCollectionList" => json!({"sceneCollections": ["Main", "Alt"]}),
        "GetInputList" => json!({"inputs": [{"inputName": "Mic/Aux"}, {"inputName": "Desktop Audio"}]}),
        "GetSceneItemList" => json!({"sceneItems": [{"sourceName": "Cam"}, {"sourceName": "Game"}]}),
        _ => json!({}),
    };
    (true, String::new(), data)
}

fn serve(listener: TcpListener, password: Option<String>, answer: Answer, log: Log, identifies: Arc<Mutex<Vec<Option<String>>>>) {
    std::thread::spawn(move || {
        for stream in listener.incoming().flatten() {
            let Ok(mut ws) = tungstenite::accept(stream) else { continue };
            let mut d = json!({"obsStudioVersion": "33.0.0", "obsWebSocketVersion": "5.5.0", "rpcVersion": 1});
            let (salt, challenge) = ("lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=");
            if password.is_some() {
                d["authentication"] = json!({"salt": salt, "challenge": challenge});
            }
            let _ = ws.send(Message::text(json!({"op": 0, "d": d}).to_string()));
            let Ok(Some(id)) = recv_json(&mut ws) else { continue };
            let sent = id["d"]["authentication"].as_str().map(str::to_string);
            identifies.lock().unwrap().push(sent.clone());
            if let Some(pw) = &password
                && sent.as_deref() != Some(auth_string(pw, salt, challenge).as_str())
            {
                let _ = ws.close(Some(CloseFrame { code: CloseCode::Library(4009), reason: "".into() }));
                let _ = ws.flush();
                continue;
            }
            let _ = ws.send(Message::text(json!({"op": 2, "d": {"negotiatedRpcVersion": 1}}).to_string()));
            while let Ok(m) = recv_json(&mut ws) {
                let Some(v) = m else { continue };
                if v["op"] != 6 {
                    continue;
                }
                let (kind, data) = (v["d"]["requestType"].as_str().unwrap_or("").to_string(), v["d"]["requestData"].clone());
                log.lock().unwrap().push((kind.clone(), data.clone()));
                let (ok, comment, resp) = answer(&kind, &data);
                let reply = json!({"op": 7, "d": {"requestType": kind, "requestId": v["d"]["requestId"], "requestStatus": {"result": ok, "code": if ok { 100 } else { 600 }, "comment": comment}, "responseData": resp}});
                if ws.send(Message::text(reply.to_string())).is_err() {
                    break;
                }
            }
        }
    });
}

fn fake(password: Option<&str>, answer: Answer) -> Fake {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let (log, identifies): (Log, Arc<Mutex<Vec<Option<String>>>>) = (Log::default(), Arc::default());
    serve(listener, password.map(str::to_string), answer, log.clone(), identifies.clone());
    Fake { port, log, identifies }
}

fn handle_for(f: &Fake, password: Option<&str>) -> Handle {
    let (port, pw) = (f.port, password.map(str::to_string));
    Handle::spawn_at(move || Ok(ObsEndpoint { port, password: pw.clone() }))
}

fn wait_for(mut f: impl FnMut() -> bool) -> bool {
    let end = std::time::Instant::now() + Duration::from_secs(5);
    while std::time::Instant::now() < end {
        if f() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(10));
    }
    false
}

fn kinds(f: &Fake) -> Vec<String> {
    f.log.lock().unwrap().iter().map(|(k, _)| k.clone()).collect()
}

#[test]
fn auth_string_matches_the_protocol_vector() {
    assert_eq!(auth_string("supersecretpassword", "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY="), "1Ct943GAT+6YQUUX47Ia/ncufilbe6+oD6lY+5kaCu4=");
}

#[test]
fn endpoint_comes_from_the_settings_file() {
    let dir = std::env::temp_dir().join(format!("ghubx_obs_test_{}", std::process::id()));
    let cfg = dir.join("plugin_config").join("obs-websocket");
    let marker = dir.join("install");
    let _ = std::fs::remove_dir_all(&dir);
    assert_eq!(read_endpoint_in(&dir, &marker).err(), Some(ObsStatus::NotInstalled));
    std::fs::create_dir_all(&marker).unwrap();
    assert_eq!(read_endpoint_in(&dir, &marker).err(), Some(ObsStatus::NeverStarted));
    std::fs::create_dir_all(&cfg).unwrap();
    let file = cfg.join("config.json");
    std::fs::write(&file, r#"{"server_enabled": true, "server_port": 4460, "server_password": "pw", "auth_required": true}"#).unwrap();
    let ep = read_endpoint_in(&dir, &marker).ok().unwrap();
    assert_eq!((ep.port, ep.password.as_deref()), (4460, Some("pw")));
    std::fs::write(&file, r#"{"server_enabled": true, "server_port": 4460, "server_password": "pw", "auth_required": false}"#).unwrap();
    assert_eq!(read_endpoint_in(&dir, &marker).ok().unwrap().password, None);
    std::fs::write(&file, r#"{"server_enabled": false, "server_port": 4460}"#).unwrap();
    assert_eq!(read_endpoint_in(&dir, &marker).err(), Some(ObsStatus::ServerOff));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn connects_without_a_password_and_runs_an_action() {
    let f = fake(None, default_answer);
    let h = handle_for(&f, None);
    h.run(ObsAction::ToggleRecord);
    assert!(wait_for(|| kinds(&f) == ["ToggleRecord"]));
    assert_eq!(f.identifies.lock().unwrap().as_slice(), [None]);
    assert!(wait_for(|| h.status() == ObsStatus::Connected { version: "33.0.0".into() }));
}

#[test]
fn password_is_sent_as_the_auth_string_and_a_wrong_one_is_refused() {
    let f = fake(Some("right"), default_answer);
    let h = handle_for(&f, Some("right"));
    h.run(ObsAction::StartStream);
    assert!(wait_for(|| kinds(&f) == ["StartStream"]));
    let want = auth_string("right", "lM1GncleQOaCu9lT1yeUZhFYnqhsLLP1G5lAGo3ixaI=", "+IxH4CnCiqpX1rM9scsNynZzbOe4KhDeYcTNS3PDaeY=");
    assert_eq!(f.identifies.lock().unwrap()[0].as_deref(), Some(want.as_str()));
    let f = fake(Some("right"), default_answer);
    let h = handle_for(&f, Some("wrong"));
    h.run(ObsAction::StartStream);
    assert!(wait_for(|| h.status() == ObsStatus::WrongPassword));
    assert!(kinds(&f).is_empty());
}

#[test]
fn mute_desktop_looks_up_the_input_first() {
    let f = fake(None, default_answer);
    let h = handle_for(&f, None);
    h.run(ObsAction::MuteDesktop);
    assert!(wait_for(|| kinds(&f).len() == 2));
    assert_eq!(kinds(&f), ["GetSpecialInputs", "SetInputMute"]);
    assert_eq!(f.log.lock().unwrap()[1].1, json!({"inputName": "Desktop Audio", "inputMuted": true}));
    fn none(kind: &str, d: &Value) -> (bool, String, Value) {
        if kind == "GetSpecialInputs" { (true, String::new(), json!({"desktop1": null})) } else { default_answer(kind, d) }
    }
    let f = fake(None, none);
    let h = handle_for(&f, None);
    h.run(ObsAction::MuteDesktop);
    assert!(wait_for(|| h.last_error().is_some_and(|e| e.contains("no desktop audio input"))));
    assert_eq!(kinds(&f), ["GetSpecialInputs"]);
}

#[test]
fn source_toggle_reads_the_item_state_first() {
    let f = fake(None, default_answer);
    let h = handle_for(&f, None);
    h.run(ObsAction::SourceToggle("Cam".into()));
    assert!(wait_for(|| kinds(&f).len() == 4));
    assert_eq!(kinds(&f), ["GetSceneList", "GetSceneItemId", "GetSceneItemEnabled", "SetSceneItemEnabled"]);
    assert_eq!(f.log.lock().unwrap()[3].1, json!({"sceneName": "Live", "sceneItemId": 7, "sceneItemEnabled": false}));
}

#[test]
fn scene_collection_and_mixer_send_their_data() {
    let f = fake(None, default_answer);
    let h = handle_for(&f, None);
    h.run(ObsAction::Scene("BRB".into()));
    h.run(ObsAction::Collection("Alt".into()));
    h.run(ObsAction::MixerToggle("Mic/Aux".into()));
    assert!(wait_for(|| kinds(&f).len() == 3));
    let log = f.log.lock().unwrap().clone();
    assert_eq!(log[0], ("SetCurrentProgramScene".to_string(), json!({"sceneName": "BRB"})));
    assert_eq!(log[1], ("SetCurrentSceneCollection".to_string(), json!({"sceneCollectionName": "Alt"})));
    assert_eq!(log[2], ("ToggleInputMute".to_string(), json!({"inputName": "Mic/Aux"})));
}

#[test]
fn a_refused_request_sets_the_error_to_obs_comment() {
    fn refuse(_: &str, _: &Value) -> (bool, String, Value) {
        (false, "Stream is already active".into(), json!({}))
    }
    let f = fake(None, refuse);
    let h = handle_for(&f, None);
    h.run(ObsAction::StartStream);
    assert!(wait_for(|| h.last_error().as_deref() == Some("Start Streaming: Stream is already active")));
}

#[test]
fn refresh_fills_the_lists() {
    let f = fake(None, default_answer);
    let h = handle_for(&f, None);
    h.refresh();
    assert!(wait_for(|| h.lists().scenes.len() == 2));
    let l = h.lists();
    assert_eq!(l.scenes, ["Live", "BRB"]);
    assert_eq!(l.collections, ["Main", "Alt"]);
    assert_eq!(l.sources, ["Cam", "Game"]);
    assert_eq!(l.mixers, ["Mic/Aux", "Desktop Audio"]);
}

#[test]
fn not_running_then_connects_once_obs_is_there() {
    let dead = TcpListener::bind("127.0.0.1:0").unwrap().local_addr().unwrap().port();
    let port = Arc::new(AtomicU16::new(dead));
    let p = port.clone();
    let h = Handle::spawn_at(move || Ok(ObsEndpoint { port: p.load(Ordering::SeqCst), password: None }));
    h.run(ObsAction::SaveBuffer);
    assert!(wait_for(|| h.status() == ObsStatus::NotRunning));
    assert!(h.last_error().is_some_and(|e| e.contains("not running")));
    let f = fake(None, default_answer);
    port.store(f.port, Ordering::SeqCst);
    h.run(ObsAction::SaveBuffer);
    assert!(wait_for(|| kinds(&f) == ["SaveReplayBuffer"]));
}

#[test]
fn a_fixed_handle_reports_its_status_and_does_nothing() {
    let h = Handle::fixed(ObsStatus::NotRunning);
    h.run(ObsAction::StartStream);
    h.refresh();
    assert_eq!(h.status(), ObsStatus::NotRunning);
    assert!(h.last_error().is_none() && h.lists() == ObsLists::default());
}
