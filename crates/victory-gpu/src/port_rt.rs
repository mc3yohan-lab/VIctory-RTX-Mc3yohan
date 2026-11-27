//! Renderer port RT: ray tracing's own costs (`VICTORY_PORT_RT=1`, off by
//! default; read in [`on`]). Ported from avatarmanz/VIctory-7900XTX (`84917c34`).
//!
//! Part 1, the lane's wait for the card: a constant buffer in upload memory
//! read on the CPU at submission (`Step::Fill`) collided with a copy at its
//! place (`place`) over the same bytes, causing `card_steps::plan` to refuse
//! the whole submission (`BEHIND_PLACED`) and stall 15-60 ms waiting for
//! flights to land.
//!
//! With the switch that copy is only in the way when it stands ahead of a
//! step that the card does in front of the lists and that reads or writes its
//! bytes (`VICTORY_PLACED_ORDER=1`), cutting the submission behind the copy's
//! list (`card_steps::cut_for`) when needed, and forwarding same-list fills
//! behind a placed copy directly to the copy's source (`HostList::forward_fill`).

use std::sync::atomic::{AtomicU64, Ordering::Relaxed};

/// Whether the port is on. A/B-able as `VICTORY_AB=<s>,VICTORY_PORT_RT` in
/// diagnostics builds; on in tests for the threads that ask for it.
pub(crate) fn on() -> bool {
    #[cfg(feature = "diagnostics")]
    if let Some(on) = crate::ab::pick("VICTORY_PORT_RT") {
        return on;
    }
    #[cfg(test)]
    if PORT_RT_IN_TEST.get() {
        return true;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var("VICTORY_PORT_RT").is_ok_and(|v| v.trim() == "1"))
}

