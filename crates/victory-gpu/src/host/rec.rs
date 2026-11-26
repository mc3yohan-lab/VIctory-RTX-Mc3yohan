//! A host list's D3D12 list, with barrier calls joined
//! (`VICTORY_JOIN_BARRIERS=1`; off by default; A/B-able with
//! `VICTORY_AB=<s>,VICTORY_JOIN_BARRIERS`).
//!
//! The state changes the host asks for (`crate::list::ensure_state`, the
//! aliasing before a texture's first upload) and the title's own barriers
//! wait here instead of going to the list at once; they go, in the order
//! they were asked for, in one call, at the next use of the list for
//! anything else. Every use goes through [`Rec`]'s `Deref`, so nothing is
//! recorded before a barrier asked for earlier. In a replay of capture
//! replay5 joining the calls that had no work between them saved 2.4 ms of
//! 79.5 (2,490 calls joined, 1,619 of them a texture's state change after
//! its upload followed by the next texture's aliasing).

use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering::Relaxed};

use windows::Win32::Graphics::Direct3D12::*;

/// Whether barriers are held now (`VICTORY_JOIN_BARRIERS=1`, or its phase
/// of `VICTORY_AB`).
pub(crate) fn on() -> bool {
    #[cfg(feature = "diagnostics")]
    if let Some(on) = crate::ab::pick("VICTORY_JOIN_BARRIERS") {
        return on;
    }
    static ON: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_JOIN_BARRIERS").is_some_and(|v| v == "1"))
}

/// Barriers held. Their resources are borrowed: the list's states hold the
/// textures (`HostList::transition`), and the title's barriers name what
/// the title keeps alive until the list has run.
struct Held(Vec<D3D12_RESOURCE_BARRIER>);

// SAFETY: borrowed interface pointers of free-threaded objects.
unsafe impl Send for Held {}

/// Since the last frame-rate line: barrier calls asked for while held,
/// calls made of them, and redundant global UAV barriers skipped.
pub(crate) static STATS: [std::sync::atomic::AtomicU64; 3] = [const { std::sync::atomic::AtomicU64::new(0) }; 3];

pub(crate) struct Rec {
    list: ID3D12GraphicsCommandList,
    any: AtomicBool,
    /// Whether any shader/helper command on this list (or an earlier list on
    /// the queue before reset) may have written a UAV since the last global
    /// `UAV(NULL)` barrier. Starts `true` on creation and `reset` so cross-list
    /// UAV writes are always ordered safely.
    uav_dirty: AtomicBool,
    held: Mutex<Held>,
}

impl Rec {
    pub(crate) fn new(list: ID3D12GraphicsCommandList) -> Self {
        Self {
            list,
            any: AtomicBool::new(false),
            uav_dirty: AtomicBool::new(true),
            held: Mutex::new(Held(Vec::new())),
        }
    }

    /// Marks that a GPU command on this list may have written a UAV (buffer or
    /// texture image store/atomic).
    #[inline]
    pub(crate) fn mark_uav_dirty(&self) {
        if !self.uav_dirty.load(Relaxed) {
            self.uav_dirty.store(true, Relaxed);
        }
    }

    /// Resets UAV dirty tracking on command-list reset: starts `true` so a
    /// preceding list's unsynced UAV writes are still ordered by this list's
    /// first barrier or indirect clamp.
    #[inline]
    pub(crate) fn reset_uav_dirty(&self) {
        self.uav_dirty.store(true, Relaxed);
    }

    /// Whether a shader/helper may have written an unsynced UAV since the last
    /// global `UAV(NULL)` barrier on this list.
    #[inline]
    pub(crate) fn uav_dirty(&self) -> bool {
        self.uav_dirty.load(Relaxed)
    }

