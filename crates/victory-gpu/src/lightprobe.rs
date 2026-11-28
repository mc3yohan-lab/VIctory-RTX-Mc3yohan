//! Diagnostics of the picture's light, frame by frame
//! (`VICTORY_LIGHT_PROBE=<file.csv>`): what the title's exposure programs
//! and the stand-in for the indirect light (`crate::ambient`) hold, written
//! by the card into a ring of records the processor reads a few frames
//! later, one line a frame.
//!
//! What the title does (programs by their names in the containers):
//! - `PS_CalculateExposure` (one 1x1 target a frame) moves the exposure
//!   toward a target by the frame's time (constant 0x3a4, 1/15 s at most) at
//!   0.65 of the difference a second, within a least and a greatest the
//!   title sets (both -5.5 in the two recorded frames: a fixed exposure).
//!   Read here: its inputs, the average luminance (1x1) and the exposure of
//!   the frame before (1x1: exposure, its log, the target).
//! - `ComputeLuxCompressionInfo` (one group, the end of the frame) turns the
//!   scene's luminance range (5x1, first texel) into the factor every
//!   lighting program of the next frame scales light by (16 bytes of title
//!   memory: factor, 1/factor, ...). Read here: that output and its input.
//! - The stand-in's slow mean of the light surfaces send out, and the mean
//!   luminance over sampled pixels of what the title's resolve left, what
//!   the stand-in found, and what was written.
//!
//! Columns: frame, ms since start, average luminance (4), exposure before
//! (4), lux factor output (4), lux input (4), sampled pixels, title's resolve
//! mean, stand-in mean, written mean, stand-in slow mean (rgb).

use std::io::Write as _;
use std::sync::atomic::{AtomicU64, Ordering::Relaxed};
use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D12::*;
use windows::core::s;

use crate::host::{HostList, host};

/// The title's programs looked at.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// `PS_CalculateExposure`.
    Exposure,
    /// `ComputeLuxCompressionInfo` (and its smooth form).
    Lux,
}

/// The lux program is looked at too (`VICTORY_LIGHT_PROBE_LUX=1`; off: run
/// light-21 lost the device in the pass after it, with a view of the frame
/// before bound as a raw buffer where the plain form has a texture).
fn lux_on() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_LIGHT_PROBE_LUX").is_some_and(|v| v == "1"))
}

pub(crate) fn role(container: &[u8]) -> Option<Role> {
    let has = |name: &[u8]| container.windows(name.len()).any(|w| w == name);
    if has(b"PS_CalculateExposure") {
        Some(Role::Exposure)
    } else if file_on() && lux_on() && has(b"ComputeLuxCompressionInfo") {
        Some(Role::Lux)
    } else {
        None
    }
}

/// The exposure program's inputs (root parameter, dwords into its table):
/// the average luminance and the exposure of the frame before.
pub(crate) const AVERAGE: (u8, u32) = (1, 8);
pub(crate) const BEFORE: (u8, u32) = (1, 16);
/// The lux program's input (5x1) and output (16 bytes).
pub(crate) const LUX_IN: (u8, u32) = (1, 8);
pub(crate) const LUX_OUT: (u8, u32) = (1, 24);

fn file() -> Option<&'static Mutex<std::io::BufWriter<std::fs::File>>> {
    static FILE: OnceLock<Option<Mutex<std::io::BufWriter<std::fs::File>>>> = OnceLock::new();
    FILE.get_or_init(|| {
        let path = std::env::var_os("VICTORY_LIGHT_PROBE").filter(|v| !v.is_empty() && v != "0")?;
        let mut f = std::io::BufWriter::new(std::fs::File::create(&path).ok()?);
        let _ = writeln!(
            f,
            "frame,ms,avg0,avg1,avg2,avg3,exp,logexp,target,exp3,lux,luxinv,lux2,lux3,in0,in1,in2,in3,samples,title,standin,written,mean_r,mean_g,mean_b"
        );
        tracing::info!(path = %std::path::Path::new(&path).display(), "light probe: a line a frame (VICTORY_LIGHT_PROBE)");
        Some(Mutex::new(f))
    })
    .as_ref()
}

/// Whether CSV light-probe logging (`VICTORY_LIGHT_PROBE`) is active.
pub(crate) fn file_on() -> bool {
    file().is_some()
}

/// Whether the probe hook is active (either CSV logging or lightweight
/// exposure tracking for `ambient::exposure_floor`).
pub(crate) fn on() -> bool {
    file_on() || crate::ambient::needs_exposure()
}

