//! Scripted UI input for `ghubx.exe --dev --script <file>`: synthetic egui events,
//! one script line per frame, so tests can click the real UI without touching the
//! user's mouse.

use egui::{Event, Key, Modifiers, MouseWheelSource, MouseWheelUnit, PointerButton, Pos2, TouchPhase, pos2, vec2};
use std::collections::VecDeque;
use std::path::PathBuf;
use std::time::{Duration, Instant};

#[derive(Clone, Debug, PartialEq)]
pub enum Cmd {
    Wait(u64),
    Move(f32, f32),
    Click(f32, f32),
    RClick(f32, f32),
    DClick(f32, f32),
    Drag(f32, f32, f32, f32),
    Scroll(f32, f32, f32),
    Key(Key),
    Text(String),
    Shot(String),
    Quit,
}

fn nums<const N: usize>(args: &str, what: &str) -> Result<[f32; N], String> {
    let v: Vec<f32> = args.split_whitespace().map(|a| a.parse::<f32>().map_err(|_| format!("'{a}' is not a number"))).collect::<Result<_, _>>()?;
    v.try_into().map_err(|v: Vec<f32>| format!("{what} needs {N} numbers, got {}", v.len()))
}

/// Parse a script; errors read `script line N: <reason>`.
pub fn parse(text: &str) -> Result<Vec<Cmd>, String> {
    let mut out = Vec::new();
    for (i, raw) in text.lines().enumerate() {
        let line = raw.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let (cmd, args) = line.split_once(char::is_whitespace).map_or((line, ""), |(c, a)| (c, a.trim()));
        let parsed = match cmd {
            "wait" => args.parse::<u64>().map(Cmd::Wait).map_err(|_| "wait needs milliseconds".to_string()),
            "move" => nums::<2>(args, cmd).map(|[x, y]| Cmd::Move(x, y)),
            "click" => nums::<2>(args, cmd).map(|[x, y]| Cmd::Click(x, y)),
            "rclick" => nums::<2>(args, cmd).map(|[x, y]| Cmd::RClick(x, y)),
            "dclick" => nums::<2>(args, cmd).map(|[x, y]| Cmd::DClick(x, y)),
            "drag" => nums::<4>(args, cmd).map(|[a, b, c, d]| Cmd::Drag(a, b, c, d)),
            "scroll" => nums::<3>(args, cmd).map(|[x, y, dy]| Cmd::Scroll(x, y, dy)),
            "key" => Key::from_name(args).map(Cmd::Key).ok_or_else(|| format!("unknown key '{args}'")),
            "text" if !args.is_empty() => Ok(Cmd::Text(args.to_string())),
            "text" => Err("text needs a string".to_string()),
            "shot" if !args.is_empty() => Ok(Cmd::Shot(args.to_string())),
            "shot" => Err("shot needs a path".to_string()),
            "quit" => Ok(Cmd::Quit),
            other => Err(format!("unknown command '{other}'")),
        };
        out.push(parsed.map_err(|e| format!("script line {}: {e}", i + 1))?);
    }
    Ok(out)
}

enum Act {
    Wait(u64),
    Frame(Vec<Event>),
    Shot(PathBuf),
    Quit,
}

/// What the script wants from this frame.
#[derive(Default)]
pub struct Frame {
    pub events: Vec<Event>,
    pub shot: Option<PathBuf>,
    pub quit: bool,
}

pub struct Runner {
    acts: VecDeque<Act>,
    wait_until: Option<Instant>,
}

fn moved(p: Pos2) -> Event {
    Event::PointerMoved(p)
}

fn button(p: Pos2, b: PointerButton, pressed: bool) -> Event {
    Event::PointerButton { pos: p, button: b, pressed, modifiers: Modifiers::NONE }
}

