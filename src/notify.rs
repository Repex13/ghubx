//! The in-app notification feed behind the header bell.
//!
//! A `Feed` is a shared `VecDeque` of notes (newest last), capped at [`CAP`]
//! plus an `unread` counter the bell shows as a dot until MARK ALL READ.
//! `GhubApp` owns one; producers (the update worker, status transitions)
//! push into clones of the same `Arc`. Session-only: nothing is persisted.

use std::collections::VecDeque;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

/// Most notes kept; older ones fall off the front.
const CAP: usize = 50;

/// Minimum gap between Windows toasts raised from feed notes (Windows drops
/// faster balloon spam anyway, but keep the app clean).
pub const TOAST_GAP: Duration = Duration::from_secs(2);

/// Pure gate for the feed's toast hook: the `general.toasts` flag must be on
/// (observer-mode callers fold it to false, so dev/shot/scripted runs stay
/// inert), the app must own a tray icon, and at least [`TOAST_GAP`] must have
/// passed since the last balloon (`None` = the first one). A `last` in the
/// future counts as "too soon" rather than panicking.
pub fn maybe_toast(enabled: bool, tray_ready: bool, last: Option<Instant>, now: Instant) -> bool {
    enabled && tray_ready && last.is_none_or(|t| now.saturating_duration_since(t) >= TOAST_GAP)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteKind {
    /// Ordinary state change (G HUB started/closed, control hand-offs, profiles).
    Info,
    /// Something about a GHUB-X update.
    Update,
    /// A failure worth seeing (update check/download/apply failed).
    Warn,
}

#[derive(Clone, Debug)]
pub struct Note {
    pub text: String,
    pub at: Instant,
    pub kind: NoteKind,
}

#[derive(Default)]
struct Shared {
    notes: VecDeque<Note>,
    unread: usize,
}

fn lock<T>(s: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    s.lock().unwrap_or_else(|e| e.into_inner())
}

/// Runs on the pushing thread for every stored note — `GhubApp` hangs the
/// Windows-toast path off it.
type NoteHook = Arc<dyn Fn(&str) + Send + Sync>;

/// A clone shares the same feed (and its note hook).
#[derive(Clone, Default)]
pub struct Feed {
    shared: Arc<Mutex<Shared>>,
    /// The single funnel all producers go through (status transitions, the
    /// settings-db watcher, and the update worker); `None` until `GhubApp`
    /// installs it and in tests.
    hook: Arc<Mutex<Option<NoteHook>>>,
}

impl Feed {
    /// Append a note; it counts as unread until [`Feed::mark_read`].
    /// The note hook fires afterwards, outside the feed lock.
    pub fn push(&self, text: impl Into<String>, kind: NoteKind) {
        let text = text.into();
        {
            let mut g = lock(&self.shared);
            g.notes.push_back(Note { text: text.clone(), at: Instant::now(), kind });
            while g.notes.len() > CAP {
                g.notes.pop_front();
            }
            g.unread = (g.unread + 1).min(g.notes.len());
        }
        let f = lock(&self.hook).clone();
        if let Some(f) = f {
            f(&text);
        }
    }

    /// Install the note hook (replaces any previous one). It runs on the
    /// pushing thread — keep it cheap and never push a note back from it.
    pub fn set_note_hook(&self, f: impl Fn(&str) + Send + Sync + 'static) {
        *lock(&self.hook) = Some(Arc::new(f));
    }

    /// Newest first.
    pub fn notes(&self) -> Vec<Note> {
        lock(&self.shared).notes.iter().rev().cloned().collect()
    }

    pub fn unread(&self) -> usize {
        lock(&self.shared).unread
    }

    /// MARK ALL READ: clears the dot, keeps the notes.
    pub fn mark_read(&self) {
        lock(&self.shared).unread = 0;
    }
}

/// "just now" / "Nm ago" / "Nh ago" / "Nd ago" for the feed rows.
pub fn ago(at: Instant) -> String {
    let s = at.elapsed().as_secs();
    if s < 45 {
        "just now".into()
    } else if s < 3600 {
        format!("{}m ago", (s / 60).max(1))
    } else if s < 86_400 {
        format!("{}h ago", s / 3600)
    } else {
        format!("{}d ago", s / 86_400)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[test]
    fn feed_caps_at_50_and_counts_unread() {
        let f = Feed::default();
        for i in 0..55 {
            f.push(format!("note {i}"), NoteKind::Info);
        }
        let notes = f.notes();
        assert_eq!(notes.len(), 50);
        // newest first: "note 54" leads, the five oldest were dropped
        assert_eq!(notes[0].text, "note 54");
        assert_eq!(notes[49].text, "note 5");
        assert_eq!(f.unread(), 50);
    }

    #[test]
    fn note_hook_sees_every_push() {
        let f = Feed::default();
        let seen = Arc::new(Mutex::new(Vec::new()));
        let seen2 = seen.clone();
        f.set_note_hook(move |text| seen2.lock().unwrap().push(text.to_string()));
        f.push("one", NoteKind::Info);
        // a clone hits the same hook (the update worker pushes into a clone)
        f.clone().push("two", NoteKind::Update);
        assert_eq!(*seen.lock().unwrap(), vec!["one".to_string(), "two".to_string()]);
    }

    #[test]
    fn maybe_toast_gates_on_flag_tray_and_gap() {
        let now = Instant::now();
        assert!(maybe_toast(true, true, None, now));
        assert!(!maybe_toast(false, true, None, now));
        assert!(!maybe_toast(true, false, None, now));
        // inside the gap: no; at/past it: yes
        assert!(!maybe_toast(true, true, Some(now - Duration::from_secs(1)), now));
        assert!(maybe_toast(true, true, Some(now - TOAST_GAP), now));
        assert!(maybe_toast(true, true, Some(now - Duration::from_secs(10)), now));
        // a `last` in the future reads as "too soon", never panics
        assert!(!maybe_toast(true, true, Some(now), now - Duration::from_secs(1)));
    }

    #[test]
    fn mark_read_clears_the_dot() {
        let f = Feed::default();
        f.push("one", NoteKind::Warn);
        f.push("two", NoteKind::Update);
        assert_eq!(f.unread(), 2);
        f.mark_read();
        assert_eq!(f.unread(), 0);
        // the notes themselves stay
        assert_eq!(f.notes().len(), 2);
        // a later note starts the count again
        f.push("three", NoteKind::Info);
        assert_eq!(f.unread(), 1);
    }

    #[test]
    fn ago_uses_human_buckets() {
        let now = Instant::now();
        assert_eq!(ago(now - Duration::from_secs(10)), "just now");
        assert_eq!(ago(now - Duration::from_secs(5 * 60)), "5m ago");
        assert_eq!(ago(now - Duration::from_secs(45)), "1m ago");
        assert_eq!(ago(now - Duration::from_secs(3 * 3600)), "3h ago");
        assert_eq!(ago(now - Duration::from_secs(2 * 86_400)), "2d ago");
    }
}
