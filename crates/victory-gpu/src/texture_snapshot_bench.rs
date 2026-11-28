//! CPU only. Run `cargo test -p victory-gpu --profile diag --lib bc7_mapping_wait -- --ignored --nocapture --test-threads=1`.

use super::{Digest, digest, refresh_direct_snapshot, sample_snapshot};
use crate::watch::{Reading, title_memory_reading, title_memory_remapping};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use victory_formats::texture::{Desc, Dimension, Layout, Swizzle};

/// The writer and reader start together with the read lock already held.
/// Thread startup and join are excluded from both intervals.
fn contention(read: impl FnOnce(Reading)) -> (Duration, Duration) {
    std::thread::scope(|scope| {
        let mapped = title_memory_reading();
        let (ready, start) = mpsc::sync_channel(1);
        let writer = scope.spawn(move || {
            ready.send(()).unwrap();
            let started = Instant::now();
            let _changed = title_memory_remapping();
            started.elapsed()
        });
        start.recv().unwrap();
        let began = Instant::now();
        read(mapped);
        let total = began.elapsed();
        (writer.join().unwrap(), total)
    })
}

fn median_ms(mut values: Vec<Duration>) -> f64 {
    values.sort_unstable();
    values[values.len() / 2].as_secs_f64() * 1000.0
}

#[test]
#[ignore = "synthetic CPU timing; run separately on an otherwise idle machine"]
fn bc7_mapping_wait() {
    for pixels in [2048, 4096] {
        let layout = Layout::new(&Desc {
            dimension: Dimension::Tex2d,
            width: pixels,
            height: pixels,
            depth: 1,
            mips: 1,
            element_bytes: 16,
            block: 4,
            samples: 1,
            swizzle: Swizzle::SW_64KB_D,
        })
        .unwrap();
        let size = layout.size as usize;
        let row = (pixels as usize / 4) * 16;
        let mut current: Vec<u8> = (0..size)
            .map(|i| (i ^ (i >> 9) ^ (i >> 17)) as u8)
            .collect();
        let mut output = vec![0; size];
        layout
            .detile(&current, 0, 0, &mut output, row, size)
            .unwrap();
        let expected = digest(&output);
        let expected_bytes = output.clone();
        for (kind, mutation) in [
            ("sampled", Some(0)),
            ("unsampled", Some(129)),
            ("unchanged", None),
        ] {
            if let Some(i) = mutation {
                current[i] ^= 0x80;
            }
            let previous = digest(&current);
            let sample = sample_snapshot(&current);
            if let Some(i) = mutation {
                current[i] ^= 0x80;
            }
            let mut results: [Vec<(Duration, Duration)>; 2] = std::array::from_fn(|_| Vec::new());
            // Two warm-up pairs followed by nine measured pairs. Alternate the order.
            for pair in 0..11 {
                for route in [pair % 2, (pair + 1) % 2] {
                    if mutation.is_some() {
                        output.fill(0xab);
                    }
                    let mut uploaded = false;
                    let mut convert = |bytes: &[u8], checked: Option<Digest>| {
                        if checked.unwrap_or_else(|| digest(bytes)) != previous {
                            layout.detile(bytes, 0, 0, &mut output, row, size).unwrap();
                            uploaded = true;
                        }
                        size
                    };
                    let times = contention(|mapped| {
                        if route == 0 {
                            convert(&current, None);
                            drop(mapped);
                        } else {
                            // SAFETY: `current` is a fully allocated, immutable source for the entire call.
                            unsafe {
                                refresh_direct_snapshot(
                                    mapped,
                                    current.as_ptr(),
                                    size,
                                    Some(previous),
                                    Some(sample),
                                    convert,
                                );
                            }
                        }
                    });
                    assert_eq!(uploaded, mutation.is_some());
                    assert!(output == expected_bytes, "BC7 output changed");
                    if pair >= 2 {
                        results[route].push(times);
                    }
                }
            }
            for (name, values) in ["in_place", "snapshot"].into_iter().zip(results) {
                let (waits, totals) = values.into_iter().unzip();
                println!(
                    "BC7 {pixels} {kind} {name} n=9 wait_ms={:.3} total_ms={:.3} output={:032x}",
                    median_ms(waits),
                    median_ms(totals),
                    expected.0
                );
            }
        }
    }
}
