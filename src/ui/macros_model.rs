//! Pure model of the macro editor (no egui): kind switching, items, selection edits, save rules.

use crate::catalog::vk_name;
use crate::profile::*;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Stage {
    Name,
    Type,
    Edit,
}

/// (section, lo, hi) steps of a selected item.
type Span = (usize, usize, usize);

/// One displayed item: step `idx` (or the delay before it) of section `sec`.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct ItemId {
    pub sec: usize,
    pub idx: usize,
    pub delay: bool,
}

#[derive(Clone, PartialEq, Debug)]
pub enum ItemKind {
    Key { vk: u16, down: bool },
    Mouse { vk: u16, down: bool },
    Text(String),
    Sys(String),
    Obs(String),
    Launch(String),
    Fold(String),
    Delay { ms: u32, pause: bool },
}

/// Steps `id.idx..hi` of the section's list.
#[derive(Clone, PartialEq, Debug)]
pub struct Item {
    pub id: ItemId,
    pub hi: usize,
    pub kind: ItemKind,
}

#[derive(Clone, PartialEq, Debug)]
pub struct Snap {
    pub name: String,
    pub kind: MacroKind,
    pub lists: [Vec<MacroStep>; 3],
    pub std: bool,
    pub std_ms: u32,
    pub show_up_down: bool,
    pub color: Option<[u8; 3]>,
}

pub fn is_mouse_vk(vk: u16) -> bool {
    matches!(vk, 0x01 | 0x02 | 0x04 | 0x05 | 0x06)
}

/// Value and unit of a delay item.
pub fn fmt_delay(ms: u32) -> (String, &'static str) {
    if ms < 1000 {
        (ms.to_string(), "ms")
    } else if ms < 60_000 {
        (format!("{:.1}", ms as f32 / 1000.0), "s")
    } else {
        (format!("{:.1}", ms as f32 / 60_000.0), "min")
    }
}

/// Items of one list for the current display mode.
pub fn items_of(list: &[MacroStep], sec: usize, std: bool, show_up_down: bool) -> Vec<Item> {
    let mut out = Vec::new();
    let fold = std && !show_up_down;
    let mut i = 0;
    while i < list.len() {
        let s = &list[i];
        if s.is_pause() {
            if !std {
                out.push(Item { id: ItemId { sec, idx: i, delay: true }, hi: i + 1, kind: ItemKind::Delay { ms: s.delay_ms, pause: true } });
            }
            i += 1;
            continue;
        }
        if !std && s.delay_ms > 0 {
            out.push(Item { id: ItemId { sec, idx: i, delay: true }, hi: i + 1, kind: ItemKind::Delay { ms: s.delay_ms, pause: false } });
        }
        let id = ItemId { sec, idx: i, delay: false };
        if fold && let Some(end) = fold_end(list, i) {
            let mut order: Vec<u16> = Vec::new();
            for st in &list[i..end] {
                if st.down && !order.contains(&st.vk) {
                    order.push(st.vk);
                }
            }
            let label = order.iter().map(|&v| vk_name(v)).map(|n| n.trim_start_matches("Left ").trim_start_matches("Right ").to_string()).collect::<Vec<_>>().join(" + ");
            out.push(Item { id, hi: end, kind: ItemKind::Fold(label) });
            i = end;
            continue;
        }
        let kind = match &s.act {
            Some(StepAct::Text(t)) => ItemKind::Text(t.clone()),
            Some(StepAct::Do(Assignment::Launch { name, .. })) => ItemKind::Launch(format!("Launch {name}")),
            Some(StepAct::Do(Assignment::Obs(_))) => ItemKind::Obs(crate::catalog::step_name(s)),
            Some(StepAct::Do(_)) => ItemKind::Sys(crate::catalog::step_name(s)),
            None if is_mouse_vk(s.vk) => ItemKind::Mouse { vk: s.vk, down: s.down },
            None => ItemKind::Key { vk: s.vk, down: s.down },
        };
        out.push(Item { id, hi: i + 1, kind });
        i += 1;
    }
    out
}

