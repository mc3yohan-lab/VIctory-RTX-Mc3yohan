//! Retention limits for reusable CPU buffers on hosts with at most 16 GiB.
//! Live render data is never evicted by these helpers.

use std::sync::OnceLock;

const MIB: usize = 1 << 20;
const GIB: u64 = 1 << 30;

fn selected(setting: Option<&str>, physical: Option<u64>) -> bool {
    match setting.map(str::trim) {
        Some("0") => false,
        Some("1") => true,
        _ => physical.is_some_and(|bytes| bytes > 0 && bytes <= 16 * GIB),
    }
}

/// `VICTORY_RAM_16GB=0/1` overrides detection, including for A/B comparisons.
pub(crate) fn enabled() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| {
        use windows_sys::Win32::System::SystemInformation::{GlobalMemoryStatusEx, MEMORYSTATUSEX};
        // SAFETY: a sized structure initialized by the system.
        let mut memory: MEMORYSTATUSEX = unsafe { std::mem::zeroed() };
        memory.dwLength = std::mem::size_of::<MEMORYSTATUSEX>() as u32;
        // SAFETY: the structure has the size the API expects.
        let physical =
            (unsafe { GlobalMemoryStatusEx(&mut memory) } != 0).then_some(memory.ullTotalPhys);
        let setting = std::env::var("VICTORY_RAM_16GB").ok();
        let on = selected(setting.as_deref(), physical);
        tracing::info!(
            enabled = on,
            physical_mib = physical.map(|n| n >> 20),
            "16 GiB host RAM retention profile"
        );
        on
    })
}

pub(crate) fn snapshot_budget() -> usize {
    if crate::residency::pressured() {
        0
    } else {
        64 * MIB
    }
}

pub(crate) fn read_cache_budget() -> Option<usize> {
    enabled().then(|| {
        if crate::residency::pressured() {
            0
        } else {
            16 * MIB
        }
    })
}

/// Keep the largest buffers that actually fit the byte budget, preserving
/// descending capacity order for binary search (`shadow::pool_take`). Return
/// the surplus so its owner can release it after unlocking the shared pool.
pub(crate) fn trim_pool(pool: &mut Vec<Vec<u8>>, count: usize, budget: usize) -> Vec<Vec<u8>> {
    pool.sort_unstable_by_key(|buffer| std::cmp::Reverse(buffer.capacity()));
    let previous = std::mem::take(pool);
    let mut gone = Vec::new();
    let mut used = 0usize;
    for buffer in previous {
        let bytes = buffer.capacity();
        if pool.len() < count && bytes <= budget.saturating_sub(used) {
            used += bytes;
            pool.push(buffer);
        } else {
            gone.push(buffer);
        }
    }
    gone
}

/// The callback finishes before a large temporary read buffer is released.
/// Its result owns its data; no reference to the cache can escape in `R`.
pub(crate) fn with_read_cache<R>(
    cache: &mut Vec<u8>,
    size: usize,
    budget: Option<usize>,
    read: impl FnOnce(&mut [u8]) -> R,
) -> R {
    if cache.len() < size {
        cache.resize(size, 0);
    }
    let result = read(&mut cache[..size]);
    if budget.is_some_and(|limit| cache.capacity() > limit) {
        *cache = Vec::new();
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn detection_and_explicit_comparison_overrides() {
        assert!(selected(None, Some(16 * GIB)));
        assert!(!selected(None, Some(16 * GIB + 1)));
        assert!(!selected(None, None));
        assert!(!selected(None, Some(0)));
        assert!(!selected(Some("0"), Some(8 * GIB)));
        assert!(selected(Some("1"), Some(64 * GIB)));
    }

    #[test]
    fn an_oversized_snapshot_does_not_displace_reusable_small_buffers() {
        let mut pool = vec![vec![3; 12 * MIB], vec![7; 8 * MIB], vec![9; 2 * MIB]];
        let gone = trim_pool(&mut pool, 8, 10 * MIB);
        assert_eq!(pool.iter().map(Vec::capacity).sum::<usize>(), 10 * MIB);
        assert_eq!(pool.iter().map(|v| v[0]).collect::<Vec<_>>(), [7, 9]);
        assert!(pool.windows(2).all(|w| w[0].capacity() >= w[1].capacity()));
        assert_eq!(gone.len(), 1);
        assert_eq!(gone[0][12 * MIB - 1], 3);
        let gone = trim_pool(&mut pool, 8, 0);
        assert!(pool.is_empty());
        assert_eq!(gone.len(), 2);
    }

    #[test]
    fn temporary_reads_preserve_results_and_release_only_after_the_callback() {
        let mut cache = Vec::new();
        let bytes = with_read_cache(&mut cache, 2 * MIB, Some(MIB), |bytes| {
            bytes.fill(0x5a);
            bytes[bytes.len() - 16..].to_vec()
        });
        assert_eq!(bytes, [0x5a; 16]);
        assert_eq!(cache.capacity(), 0);
        with_read_cache(&mut cache, 4096, Some(MIB), |bytes| bytes[0] = 42);
        let address = cache.as_ptr();
        assert_eq!(
            with_read_cache(&mut cache, 64, Some(MIB), |bytes| bytes[0]),
            42
        );
        assert_eq!(cache.as_ptr(), address);
        with_read_cache(&mut cache, 4 * MIB, None, |_| ());
        assert!(cache.capacity() >= 4 * MIB);
    }
}