    /// Records `barriers` now, or holds them for the next use of the list.
    /// Redundant global `UAV(NULL)` barriers when `!uav_dirty` (or already
    /// queued in `held`) are elided to avoid full-GPU SM flushes.
    ///
    /// # Safety
    /// As `ResourceBarrier`'s: an open list, live resources.
    pub(crate) unsafe fn barriers(&self, barriers: &[D3D12_RESOURCE_BARRIER]) {
        if barriers.is_empty() {
            return;
        }
        if !on() {
            // (What was held while it was on goes first.)
            if self.any.load(Relaxed) {
                self.flush();
            }
            if barriers.iter().any(is_global_uav) {
                let mut filtered = Vec::with_capacity(barriers.len());
                for b in barriers {
                    if is_global_uav(b) {
                        if self.uav_dirty.swap(false, Relaxed) {
                            // SAFETY: plain copy of borrowed descriptor.
                            filtered.push(unsafe { std::ptr::read(b) });
                        } else {
                            STATS[2].fetch_add(1, Relaxed);
                        }
                    } else {
                        // SAFETY: plain copy of borrowed descriptor.
                        filtered.push(unsafe { std::ptr::read(b) });
                    }
                }
                if !filtered.is_empty() {
                    // SAFETY: the caller's.
                    unsafe { self.list.ResourceBarrier(&filtered) };
                }
                return;
            }
            // SAFETY: the caller's.
            unsafe { self.list.ResourceBarrier(barriers) };
            return;
        }
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        let mut pushed = false;
        let mut flushed_for_conflict = false;
        // A resource whose state changes again: what is held goes first, so
        // that no call holds two changes of one resource.
        if barriers.iter().filter_map(named).any(|r| held.0.iter().filter_map(named).any(|h| h == r)) {
            // SAFETY: as in `flush`.
            unsafe { self.list.ResourceBarrier(&held.0) };
            held.0.clear();
            STATS[1].fetch_add(1, Relaxed);
            flushed_for_conflict = true;
        }
        let _ = flushed_for_conflict;
        for b in barriers {
            if is_global_uav(b) {
                if !self.uav_dirty.load(Relaxed) || held.0.iter().any(is_global_uav) {
                    STATS[2].fetch_add(1, Relaxed);
                    continue;
                }
                self.uav_dirty.store(false, Relaxed);
            }
            // SAFETY: a plain copy of the descriptions; nothing is released
            // through them (their pointers are borrowed, `ManuallyDrop`).
            held.0.push(unsafe { std::ptr::read(b) });
            pushed = true;
        }
        if pushed {
            STATS[0].fetch_add(1, Relaxed);
            self.any.store(true, Relaxed);
        }
    }

    /// What is held goes to the list, in one call.
    fn flush(&self) {
        let mut held = self.held.lock().unwrap_or_else(|e| e.into_inner());
        self.any.store(false, Relaxed);
        if held.0.is_empty() {
            return;
        }
        // SAFETY: the barriers were asked for on this open list, in order;
        // their resources are alive (`Held`).
        unsafe { self.list.ResourceBarrier(&held.0) };
        held.0.clear();
        STATS[1].fetch_add(1, Relaxed);
    }
}

/// Whether `b` is a global UAV barrier (`pResource == NULL`) waiting for every
/// earlier shader write across the entire GPU.
fn is_global_uav(b: &D3D12_RESOURCE_BARRIER) -> bool {
    // SAFETY: `UAV` union field is valid when `Type == UAV`.
    b.Type == D3D12_RESOURCE_BARRIER_TYPE_UAV && unsafe { b.Anonymous.UAV.pResource.is_none() }
}

/// The resource a state change or aliasing names (its interface pointer).
fn named(b: &D3D12_RESOURCE_BARRIER) -> Option<usize> {
    // SAFETY: the union's member is the one `Type` says; a pointer read.
    unsafe {
        let p: Option<&ID3D12Resource> = match b.Type {
            D3D12_RESOURCE_BARRIER_TYPE_TRANSITION => b.Anonymous.Transition.pResource.as_ref(),
            D3D12_RESOURCE_BARRIER_TYPE_ALIASING => b.Anonymous.Aliasing.pResourceAfter.as_ref(),
            _ => None,
        };
        p.map(|r| windows::core::Interface::as_raw(r) as usize)
    }
}

impl std::ops::Deref for Rec {
    type Target = ID3D12GraphicsCommandList;

    /// The list, with what is held recorded first.
    fn deref(&self) -> &ID3D12GraphicsCommandList {
        if self.any.load(Relaxed) {
            self.flush();
        }
        &self.list
    }
}

/// For the frame-rate line.
pub(crate) fn line(frames: f64) {
    let [asked, made, skipped] = [0, 1, 2].map(|i| STATS[i].swap(0, Relaxed) as f64 / frames);
    if asked > 0.0 || skipped > 0.0 {
        tracing::info!(
            "barrier calls per frame (VICTORY_JOIN_BARRIERS): {asked:.0} asked for, {made:.0} made of them ({skipped:.0} redundant global UAV waits skipped)"
        );
    }
}