/// End (exclusive) of a foldable run starting at `i`: keyboard keys only, starts with the down of K,
/// ends with the up of K, and every key pressed in between is released by then.
fn fold_end(list: &[MacroStep], i: usize) -> Option<usize> {
    let first = &list[i];
    if first.act.is_some() || first.vk == 0 || is_mouse_vk(first.vk) || !first.down {
        return None;
    }
    let mut held: Vec<u16> = Vec::new();
    for (j, s) in list.iter().enumerate().skip(i) {
        if s.act.is_some() || s.vk == 0 || is_mouse_vk(s.vk) {
            return None;
        }
        if s.down {
            if !held.contains(&s.vk) {
                held.push(s.vk);
            }
        } else if held.contains(&s.vk) {
            held.retain(|&v| v != s.vk);
        } else {
            return None;
        }
        if held.is_empty() {
            return (!s.down && s.vk == first.vk).then_some(j + 1);
        }
    }
    None
}

pub struct Model {
    pub name: String,
    pub kind: MacroKind,
    pub lists: [Vec<MacroStep>; 3],
    pub std: bool,
    pub std_ms: u32,
    pub show_up_down: bool,
    pub color: Option<[u8; 3]>,
    pub is_new: bool,
    pub snap: Snap,
    pub active: usize,
    pub sel: Vec<ItemId>,
    pub clip: Vec<MacroStep>,
}

impl Model {
    pub fn fresh(name: &str, kind: MacroKind) -> Self {
        let snap = Snap { name: name.into(), kind, lists: Default::default(), std: true, std_ms: 50, show_up_down: true, color: None };
        Self::from_snap(snap, true)
    }

    pub fn open(m: &Macro) -> Self {
        let ms = m.standard_delay();
        let snap = Snap {
            name: m.name.clone(),
            kind: m.kind,
            lists: [m.steps.clone(), m.hold.clone(), m.release.clone()],
            std: ms.is_some(),
            std_ms: ms.unwrap_or(50),
            show_up_down: m.show_up_down,
            color: m.color,
        };
        Self::from_snap(snap, false)
    }

    fn from_snap(snap: Snap, is_new: bool) -> Self {
        Self {
            name: snap.name.clone(),
            kind: snap.kind,
            lists: snap.lists.clone(),
            std: snap.std,
            std_ms: snap.std_ms,
            show_up_down: snap.show_up_down,
            color: snap.color,
            is_new,
            snap,
            active: 0,
            sel: Vec::new(),
            clip: Vec::new(),
        }
    }

    pub fn cur(&self) -> Snap {
        Snap { name: self.name.clone(), kind: self.kind, lists: self.lists.clone(), std: self.std, std_ms: self.std_ms, show_up_down: self.show_up_down, color: self.color }
    }

    pub fn changed(&self) -> bool {
        self.cur() != self.snap
    }

    pub fn sections(&self) -> usize {
        if self.kind == MacroKind::Sequence { 3 } else { 1 }
    }

    pub fn items(&self, sec: usize) -> Vec<Item> {
        items_of(&self.lists[sec], sec, self.std, self.show_up_down || !self.std)
    }

    fn used_lists(&self) -> usize {
        self.sections()
    }

    pub fn can_save(&self) -> bool {
        (self.is_new || self.changed()) && !self.name.trim().is_empty() && self.lists[..self.used_lists()].iter().any(|l| !l.is_empty())
    }

    pub fn can_reset(&self) -> bool {
        if self.is_new { self.lists.iter().any(|l| !l.is_empty()) } else { self.changed() }
    }

    pub fn reset(&mut self) {
        if self.is_new {
            self.lists = Default::default();
        } else {
            let s = self.snap.clone();
            self.name = s.name;
            self.kind = s.kind;
            self.lists = s.lists;
            self.std = s.std;
            self.std_ms = s.std_ms;
            self.show_up_down = s.show_up_down;
            self.color = s.color;
        }
        self.sel.clear();
        self.active = 0;
    }