const RS_EXPOSURE: &str = "RootConstants(num32BitConstants=4, b0), DescriptorTable(SRV(t0)), DescriptorTable(SRV(t1)), UAV(u0), UAV(u1), UAV(u2)";
const RS_LUX: &str = "RootConstants(num32BitConstants=4, b0), DescriptorTable(SRV(t0)), DescriptorTable(UAV(u3)), UAV(u0)";

/// Dwords a record; records in the ring.
const RECORD: u64 = 64;
const RING: u64 = 64;
/// Records read this many after they were asked for.
const LAG: u64 = 16;

const SOURCE: &str = r#"
cbuffer Set : register(b0) { uint record; uint cells; uint has_stats; uint sequence; };
globallycoherent RWByteAddressBuffer ring : register(u0);
#ifdef EXPOSURE
Texture2D<float4> average : register(t0);
Texture2D<float4> before : register(t1);
RWByteAddressBuffer stats : register(u1);
RWStructuredBuffer<float4> mean : register(u2);
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void exposure() {
    uint at = record * 256;
    ring.Store(at + 252, 0);
    DeviceMemoryBarrier();
    ring.Store4(at, asuint(average.Load(int3(0, 0, 0))));
    ring.Store4(at + 16, asuint(before.Load(int3(0, 0, 0))));
    if (has_stats != 0) {
        ring.Store4(at + 64, stats.Load4(0));
        stats.Store4(0, uint4(0, 0, 0, 0));
        ring.Store4(at + 80, asuint(mean[cells]));
    } else {
        ring.Store4(at + 64, uint4(0, 0, 0, 0));
        ring.Store4(at + 80, uint4(0, 0, 0, 0));
    }
    DeviceMemoryBarrier();
    ring.Store(at + 252, sequence);
}
#endif
#ifdef LUX
Texture2D<float4> input : register(t0);
RWByteAddressBuffer output : register(u3);
[RootSignature(RS)]
[numthreads(1, 1, 1)]
void lux() {
    uint at = record * 256;
    ring.Store4(at + 32, output.Load4(0));
    ring.Store4(at + 48, asuint(input.Load(int3(0, 0, 0))));
}
#endif
"#;

struct Pass {
    exposure: (ID3D12RootSignature, ID3D12PipelineState),
    lux: (ID3D12RootSignature, ID3D12PipelineState),
    ring: ID3D12Resource,
    data: usize,
}
// SAFETY: D3D12 objects are free-threaded; the mapping lives as the buffer.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn pipeline(
    device: &ID3D12Device,
    part: &str,
    rs: &str,
    entry: windows::core::PCSTR,
) -> windows::core::Result<(ID3D12RootSignature, ID3D12PipelineState)> {
    let code = crate::host::compile(&format!("#define {part}\n#define RS \"{rs}\"\n{SOURCE}"), entry, s!("cs_5_1"))?;
    // SAFETY: the root signature is inside the shader; plain creations.
    unsafe {
        let root: ID3D12RootSignature = device.CreateRootSignature(0, &code)?;
        let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
            pRootSignature: std::mem::transmute_copy(&root),
            CS: D3D12_SHADER_BYTECODE { pShaderBytecode: code.as_ptr().cast(), BytecodeLength: code.len() },
            ..Default::default()
        };
        let pso = device.CreateComputePipelineState(&desc)?;
        Ok((root, pso))
    }
}

fn pass() -> Option<&'static Pass> {
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    PASS.get_or_init(|| {
        let host = host()?;
        let made = || -> windows::core::Result<Pass> {
            let exposure = pipeline(&host.device, "EXPOSURE", RS_EXPOSURE, s!("exposure"))?;
            let lux = pipeline(&host.device, "LUX", RS_LUX, s!("lux"))?;
            let (ring, data) = host.shared_buffer(RECORD * RING * 4).ok_or_else(windows::core::Error::empty)?;
            // SAFETY: the mapping holds the whole buffer.
            unsafe { std::ptr::write_bytes(data, 0, (RECORD * RING * 4) as usize) };
            Ok(Pass { exposure, lux, ring, data: data as usize })
        };
        made().inspect_err(|e| tracing::warn!(error = %e, "no light probe")).ok()
    })
    .as_ref()
}

/// Records asked for so far (the exposure program runs once a frame), and
/// when each was.
static ASKED: AtomicU64 = AtomicU64::new(0);
static WHEN: [AtomicU64; RING as usize] = [const { AtomicU64::new(0) }; RING as usize];