impl Runner {
    pub fn new(cmds: &[Cmd]) -> Self {
        let mut acts = VecDeque::new();
        let click = |acts: &mut VecDeque<Act>, p: Pos2, b: PointerButton| {
            acts.push_back(Act::Frame(vec![moved(p)]));
            acts.push_back(Act::Frame(vec![button(p, b, true)]));
            acts.push_back(Act::Frame(vec![button(p, b, false)]));
        };
        for c in cmds {
            match c {
                Cmd::Wait(ms) => acts.push_back(Act::Wait(*ms)),
                Cmd::Move(x, y) => acts.push_back(Act::Frame(vec![moved(pos2(*x, *y))])),
                Cmd::Click(x, y) => click(&mut acts, pos2(*x, *y), PointerButton::Primary),
                Cmd::RClick(x, y) => click(&mut acts, pos2(*x, *y), PointerButton::Secondary),
                Cmd::DClick(x, y) => {
                    let p = pos2(*x, *y);
                    acts.push_back(Act::Frame(vec![moved(p)]));
                    for _ in 0..2 {
                        acts.push_back(Act::Frame(vec![button(p, PointerButton::Primary, true)]));
                        acts.push_back(Act::Frame(vec![button(p, PointerButton::Primary, false)]));
                    }
                }
                Cmd::Drag(x1, y1, x2, y2) => {
                    let (a, b) = (pos2(*x1, *y1), pos2(*x2, *y2));
                    acts.push_back(Act::Frame(vec![moved(a)]));
                    acts.push_back(Act::Frame(vec![button(a, PointerButton::Primary, true)]));
                    for i in 1..=6 {
                        acts.push_back(Act::Frame(vec![moved(a + (b - a) * (i as f32 / 6.0))]));
                    }
                    acts.push_back(Act::Frame(vec![button(b, PointerButton::Primary, false)]));
                }
                Cmd::Scroll(x, y, dy) => {
                    acts.push_back(Act::Frame(vec![moved(pos2(*x, *y)), Event::MouseWheel { unit: MouseWheelUnit::Point, delta: vec2(0.0, *dy), phase: TouchPhase::Move, source: MouseWheelSource::Unknown, modifiers: Modifiers::NONE }]));
                }
                Cmd::Key(k) => {
                    for pressed in [true, false] {
                        acts.push_back(Act::Frame(vec![Event::Key { key: *k, physical_key: Some(*k), pressed, repeat: false, modifiers: Modifiers::NONE }]));
                    }
                }
                Cmd::Text(t) => acts.push_back(Act::Frame(vec![Event::Text(t.clone())])),
                Cmd::Shot(p) => acts.push_back(Act::Shot(PathBuf::from(p))),
                Cmd::Quit => acts.push_back(Act::Quit),
            }
        }
        Self { acts, wait_until: None }
    }

    pub fn finished(&self) -> bool {
        self.acts.is_empty()
    }

    /// The next frame's worth of script.
    pub fn next_frame(&mut self) -> Frame {
        let mut out = Frame::default();
        match self.acts.front() {
            None => {}
            Some(Act::Wait(ms)) => {
                let until = *self.wait_until.get_or_insert_with(|| Instant::now() + Duration::from_millis(*ms));
                if Instant::now() >= until {
                    self.wait_until = None;
                    self.acts.pop_front();
                }
            }
            Some(_) => match self.acts.pop_front() {
                Some(Act::Frame(e)) => out.events = e,
                Some(Act::Shot(p)) => out.shot = Some(p),
                Some(Act::Quit) => out.quit = true,
                _ => {}
            },
        }
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_parses_every_command() {
        let text = "# comment\n\nwait 250\nmove 1 2\nclick 3 4\nrclick 5 6\ndclick 7 8\ndrag 1 2 3 4\nscroll 9 10 -40\nkey Enter\ntext hello world\nshot out.png\nquit\n";
        assert_eq!(
            parse(text).unwrap(),
            vec![
                Cmd::Wait(250),
                Cmd::Move(1.0, 2.0),
                Cmd::Click(3.0, 4.0),
                Cmd::RClick(5.0, 6.0),
                Cmd::DClick(7.0, 8.0),
                Cmd::Drag(1.0, 2.0, 3.0, 4.0),
                Cmd::Scroll(9.0, 10.0, -40.0),
                Cmd::Key(Key::Enter),
                Cmd::Text("hello world".into()),
                Cmd::Shot("out.png".into()),
                Cmd::Quit,
            ]
        );
        assert!(parse("wait 1\nclik 1 2").unwrap_err().starts_with("script line 2:"));
        assert!(parse("click 1").unwrap_err().starts_with("script line 1:"));
        assert!(parse("\n\nkey Nope").unwrap_err().starts_with("script line 3:"));
    }

    #[test]
    fn click_takes_three_frames_and_wait_blocks() {
        let mut r = Runner::new(&[Cmd::Click(1.0, 1.0), Cmd::Wait(30), Cmd::Quit]);
        assert!(matches!(r.next_frame().events[..], [Event::PointerMoved(_)]));
        assert!(matches!(r.next_frame().events[..], [Event::PointerButton { pressed: true, .. }]));
        assert!(matches!(r.next_frame().events[..], [Event::PointerButton { pressed: false, .. }]));
        assert!(!r.next_frame().quit);
        std::thread::sleep(Duration::from_millis(40));
        r.next_frame();
        assert!(r.next_frame().quit);
        assert!(r.finished());
    }
}