    /// G HUB's rule: the main list of the old kind moves to the main list of the new kind.
    pub fn switch_kind(&mut self, new: MacroKind) {
        let old = self.kind;
        if old == new {
            return;
        }
        let src = if old == MacroKind::Sequence && self.lists[0].is_empty() { 1 } else { 0 };
        let dst = if new == MacroKind::Sequence && old == MacroKind::RepeatWhileHolding { 1 } else { 0 };
        if src != dst && !self.lists[src].is_empty() {
            let moved = std::mem::take(&mut self.lists[src]);
            self.lists[dst] = moved;
        }
        self.kind = new;
        self.sel.clear();
        self.active = 0;
    }

    fn spans(&self) -> (Vec<Span>, Vec<(usize, usize)>) {
        let mut spans = Vec::new();
        let mut zero = Vec::new();
        for sec in 0..self.sections() {
            for it in self.items(sec) {
                if !self.sel.contains(&it.id) {
                    continue;
                }
                match it.kind {
                    ItemKind::Delay { pause: false, .. } => zero.push((sec, it.id.idx)),
                    _ => spans.push((sec, it.id.idx, it.hi)),
                }
            }
        }
        (spans, zero)
    }

    /// Delete the selection: actions go, a delay item sets the delay to 0, a pause goes.
    pub fn delete_selection(&mut self) {
        let (mut spans, zero) = self.spans();
        for (sec, i) in zero {
            self.lists[sec][i].delay_ms = 0;
        }
        spans.sort_by(|a, b| b.cmp(a));
        for (sec, lo, hi) in spans {
            self.lists[sec].drain(lo..hi);
        }
        self.sel.clear();
    }

    pub fn copy_selection(&mut self) {
        let (spans, _) = self.spans();
        self.clip = spans.iter().flat_map(|&(sec, lo, hi)| self.lists[sec][lo..hi].to_vec()).collect();
    }

    pub fn cut_selection(&mut self) {
        self.copy_selection();
        self.delete_selection();
    }

    pub fn paste(&mut self) {
        let sec = self.active.min(self.sections() - 1);
        let clip = self.clip.clone();
        self.lists[sec].extend(clip);
    }

    pub fn select_all(&mut self) {
        let sec = self.active.min(self.sections() - 1);
        self.sel = self.items(sec).iter().map(|i| i.id).collect();
    }

    /// Move the selected steps (in order) to the gap before step `gap` of section `to`.
    pub fn move_selection(&mut self, to: usize, gap: usize) {
        let (mut spans, _) = self.spans();
        let mut delay_spans: Vec<Span> = Vec::new();
        for id in &self.sel {
            if id.delay && !spans.iter().any(|s| s.0 == id.sec && s.1 == id.idx) {
                let hi = self.items(id.sec).iter().find(|i| i.id.idx == id.idx && !i.id.delay).map_or(id.idx + 1, |i| i.hi);
                delay_spans.push((id.sec, id.idx, hi));
            }
        }
        spans.extend(delay_spans);
        spans.sort();
        spans.dedup();
        if spans.is_empty() {
            return;
        }
        let moved: Vec<MacroStep> = spans.iter().flat_map(|&(sec, lo, hi)| self.lists[sec][lo..hi].to_vec()).collect();
        let mut gap = gap;
        for &(sec, lo, hi) in spans.iter().rev() {
            if sec == to && hi <= gap {
                gap -= hi - lo;
            }
            self.lists[sec].drain(lo..hi);
        }
        let at = gap.min(self.lists[to].len());
        for (k, s) in moved.into_iter().enumerate() {
            self.lists[to].insert(at + k, s);
        }
        self.sel.clear();
        self.active = to;
    }