fn uav_barrier(list: &ID3D12GraphicsCommandList) {
    let barrier = D3D12_RESOURCE_BARRIER { Type: D3D12_RESOURCE_BARRIER_TYPE_UAV, ..Default::default() };
    // SAFETY: an open list; the barrier names no resource.
    unsafe { list.ResourceBarrier(&[barrier]) };
}

/// A completion identifies the request, not just the address in the ring.
/// Zero is reserved for an unwritten slot or one being overwritten.
fn sequence_tag(record: u64) -> u32 {
    (record % u64::from(u32::MAX)) as u32 + 1
}

fn completed_record<T>(
    record: u64,
    marker: impl Fn() -> u32,
    read: impl FnOnce() -> T,
) -> Option<T> {
    let expected = sequence_tag(record);
    if marker() != expected {
        return None;
    }
    let value = read();
    (marker() == expected).then_some(value)
}

fn newest_completed<T>(
    last: &mut Option<u64>,
    newest_first: impl Iterator<Item = u64>,
    mut read: impl FnMut(u64) -> Option<T>,
) -> Option<(u64, T)> {
    for record in newest_first {
        if last.is_some_and(|last| record <= last) {
            break;
        }
        if let Some(value) = read(record) {
            *last = Some(record);
            return Some((record, value));
        }
    }
    None
}

impl Pass {
    fn completed<T>(&self, record: u64, read: impl FnOnce(usize) -> T) -> Option<T> {
        let at = self.data + ((record % RING) * RECORD * 4) as usize;
        // SAFETY: the aligned completion word is within the retained mapping.
        let marker = || unsafe { ((at + 252) as *const u32).read_volatile() };
        completed_record(record, marker, || read(at))
    }
}

/// Writes the record asked for `LAG` records ago (the card has run it), and
/// feeds the latest completed exposure multiplier back to `crate::ambient`.
fn write_old(pass: &Pass, now: u64) {
    static DRAIN: Mutex<Option<u64>> = Mutex::new(None);
    let mut last = DRAIN.lock().unwrap_or_else(|e| e.into_inner());
    if !file_on() {
        let candidates = (1..=LAG).map_while(|back| now.checked_sub(back));
        if let Some((_, exp)) = newest_completed(&mut last, candidates, |record| {
            pass
                .completed(record, |at| {
                    // SAFETY: word 4 is within the retained record mapping.
                    f32::from_bits(unsafe { ((at + 16) as *const u32).read_volatile() })
                })
                .filter(|exp| exp.is_finite() && *exp > 1e-6)
        }) {
            crate::ambient::note_exposure(exp);
        }
        return;
    }
    let Some((old, words)) = newest_completed(&mut last, now.checked_sub(LAG).into_iter(), |record| {
        pass.completed(record, |at| {
            // SAFETY: the ring mapping covers the entire aligned record.
            unsafe { (at as *const [u32; RECORD as usize]).read_volatile() }
        })
    }) else {
        return;
    };
    let slot = old % RING;
    let f = |i: usize| f32::from_bits(words[i]);
    crate::ambient::note_exposure(f(4));
    let ms = WHEN[slot as usize].load(Relaxed);
    let samples = words[16];
    let mean = |i: usize| if samples == 0 { 0.0 } else { f64::from(words[i]) / 256.0 / f64::from(samples) };
    if let Some(file) = file() {
        let mut file = file.lock().unwrap_or_else(|e| e.into_inner());
        let _ = writeln!(
            file,
            "{old},{ms},{:.5},{:.5},{:.5},{:.5},{:.5},{:.4},{:.4},{:.4},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5},{samples},{:.5},{:.5},{:.5},{:.5},{:.5},{:.5}",
            f(0), f(1), f(2), f(3), f(4), f(5), f(6), f(7), f(8), f(9), f(10), f(11), f(12), f(13), f(14), f(15),
            mean(17), mean(18), mean(19), f(20), f(21), f(22)
        );
        if old % 64 == 0 {
            let _ = file.flush();
        }
    }
}