#[cfg(test)]
thread_local! {
    /// [`on`], for what this thread does.
    pub(crate) static PORT_RT_IN_TEST: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// Since the last line: submissions whose copies at their place stood in the
/// card's way only by the old rule (the order lets them through); cut behind
/// such a copy; left to wait, as the step in its way is in the copy's own list;
/// submissions refused for it with the switch off; forwarded fills.
static COUNTS: [AtomicU64; 5] = [const { AtomicU64::new(0) }; 5];
pub(crate) const BY_ORDER: usize = 0;
pub(crate) const CUT: usize = 1;
pub(crate) const SAME_LIST: usize = 2;
pub(crate) const REFUSED_OFF: usize = 3;
/// Fills that take their bytes out of the source of a copy at its place ahead
/// of them in their list (`HostList::forward_fill`).
pub(crate) const FORWARDED: usize = 4;

pub(crate) fn count(what: usize) {
    COUNTS[what].fetch_add(1, Relaxed);
}

/// What a step of a list does, as [`forwarded`] sees it.
#[derive(Clone, Copy, Debug)]
pub(crate) enum Seen {
    /// A copy at its place: destination, source, bytes, and whether the card
    /// copies what the CPU takes out of the source at submission.
    Placed { dst: usize, src: usize, len: usize, from_upload: bool },
    /// It writes title memory `[from, to)`, or declares a shader write there.
    Writes(usize, usize),
    /// It may write anywhere.
    Anything,
    /// It writes no title memory.
    Nothing,
}

/// Where a fill of `[src, src + len)` recorded now takes its bytes, given the
/// list's steps so far, last first; `None`: out of `src` itself.
pub(crate) fn forwarded(steps: impl Iterator<Item = Seen>, src: usize, len: usize) -> Option<usize> {
    let to = src.checked_add(len)?;
    if len == 0 {
        return None;
    }
    let meet = |a: usize, b: usize, c: usize, d: usize| a < d && c < b;
    let mut later: Vec<(usize, usize)> = Vec::new();
    for step in steps {
        let (from, until) = match step {
            Seen::Placed { dst, src: from, len: n, from_upload } => {
                if meet(dst, dst + n, src, to) {
                    let holds = dst <= src && to <= dst + n;
                    let moved = later.iter().any(|&(a, b)| meet(a, b, from, from + n));
                    let apart = !meet(from, from + n, dst, dst + n);
                    return (holds && !moved && apart && from_upload).then(|| from + (src - dst));
                }
                (dst, dst + n)
            }
            Seen::Writes(a, b) => (a, b),
            Seen::Anything => return None,
            Seen::Nothing => continue,
        };
        if meet(from, until, src, to) {
            return None;
        }
        later.push((from, until));
    }
    None
}

/// The port's line of the frame lines, per frame: printed with the switch on
/// or with something counted.
pub(crate) fn line(frames: f64) {
    let n: Vec<f64> = COUNTS.iter().map(|c| c.swap(0, Relaxed) as f64 / frames).collect();
    if on() || n.iter().any(|&v| v > 0.0) {
        tracing::info!(
            "renderer port RT (VICTORY_PORT_RT={}) per frame: submissions with a copy at its place over what the card brings: \
             {:.2} let through by the order, {:.2} cut behind the copy, {:.2} left to wait (the step in the copy's own list); \
             {:.2} refused with the switch off; {:.1} fills that take a copy's bytes out of its source",
            u8::from(on()),
            n[BY_ORDER],
            n[CUT],
            n[SAME_LIST],
            n[REFUSED_OFF],
            n[FORWARDED],
        );
    }
}

#[cfg(test)]
mod tests {
    use super::{Seen, forwarded};

    fn placed(dst: usize, src: usize, len: usize) -> Seen {
        Seen::Placed { dst, src, len, from_upload: true }
    }

    fn walk(steps: &[Seen], src: usize, len: usize) -> Option<usize> {
        forwarded(steps.iter().rev().copied(), src, len)
    }

    #[test]
    fn a_fill_behind_a_copy_that_holds_it_reads_the_copys_source() {
        assert_eq!(walk(&[placed(0x1000, 0x8000, 0xb0)], 0x1000, 0xb0), Some(0x8000));
        assert_eq!(walk(&[placed(0x1000, 0x8000, 0x100)], 0x1040, 0x20), Some(0x8040));
        let between = [placed(0x1000, 0x8000, 0xb0), Seen::Nothing, Seen::Writes(0x9000, 0x9100), placed(0x3000, 0x4000, 16)];
        assert_eq!(walk(&between, 0x1000, 0xb0), Some(0x8000));
    }

    #[test]
    fn anything_else_reads_where_it_did() {
        assert_eq!(walk(&[placed(0x1000, 0x8000, 0xb0)], 0x1080, 0x40), None);
        assert_eq!(walk(&[], 0x1000, 0xb0), None);
        assert_eq!(walk(&[placed(0x2000, 0x8000, 0xb0)], 0x1000, 0xb0), None);
        assert_eq!(walk(&[Seen::Placed { dst: 0x1000, src: 0x8000, len: 0xb0, from_upload: false }], 0x1000, 0xb0), None);
        assert_eq!(walk(&[placed(0x1000, 0x1040, 0x100)], 0x1000, 0x40), None);
        for after in [Seen::Writes(0x10a0, 0x10b0), Seen::Writes(0x8010, 0x8020), Seen::Anything] {
            assert_eq!(walk(&[placed(0x1000, 0x8000, 0xb0), after], 0x1000, 0xb0), None, "{after:?}");
        }
        assert_eq!(walk(&[placed(0x1000, 0x8000, 0xb0), placed(0x1000, 0x9000, 0x10)], 0x1000, 0xb0), None);
        assert_eq!(walk(&[placed(0x1000, 0x9000, 0x10), placed(0x1000, 0x8000, 0xb0)], 0x1000, 0xb0), Some(0x8000));
        assert_eq!(walk(&[placed(0x1000, 0x8000, 0xb0)], 0x1000, 0), None);
        assert_eq!(walk(&[placed(0x1000, 0x8000, 0xb0)], usize::MAX, 2), None);
    }

    #[test]
    fn the_switch_is_off_unless_asked_for() {
        assert!(!super::PORT_RT_IN_TEST.get());
        super::PORT_RT_IN_TEST.set(true);
        assert!(super::on());
        super::PORT_RT_IN_TEST.set(false);
    }
}