    /// The macro as SAVE stores it.
    pub fn to_macro(&self, id: &str) -> Macro {
        let mut m = Macro { id: id.into(), name: self.name.trim().into(), kind: self.kind, show_up_down: self.show_up_down, color: self.color, ..Default::default() };
        m.steps = self.lists[0].clone();
        if self.kind == MacroKind::Sequence {
            m.hold = self.lists[1].clone();
            m.release = self.lists[2].clone();
        }
        if self.std {
            m.apply_standard_delay(self.std_ms);
        } else {
            for list in [&mut m.steps, &mut m.hold, &mut m.release] {
                let n = list.len();
                let mut pending = 0;
                let mut out = Vec::new();
                for (i, mut s) in std::mem::take(list).into_iter().enumerate() {
                    if s.is_pause() && i + 1 != n {
                        pending += s.delay_ms;
                        continue;
                    }
                    s.delay_ms += pending;
                    pending = 0;
                    out.push(s);
                }
                *list = out;
            }
        }
        m
    }
}

/// G HUB's warning about unbalanced key or button events over press, hold and release in order.
pub fn validate(m: &Macro) -> Option<&'static str> {
    let mut held: Vec<u16> = Vec::new();
    let mut up_first: Option<bool> = None;
    for s in m.steps.iter().chain(&m.hold).chain(&m.release) {
        if s.vk == 0 || s.act.is_some() {
            continue;
        }
        if s.down {
            if !held.contains(&s.vk) {
                held.push(s.vk);
            }
        } else if held.contains(&s.vk) {
            held.retain(|&v| v != s.vk);
        } else if up_first.is_none() {
            up_first = Some(is_mouse_vk(s.vk));
        }
    }
    match (up_first, held.first()) {
        (Some(false), _) => Some("This macro has a key up before a key down for the same key. Are you sure you want to save it?"),
        (Some(true), _) => Some("This macro has a mouse up before a mouse down for the same click. Are you sure you want to save it?"),
        (None, Some(&v)) if is_mouse_vk(v) => Some("WARNING! This macro has a mouse down without a mouse up for the same click. Are you sure you want to save it?"),
        (None, Some(_)) => Some("WARNING! This macro has a key down without a key up for the same key. Are you sure you want to save it?"),
        (None, None) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn k(vk: u16, down: bool, d: u32) -> MacroStep {
        MacroStep::key(vk, down, d)
    }

    fn model(kind: MacroKind, lists: [Vec<MacroStep>; 3]) -> Model {
        let mut m = Model::open(&Macro { id: "m".into(), name: "m".into(), kind, steps: lists[0].clone(), hold: lists[1].clone(), release: lists[2].clone(), ..Default::default() });
        m.std = true;
        m.snap.std = true;
        m
    }

    #[test]
    fn kind_switching_follows_the_rule() {
        use MacroKind::*;
        let steps = vec![k(1, true, 0)];
        let hold = vec![k(2, true, 0)];
        for from in [NoRepeat, RepeatWhileHolding, Toggle] {
            for to in [NoRepeat, RepeatWhileHolding, Toggle, Sequence] {
                let mut m = model(from, [steps.clone(), vec![], vec![]]);
                m.switch_kind(to);
                let expect = if to == Sequence && from == RepeatWhileHolding { 1 } else { 0 };
                assert_eq!(m.lists[expect], steps, "{from:?} -> {to:?}");
                if from == to {
                    continue;
                }
            }
        }
        let mut m = model(Sequence, [steps.clone(), hold.clone(), vec![]]);
        m.switch_kind(Toggle);
        assert_eq!(m.lists[0], steps);
        let mut m = model(Sequence, [vec![], hold.clone(), vec![]]);
        m.switch_kind(NoRepeat);
        assert_eq!(m.lists[0], hold);
        let mut m = model(Sequence, [vec![], vec![], vec![]]);
        m.switch_kind(Toggle);
        assert!(m.lists.iter().all(Vec::is_empty));
        let mut m = model(RepeatWhileHolding, [steps.clone(), vec![], vec![]]);
        m.switch_kind(Sequence);
        m.switch_kind(RepeatWhileHolding);
        assert_eq!(m.lists[0], steps);
        assert!(m.lists[1].is_empty());
    }

    fn labels(m: &Model) -> Vec<String> {
        m.items(0)
            .into_iter()
            .map(|i| match i.kind {
                ItemKind::Key { vk, down } => format!("{}{}", vk_name(vk), if down { "v" } else { "^" }),
                ItemKind::Fold(l) => format!("[{l}:{}]", i.hi - i.id.idx),
                ItemKind::Delay { ms, pause } => format!("d{ms}{}", if pause { "p" } else { "" }),
                ItemKind::Mouse { vk, down } => format!("m{vk}{}", if down { "v" } else { "^" }),
                _ => "?".into(),
            })
            .collect()
    }

    #[test]
    fn chips_for_standard_and_custom_delays() {
        let list = vec![k(0x41, true, 0), k(0x41, false, 50), MacroStep::pause(50)];
        let mut m = model(MacroKind::NoRepeat, [list.clone(), vec![], vec![]]);
        assert_eq!(labels(&m), ["Av", "A^"]);
        m.show_up_down = false;
        assert_eq!(labels(&m), ["[A:2]"]);
        let ctrl_c = vec![k(0xA2, true, 0), k(0x43, true, 50), k(0x43, false, 50), k(0xA2, false, 50)];
        let m2 = model(MacroKind::NoRepeat, [ctrl_c, vec![], vec![]]);
        let mut m2 = m2;
        m2.show_up_down = false;
        assert_eq!(labels(&m2), ["[Ctrl + C:4]"]);
        let mut m3 = model(MacroKind::NoRepeat, [vec![k(0x41, true, 0), k(0x42, true, 0), k(0x41, false, 0), k(0x42, false, 0), k(0x43, true, 0)], vec![], vec![]]);
        m3.show_up_down = false;
        assert_eq!(labels(&m3).len(), 5);
        let mut m4 = model(MacroKind::NoRepeat, [vec![k(0x01, true, 0), k(0x01, false, 0)], vec![], vec![]]);
        m4.show_up_down = false;
        assert_eq!(labels(&m4), ["m1v", "m1^"]);
        let mut m5 = model(MacroKind::NoRepeat, [vec![k(0x41, true, 0), k(0x41, false, 120), MacroStep::pause(250), k(0x42, true, 0)], vec![], vec![]]);
        m5.std = false;
        assert_eq!(labels(&m5), ["Av", "d120", "A^", "d250p", "Bv"]);
    }

    #[test]
    fn delete_actions_delays_pauses_and_folds() {
        let list = vec![k(0x41, true, 0), k(0x41, false, 120), MacroStep::pause(250), k(0x42, true, 0)];
        let mut m = model(MacroKind::NoRepeat, [list, vec![], vec![]]);
        m.std = false;
        m.sel = vec![ItemId { sec: 0, idx: 1, delay: true }];
        m.delete_selection();
        assert_eq!(m.lists[0][1].delay_ms, 0);
        assert_eq!(m.lists[0].len(), 4);
        m.sel = vec![ItemId { sec: 0, idx: 2, delay: true }];
        m.delete_selection();
        assert_eq!(m.lists[0].len(), 3);
        m.sel = vec![ItemId { sec: 0, idx: 0, delay: false }, ItemId { sec: 0, idx: 2, delay: false }];
        m.delete_selection();
        assert_eq!(m.lists[0], vec![k(0x41, false, 0)]);
        let mut f = model(MacroKind::NoRepeat, [vec![k(0x41, true, 0), k(0x41, false, 50), k(0x42, true, 50)], vec![], vec![]]);
        f.show_up_down = false;
        f.sel = vec![ItemId { sec: 0, idx: 0, delay: false }];
        f.delete_selection();
        assert_eq!(f.lists[0], vec![k(0x42, true, 50)]);
    }

    #[test]
    fn move_within_and_across_sections() {
        let l = |v: u16| vec![k(v, true, 0), k(v, false, 0)];
        let mut m = model(MacroKind::Sequence, [[l(1), l(2)].concat(), l(3), vec![]]);
        m.sel = vec![ItemId { sec: 0, idx: 0, delay: false }];
        m.move_selection(0, 4);
        assert_eq!(m.lists[0].iter().map(|s| s.vk).collect::<Vec<_>>(), vec![1, 2, 2, 1]);
        let mut m = model(MacroKind::Sequence, [[l(1), l(2)].concat(), l(3), vec![]]);
        m.sel = vec![ItemId { sec: 0, idx: 2, delay: false }];
        m.move_selection(0, 0);
        assert_eq!(m.lists[0][0].vk, 2);
        let mut m = model(MacroKind::Sequence, [[l(1), l(2)].concat(), l(3), vec![]]);
        m.sel = vec![ItemId { sec: 0, idx: 0, delay: false }, ItemId { sec: 0, idx: 3, delay: false }];
        m.move_selection(2, 0);
        assert_eq!(m.lists[2].iter().map(|s| (s.vk, s.down)).collect::<Vec<_>>(), [(1, true), (2, false)]);
        assert_eq!(m.lists[0].len(), 2);
    }

    #[test]
    fn copy_cut_paste_into_the_active_section() {
        let mut m = model(MacroKind::Sequence, [vec![k(1, true, 0), k(1, false, 5)], vec![], vec![]]);
        m.select_all();
        m.copy_selection();
        m.active = 2;
        m.paste();
        assert_eq!(m.lists[2].len(), 2);
        m.active = 0;
        m.select_all();
        m.cut_selection();
        assert!(m.lists[0].is_empty());
        m.paste();
        assert_eq!(m.lists[0].len(), 2);
    }

    #[test]
    fn save_normalises_lists_and_delays() {
        let mut m = model(MacroKind::NoRepeat, [vec![k(1, true, 0), k(1, false, 9)], vec![k(2, true, 0)], vec![k(3, true, 0)]]);
        m.std = true;
        m.std_ms = 30;
        let out = m.to_macro("x");
        assert!(out.hold.is_empty() && out.release.is_empty());
        assert_eq!(out.std_delay, Some(30));
        assert_eq!(out.steps[1].delay_ms, 30);
        let mut seq = model(MacroKind::Sequence, [vec![k(1, true, 0)], vec![k(2, true, 0), k(2, false, 7)], vec![]]);
        seq.std = true;
        seq.std_ms = 30;
        let out = seq.to_macro("x");
        assert!(out.hold.last().unwrap().is_pause());
        let mut c = model(MacroKind::NoRepeat, [vec![k(1, true, 0), MacroStep::pause(40), k(1, false, 10), MacroStep::pause(250)], vec![], vec![]]);
        c.std = false;
        let out = c.to_macro("x");
        assert_eq!(out.std_delay, None);
        assert_eq!(out.steps, vec![k(1, true, 0), k(1, false, 50), MacroStep::pause(250)]);
    }

    #[test]
    fn validation_warnings() {
        let mac = |steps: Vec<MacroStep>| Macro { steps, ..Default::default() };
        assert!(validate(&mac(vec![k(0x41, true, 0), k(0x41, false, 0)])).is_none());
        assert!(validate(&mac(vec![k(0x41, false, 0)])).unwrap().contains("key up before a key down"));
        assert!(validate(&mac(vec![k(0x41, true, 0)])).unwrap().contains("key down without a key up"));
        assert!(validate(&mac(vec![k(0x01, false, 0)])).unwrap().contains("mouse up before a mouse down"));
        assert!(validate(&mac(vec![k(0x01, true, 0)])).unwrap().contains("mouse down without a mouse up"));
        let seq = Macro { kind: MacroKind::Sequence, steps: vec![k(0x57, true, 0)], release: vec![k(0x57, false, 0)], ..Default::default() };
        assert!(validate(&seq).is_none());
    }

    #[test]
    fn save_and_reset_rules() {
        let mut n = Model::fresh("a", MacroKind::NoRepeat);
        assert!(!n.can_save() && !n.can_reset());
        n.lists[0].push(k(1, true, 0));
        assert!(n.can_save() && n.can_reset());
        n.name = "  ".into();
        assert!(!n.can_save());
        let mut o = model(MacroKind::NoRepeat, [vec![k(1, true, 0)], vec![], vec![]]);
        assert!(!o.can_save() && !o.can_reset());
        o.name = "other".into();
        assert!(o.can_save() && o.can_reset());
        o.reset();
        assert_eq!(o.name, "m");
        assert!(!o.changed());
    }
}