/// After the exposure program's draw, in `list` with its heap set: its two
/// inputs. Replaces the list's compute root signature and pipeline.
pub(crate) fn after_exposure(list: &HostList, average: D3D12_GPU_DESCRIPTOR_HANDLE, before: D3D12_GPU_DESCRIPTOR_HANDLE) -> bool {
    let Some(pass) = pass() else {
        return false;
    };
    let n = ASKED.fetch_add(1, Relaxed);
    let csv = file_on();
    if csv {
        WHEN[(n % RING) as usize].store(crate::host::now_ms(), Relaxed);
    }
    write_old(pass, n);
    let stats = csv.then(crate::ambient::probe_buffers).flatten();
    let words = [(n % RING) as u32, crate::ambient::CELLS_AT, u32::from(stats.is_some()), sequence_tag(n)];
    list.keep(&pass.ring);
    let l = &list.list;
    // SAFETY: an open list with the views' heap set; buffers kept.
    unsafe {
        if stats.is_some() {
            uav_barrier(l);
        }
        l.SetComputeRootSignature(&pass.exposure.0);
        l.SetPipelineState(&pass.exposure.1);
        l.SetComputeRoot32BitConstants(0, 4, words.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(1, average);
        l.SetComputeRootDescriptorTable(2, before);
        l.SetComputeRootUnorderedAccessView(3, pass.ring.GetGPUVirtualAddress());
        let (a, b) = match &stats {
            Some((stats, cells)) => {
                list.keep(stats);
                list.keep(cells);
                (stats.GetGPUVirtualAddress(), cells.GetGPUVirtualAddress())
            }
            None => (pass.ring.GetGPUVirtualAddress(), pass.ring.GetGPUVirtualAddress()),
        };
        l.SetComputeRootUnorderedAccessView(4, a);
        l.SetComputeRootUnorderedAccessView(5, b);
        l.Dispatch(1, 1, 1);
        if stats.is_some() {
            uav_barrier(l);
        }
    }
    true
}

/// After the lux program's dispatch: its input and output.
pub(crate) fn after_lux(list: &HostList, input: D3D12_GPU_DESCRIPTOR_HANDLE, output: D3D12_GPU_DESCRIPTOR_HANDLE) -> bool {
    let Some(pass) = pass() else {
        return false;
    };
    let n = ASKED.load(Relaxed).saturating_sub(1);
    let words = [(n % RING) as u32, 0, 0, 0];
    list.keep(&pass.ring);
    let l = &list.list;
    // SAFETY: as in `after_exposure`.
    unsafe {
        uav_barrier(l);
        l.SetComputeRootSignature(&pass.lux.0);
        l.SetPipelineState(&pass.lux.1);
        l.SetComputeRoot32BitConstants(0, 4, words.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(1, input);
        l.SetComputeRootDescriptorTable(2, output);
        l.SetComputeRootUnorderedAccessView(3, pass.ring.GetGPUVirtualAddress());
        l.Dispatch(1, 1, 1);
        uav_barrier(l);
    }
    true
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_new_exposure_is_never_replaced_by_an_older_ready_record() {
        let mut last = None;
        let ready = |record| [8, 9].contains(&record).then_some(record);
        assert_eq!(newest_completed(&mut last, (8..=9).rev(), ready), Some((9, 9)));
        assert_eq!(newest_completed(&mut last, (8..=10).rev(), ready), None);
        assert_eq!(newest_completed(&mut last, [10].into_iter(), Some), Some((10, 10)));
    }

    #[test]
    fn an_exposure_slot_rejects_old_data_across_a_ring_wrap_and_during_overwrite() {
        let old = 9;
        let current = old + RING;
        assert_eq!(current % RING, old % RING);
        assert_ne!(sequence_tag(current), 0);
        assert_ne!(sequence_tag(current), sequence_tag(old));
        let marker = std::cell::Cell::new(sequence_tag(old));
        assert!(completed_record(current, || marker.get(), || unreachable!()).is_none());
        marker.set(sequence_tag(current));
        assert_eq!(completed_record(current, || marker.get(), || 0.25), Some(0.25));
        assert_eq!(completed_record(current, || marker.get(), || {
            marker.set(0);
            0.25
        }), None);
    }

    #[test]
    fn roles_by_name() {
        assert_eq!(role(b"..PS_CalculateExposure_396dfccc_Wrapped.."), Some(Role::Exposure));
        assert_eq!(role(b"avgLuminanceCompute:ComputeLuxCompressionInfoSmooth_5b8554dd"), lux_on().then_some(Role::Lux));
        assert_eq!(role(b"PS_SetExposurePrevious"), None);
    }

    /// The passes compile with the system's compiler.
    #[test]
    fn the_passes_build() {
        let Some(host) = host() else {
            assert!(
                !cfg!(feature = "diagnostics") || !crate::switch::on("VICTORY_D3D12_WARP"),
                "WARP was requested but no host device opened"
            );
            return;
        };
        pipeline(&host.device, "EXPOSURE", RS_EXPOSURE, s!("exposure")).expect("exposure pass");
        pipeline(&host.device, "LUX", RS_LUX, s!("lux")).expect("lux pass");
    }
}
