//! Polls G HUB's `settings.db` mtime so the UI can nudge a re-import after G HUB
//! itself changed the settings. G HUB rewrites the file repeatedly while open,
//! so a change only counts once it has gone quiet for the debounce window.
//!
//! The app polls this every ~5 s from the UI frame (never in shot/script mode:
//! screenshots must stay deterministic); the first poll is only a baseline.

use std::time::{Duration, SystemTime};

/// The debounced state of `settings.db`'s modified time.
#[derive(Default)]
pub struct DbWatch {
    /// Last mtime the poll saw (`None` = file missing). `None` here means "not polled yet".
    seen: Option<Option<SystemTime>>,
    /// A change was seen but the file is still inside the debounce window.
    pending: bool,
}

impl DbWatch {
    /// `mtime` = `metadata(path).modified()` (None when the file is missing),
    /// `now` = `SystemTime::now()`. Returns true once when the file changed and
    /// then stayed untouched for `quiet`.
    pub fn poll(&mut self, mtime: Option<SystemTime>, now: SystemTime, quiet: Duration) -> bool {
        let settled = |t: Option<SystemTime>| t.is_some_and(|t| now.duration_since(t).map(|a| a >= quiet).unwrap_or(false));
        match self.seen {
            None => {
                // first poll is only the baseline
                self.seen = Some(mtime);
                false
            }
            Some(prev) if prev != mtime => {
                self.seen = Some(mtime);
                // already quiet for `quiet`: fire now; otherwise wait for the next polls
                let fire = settled(mtime);
                self.pending = mtime.is_some() && !fire;
                fire
            }
            _ => {
                let fire = self.pending && settled(mtime);
                if fire {
                    self.pending = false;
                }
                fire
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const QUIET: Duration = Duration::from_secs(2);

    fn at(secs: u64) -> SystemTime {
        SystemTime::UNIX_EPOCH + Duration::from_secs(secs)
    }

    #[test]
    fn first_poll_is_only_a_baseline() {
        let mut w = DbWatch::default();
        assert!(!w.poll(Some(at(100)), at(110), QUIET), "existing file is not a change");
        // same mtime again: still nothing
        assert!(!w.poll(Some(at(100)), at(115), QUIET));
    }

    #[test]
    fn settled_change_fires_once() {
        let mut w = DbWatch::default();
        w.poll(Some(at(100)), at(100), QUIET);
        // the file was rewritten 10 s ago and stayed quiet since: fire immediately
        assert!(w.poll(Some(at(90)), at(100), QUIET));
        // firing once: no repeat while the mtime stands
        assert!(!w.poll(Some(at(90)), at(105), QUIET));
        // the next settled change fires again
        assert!(w.poll(Some(at(110)), at(115), QUIET));
    }

    #[test]
    fn writes_debounce_until_quiet() {
        let mut w = DbWatch::default();
        w.poll(Some(at(100)), at(100), QUIET);
        // G HUB writes 1 s ago — inside the window, pending
        assert!(!w.poll(Some(at(110)), at(111), QUIET));
        // another write restarts the debounce
        assert!(!w.poll(Some(at(112)), at(113), QUIET));
        // writes stopped; the file is now older than 2 s: fire once
        assert!(w.poll(Some(at(112)), at(115), QUIET));
        assert!(!w.poll(Some(at(112)), at(116), QUIET));
    }

    #[test]
    fn missing_file_is_not_a_change() {
        let mut w = DbWatch::default();
        w.poll(Some(at(100)), at(100), QUIET);
        // file vanished: remembered, no note (nothing to re-import)
        assert!(!w.poll(None, at(105), QUIET));
        // file reappeared with an old timestamp = a settled change: fires
        assert!(w.poll(Some(at(90)), at(110), QUIET));
    }
}
