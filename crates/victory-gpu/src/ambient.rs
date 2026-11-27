//! A stand-in for the indirect light the title traces rays for.
//!
//! With the ray-tracing structure builder off (`crate::pipeline::builder_runs`)
//! the title's rays find nothing. Its program that resolves the diffuse light
//! of its screen probes (`CS_ScreenProbes_TemporalResolve_*`) then leaves the
//! sky's light outdoors, nothing indoors (walls are black where no lamp or
//! sun reaches) and, in blocks of the picture's tiles, what the probes'
//! memory held. Right after that program the light is raised to an estimate
//! and bounded by a multiple of it.
//!
//! That a stand-in can be put there, and in which of the program's images,
//! is beloooo's find (VIctory-Belobug-AMD, 06031237, 4f45ab88, cd9f78ce,
//! GPL-3.0-or-later), which puts a constant there, shaded in creases from the
//! scene's depth. The estimate here is worked out from the frame instead, so
//! it follows the hour, the room and its lamps by itself: the light the
//! visible surfaces send out (the title keeps it for its own screen traces:
//! `CS_CacheScreenSpaceRadiance`, the frame before) is averaged in 64x36
//! cells, and every point gets the mean of the cells weighted by the share of
//! its surroundings they cover, as a directional value (first-order harmonics
//! at 160x90 points, taken between them by depth).
//!
//! What it assumes: what is not on screen sends out what is (with a slow
//! mean of the last seconds, so that turning into a dark corner is not black
//! at once). No occlusion but the creases'.
//!
//! `VICTORY_AMBIENT`:
//! - unset or `0`/`off`: the title's own output.
//! - a number: that share of the estimate (1: all of it).
//! - `constant:<level>`: the constant, as beloooo's fork has it.
//!
//! `VICTORY_AMBIENT_SHADE=<strength>,<radius in metres>` sets the creases.
//!
//! The air of a room is lit by the same probes: without rays they hold the
//! sky's light indoors too, a brown veil at sunset. `VICTORY_AMBIENT_AIR`
//! bounds what the air scatters by a multiple of the light around.

use std::sync::atomic::AtomicU32;
use std::sync::{Mutex, OnceLock};

use windows::Win32::Graphics::Direct3D12::*;
use windows::core::s;

use crate::host::{HostList, host};
use crate::object::This;

/// Programs of the title the stand-in follows.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum Role {
    /// Resolves the diffuse light.
    Resolve,
    /// Keeps the light visible surfaces send out.
    Radiance,
    /// Adds the lamps' light to what the air scatters (the last program
    /// that writes that volume before it is summed up along the view).
    Air,
    /// Writes outdoor sky fallback for missed specular reflection rays
    /// (`CS_RayMiss_*`, excluding volumetrics) before SSR is composited.
    RayMiss,
    /// Writes the title's final specular reflection history (`CS_UpdateHistory`).
    Reflect,
    /// Rebuilds the in-scatter marched along rays from its gathered image:
    /// followed before it runs, not after ([`clear_march`]).
    March,
}

/// The program's part in this, by its name in the container.
pub(crate) fn role(container: &[u8]) -> Option<Role> {
    let has = |name: &[u8]| container.windows(name.len()).any(|w| w == name);
    if has(b"CS_ScreenProbes_TemporalResolve") {
        Some(Role::Resolve)
    } else if has(b"CS_CacheScreenSpaceRadiance") {
        Some(Role::Radiance)
    } else if has(b"CS_AccumulateLocalLightInscatteringVolume") {
        Some(Role::Air)
    } else if has(b"CS_RayMiss_") && !has(b"CS_RayMiss_Volumetrics") {
        Some(Role::RayMiss)
    } else if has(b"CS_UpdateHistory") {
        Some(Role::Reflect)
    } else if has(b"CS_ReconstructRaymarchClassified") {
        Some(Role::March)
    } else {
        None
    }
}

/// Where the resolve writes the diffuse light (R9G9B9E5 as 32-bit integers)
/// and reads the scene's depth, and where the other writes the light of
/// visible surfaces: root parameter and place in its table, in dwords.
pub(crate) const LIGHT: (u8, u32) = (1, 176);
pub(crate) const DEPTH: (u8, u32) = (0, 464);
pub(crate) const RADIANCE: (u8, u32) = (1, 16);
/// Where the light the air scatters is.
pub(crate) const AIR: (u8, u32) = (1, 32);
/// Where `CS_RayMiss_*` writes missed specular reflection sky radiance.
pub(crate) const RAY_MISS: (u8, u32) = (1, 104);
/// Where `CS_UpdateHistory` writes final reflections (R9G9B9E5 in R32_UINT; Belobug 44dcd437).
pub(crate) const REFLECTIONS: (u8, u32) = (1, 40);
/// Where `CS_ReconstructRaymarchClassified*` reads the gathered in-scatter
/// (R16G16B16A16_FLOAT; `Full` reads both, `NoAlpha` the first).
pub(crate) const MARCH: [(u8, u32); 2] = [(1, 96), (1, 112)];

#[derive(Clone, Copy, PartialEq, Debug)]
enum Set {
    Off,
    Estimate(f32),
    Constant(f32),
}

fn parse(set: &str) -> Set {
    let set = set.trim();
    let number = |t: &str| t.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v > 0.0);
    match set.strip_prefix("constant:") {
        Some(level) => number(level).map_or(Set::Off, Set::Constant),
        None => number(set).map_or(Set::Off, Set::Estimate),
    }
}

fn set() -> Set {
    static SET: OnceLock<Set> = OnceLock::new();
    *SET.get_or_init(|| match parse(&std::env::var("VICTORY_AMBIENT").unwrap_or_default()) {
        // With the RT band asked for and no share, the band's level.
        Set::Off => rt_band().map_or(Set::Off, |b| Set::Estimate(b.level)),
        set => set,
    })
}

/// With ray tracing (`VICTORY_AMBIENT_RT=<least>,<most>[,<level>]`; unset:
/// off, the title's own light whenever the builder runs): the diffuse light
/// the title's resolve leaves is kept between `least` and `most` times the
/// estimate (ported from avatarmanz/VIctory-7900XTX `77137b9`).
#[derive(Clone, Copy, PartialEq, Debug)]
pub(crate) struct Band {
    least: f32,
    most: f32,
    level: f32,
}

pub(crate) fn rt_band() -> Option<Band> {
    static SET: OnceLock<Option<Band>> = OnceLock::new();
    *SET.get_or_init(|| {
        let band = parse_band(&std::env::var("VICTORY_AMBIENT_RT").ok()?)?;
        tracing::info!(
            least = band.least,
            most = band.most,
            level = band.level,
            "the stand-in keeps the traced indirect light in a band (VICTORY_AMBIENT_RT)"
        );
        Some(band)
    })
}

fn parse_band(set: &str) -> Option<Band> {
    let mut parts = set
        .split(',')
        .map(|p| p.trim().parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0));
    let least = parts.next().flatten()?;
    let most = parts.next().flatten().unwrap_or(4.0).max(least).max(1e-3);
    let level = parts.next().flatten().filter(|v| *v > 0.0).unwrap_or(0.8);
    Some(Band { least, most, level })
}

/// Whether the stand-in is on: set, and the title's rays find nothing (or
/// the band is asked for with them).
pub(crate) fn on() -> bool {
    set() != Set::Off && (rt_band().is_some() || !crate::pipeline::builder_runs())
}

/// The title's leftover light is kept between these multiples of the
/// estimate: with rays, the band's; without, at least the estimate and at
/// most `most()`.
fn bounds() -> (f32, f32) {
    match rt_band() {
        Some(b) if crate::pipeline::builder_runs() => (b.least, b.most),
        _ => (1.0, most()),
    }
}

/// The most the air scatters, as a multiple of the light around
/// (`VICTORY_AMBIENT_AIR=<multiple>`; unset or 0: what the title has).
fn air() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| multiple(&std::env::var("VICTORY_AMBIENT_AIR").unwrap_or_default()))
}

fn multiple(set: &str) -> f32 {
    set.trim().parse().ok().filter(|v: &f32| v.is_finite() && *v > 0.0).unwrap_or(0.0)
}

/// The most the in-scatter marched along rays holds, as a multiple of the
/// room's slow mean (`VICTORY_AMBIENT_MARCH=<multiple>`; unset or 0: off).
pub(crate) fn march() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| multiple(&std::env::var("VICTORY_AMBIENT_MARCH").unwrap_or_default()))
}

/// How the slow mean of the light around follows the frames
/// (`VICTORY_AMBIENT_SETTLE=<time constant in ms>[,<jump factor>[,<fresh
/// seconds>]]`; unset: 250,3; 0: 0.04 of the way a frame, as before, which is 25
/// frames: 0.8 s at the console's 30 frames a second, 2-4 s at 7-13). Set,
/// it is the same share of the way a second whatever the frame rate, and a
/// change by more than the jump factor (default 3; 0: none) is taken at
/// once. Fresh seconds (default 0: none): for that long after such a jump
/// the light the title's own resolve left is bounded by the estimate, not
/// four times it, rising back to four times by the end.
#[derive(Clone, Copy)]
struct Settle {
    ms: f32,
    jump: f32,
    fresh: f32,
}

fn settle() -> Option<Settle> {
    static SET: OnceLock<Option<Settle>> = OnceLock::new();
    *SET.get_or_init(|| {
        // Default on (250 ms, jump factor 3: reports/LIGHT-POPIN.md); `0`
        // gives the old 0.04 of the way a frame.
        let set = std::env::var("VICTORY_AMBIENT_SETTLE").unwrap_or_else(|_| "250,3".to_owned());
        let mut parts = set.split(',').map(|p| p.trim().parse::<f32>().ok());
        let ms = parts.next().flatten().filter(|v| v.is_finite() && *v > 0.0)?;
        let jump = parts.next().flatten().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(3.0);
        let fresh = parts.next().flatten().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0);
        tracing::info!(
            ms,
            jump,
            fresh,
            "the stand-in's mean of the light around settles by the clock (VICTORY_AMBIENT_SETTLE)"
        );
        Some(Settle { ms, jump, fresh })
    })
}

/// The share of the way the slow mean goes at this frame's keeping
/// ([`Role::Radiance`]; the other passes do not read it).
fn follow(role: Role) -> f32 {
    match settle() {
        Some(settle) if role == Role::Radiance => 1.0 - (-frame_ms(role) / settle.ms).exp(),
        _ => 0.04,
    }
}

/// Milliseconds since the last keeping ([`Role::Radiance`], once a frame);
/// the first, and after a pause of over a second, as a frame of 30. Other
/// roles: the last answer.
fn frame_ms(role: Role) -> f32 {
    use std::sync::atomic::{AtomicU32, AtomicU64, Ordering::Relaxed};
    static LAST: AtomicU64 = AtomicU64::new(0);
    static DT: AtomicU32 = AtomicU32::new(0x4204_0000); // 33.0
    if role != Role::Radiance {
        return f32::from_bits(DT.load(Relaxed));
    }
    // (Asked twice in a keeping: the words and the share.)
    static ASKED: AtomicU64 = AtomicU64::new(0);
    let now = crate::host::now_ms();
    if ASKED.swap(now, Relaxed) == now {
        return f32::from_bits(DT.load(Relaxed));
    }
    let last = LAST.swap(now, Relaxed);
    let dt = match now.saturating_sub(last) {
        0 => 1.0,
        dt if last == 0 || dt > 1000 => 33.0,
        dt => dt as f32,
    };
    DT.store(dt.to_bits(), Relaxed);
    dt
}

/// Strength and radius of the creases' shade.
fn shade() -> [f32; 2] {
    static SET: OnceLock<[f32; 2]> = OnceLock::new();
    *SET.get_or_init(|| {
        let mut shade = [1.0, 0.6];
        let set = std::env::var("VICTORY_AMBIENT_SHADE").unwrap_or_default();
        for (to, v) in shade.iter_mut().zip(set.split(',')) {
            if let Ok(v) = v.trim().parse() {
                *to = v;
            }
        }
        shade
    })
}

/// The most missed specular reflection rays (`CS_RayMiss_*`) may shine
/// indoors, as a multiple of the light around (`VICTORY_AMBIENT_REFL`,
/// default 2.0; 0: untouched). Prevents outdoor sky cubemaps from making
/// character corneas glow white/blue in dark rooms while preserving SSR.
fn refl() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| {
        match std::env::var("VICTORY_AMBIENT_REFL").ok().as_deref().map(str::trim) {
            Some("0" | "off") => 0.0,
            Some(s) => s.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(2.0),
            None => 2.0,
        }
    })
}

/// Optional minimum pre-exposure HDR indoor indirect light floor
/// (`VICTORY_AMBIENT_FLOOR`, default `0` / off).
fn floor() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| {
        match std::env::var("VICTORY_AMBIENT_FLOOR").ok().as_deref().map(str::trim) {
            Some("0" | "off") | None => 0.0,
            Some(s) => s.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0),
        }
    })
}

/// Post-exposure indoor diffuse ambient floor (`VICTORY_AMBIENT_EXPOSURE_FLOOR`,
/// default `0.0045`; `0` or `off` disables). Scales the diffuse stand-in floor
/// inversely with the frame's `PS_CalculateExposure` factor so dark backlit
/// interiors locked to bright outdoor EV (e.g. diner EV -9.75) stay legible
/// with SSAO shading while sunlit exteriors (EV -5.5) and night scenes remain
/// untouched.
fn exposure_floor() -> f32 {
    static SET: OnceLock<f32> = OnceLock::new();
    *SET.get_or_init(|| {
        match std::env::var("VICTORY_AMBIENT_EXPOSURE_FLOOR").ok().as_deref().map(str::trim) {
            Some("0" | "off") => 0.0,
            Some(s) => s.parse::<f32>().ok().filter(|v| v.is_finite() && *v >= 0.0).unwrap_or(0.0045),
            None => 0.0045,
        }
    })
}

static LAST_EXPOSURE: AtomicU32 = AtomicU32::new(0);

/// Records the latest finite exposure multiplier read by `lightprobe`.
pub(crate) fn note_exposure(exp: f32) {
    if exp.is_finite() && exp > 1e-6 {
        LAST_EXPOSURE.store(exp.to_bits(), std::sync::atomic::Ordering::Relaxed);
    }
}

fn current_exposure() -> Option<f32> {
    let bits = LAST_EXPOSURE.load(std::sync::atomic::Ordering::Relaxed);
    (bits != 0).then(|| f32::from_bits(bits)).filter(|v| v.is_finite() && *v > 1e-6)
}

/// Whether `ambient` needs lightweight `PS_CalculateExposure` tracking.
pub(crate) fn needs_exposure() -> bool {
    on() && exposure_floor() > 0.0
}

/// Upper cap and lower floor on final reflection history (`CS_UpdateHistory`,
/// Belobug 44dcd437 / vx-layer), as parts of the local light around
/// (`VICTORY_AMBIENT_REFLECT`, `VICTORY_AMBIENT_REFLECT_LEAST`; unset or 0: off).
fn reflect() -> [f32; 2] {
    static SET: OnceLock<[f32; 2]> = OnceLock::new();
    *SET.get_or_init(|| {
        let part = |name: &str| {
            std::env::var(name)
                .ok()
                .and_then(|v| v.trim().parse::<f32>().ok())
                .filter(|v| v.is_finite() && *v > 0.0)
                .unwrap_or(0.0)
        };
        let set = [part("VICTORY_AMBIENT_REFLECT"), part("VICTORY_AMBIENT_REFLECT_LEAST")];
        if set != [0.0, 0.0] {
            tracing::info!(
                most = set[0],
                least = set[1],
                "reflections held to the light around (VICTORY_AMBIENT_REFLECT, _LEAST)"
            );
        }
        set
    })
}

const SOURCE: &str = r#"
#define RS "RootConstants(num32BitConstants=18, b0), DescriptorTable(SRV(t0)), DescriptorTable(UAV(u0)), UAV(u1), UAV(u2), UAV(u3)"
#define CX 64
#define CY 36
#define CELLS (CX * CY)
#define PX 160
#define PY 90
cbuffer Set : register(b0) {
    float level; float constant; float strength; float radius;
    float near; float tan_y; uint width; uint height;
    float follow; float most; float air; uint probe;
    float jump; float fresh; float dt; float refl;
    float floor_level; float least;
};
Texture2D<float> depth : register(t0);
// Cells: the light they send out (then, at CELLS, the slow mean), from
// CELLS + 1 on where they are.
RWStructuredBuffer<float4> cells : register(u1);
// Four float4s a point (64-byte aligned): red, green and blue (each a direction
// and mean), then `(depth_z, 0, 0, 0)` so `raise` reuses coarse depth without
// 4 redundant full-res `depth.Load` calls per pixel (AnyXboxX#475).
RWStructuredBuffer<float4> points : register(u2);
// For the light probe (`crate::lightprobe`): sampled pixels, and the sums of
// what the title's resolve left, what was found and what was written.
RWByteAddressBuffer stats : register(u3);
static const float3 LUMA = float3(0.2126, 0.7152, 0.0722);

// The point seen at a texel, in metres from the eye (x right, y up, z away).
float3 seen(int2 p, float2 size) {
    float d = depth.Load(int3(clamp(p, int2(0, 0), int2(size) - 1), 0));
    float z = min(near / max(d, 1e-7), 1000.0);
    float2 ndc = (float2(p) + 0.5) / size * 2.0 - 1.0;
    return float3(ndc.x * tan_y * size.x / size.y * z, -ndc.y * tan_y * z, z);
}

// Smooth bilinear sample of the gathered 160x90 ambient radiance grid so
// reflection/air caps never create 12x12 pixel block edges across silhouettes.
float3 sample_points(float2 uv) {
    float2 g = uv * float2(PX, PY) - 0.5;
    int2 g0 = int2(floor(g));
    float2 f = saturate(g - float2(g0));
    float3 res = 0.0;
    for (int k = 0; k < 4; k++) {
        int2 q = clamp(g0 + int2(k & 1, k >> 1), int2(0, 0), int2(PX, PY) - 1);
        float w = (k & 1 ? f.x : 1.0 - f.x) * (k >> 1 ? f.y : 1.0 - f.y);
        uint at0 = (q.y * PX + q.x) * 4;
        res += w * float3(points[at0].w, points[at0 + 1].w, points[at0 + 2].w);
    }
    return any(isnan(res)) || any(isinf(res)) ? 0.0 : max(res, 0.0);
}

#ifdef KEEP
RWTexture2D<float3> radiance : register(u0);
[RootSignature(RS)]
[numthreads(8, 4, 1)]
void keep(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    radiance.GetDimensions(w, h);
    float3 sum = 0;
    for (uint y = 0; y < 5; y++) for (uint x = 0; x < 5; x++) {
        uint2 p = uint2((float2(at.xy) + (float2(x, y) + 0.5) / 5.0) * float2(w, h) / float2(CX, CY));
        float3 c = max(radiance[p], 0.0);
        c = any(isnan(c)) || any(isinf(c)) ? 0.0 : c;
        // Lamps and glints count, but not for everything.
        sum += c / max(1.0, dot(c, LUMA) / 20.0);
    }
    cells[at.y * CX + at.x] = float4(sum / 25.0, 1.0);
}
groupshared float3 mean_acc[64];
[RootSignature(RS)]
[numthreads(64, 1, 1)]
void mean(uint gi : SV_GroupIndex) {
    float3 sum = 0;
    for (uint i = gi; i < CELLS; i += 64) sum += cells[i].rgb;
    mean_acc[gi] = sum;
    GroupMemoryBarrierWithGroupSync();
    [unroll]
    for (uint s = 32; s > 0; s >>= 1) {
        if (gi < s) mean_acc[gi] += mean_acc[gi + s];
        GroupMemoryBarrierWithGroupSync();
    }
    if (gi != 0) return;
    float4 was = cells[CELLS];
    float3 floor_rgb = float3(1.0, 0.98, 0.94) * floor_level;
    float3 now = max(mean_acc[0] / CELLS, floor_rgb);
    // (`jump` > 0: an upward brightness jump of more than that factor is
    // followed at once when emerging into bright light; downward drops decay
    // smoothly with `follow` so looking down at dark surfaces or entering dim
    // rooms never collapses indirect light to black in a single frame.)
    float a = max(dot(now, LUMA), 1e-5), b = max(dot(was.rgb, LUMA), 1e-5);
    bool jumped = jump > 0.0 && (a / b) > jump;
    // Its fourth part: seconds since the light around last jumped (from
    // 0.001; 0: nothing kept yet).
    float age = was.a > 0.0 && !jumped ? was.a + dt : 0.001;
    cells[CELLS] = float4(was.a > 0.0 ? lerp(was.rgb, now, jumped ? 1.0 : follow) : now, age);
}
#endif

#ifdef PLACE
[RootSignature(RS)]
[numthreads(8, 4, 1)]
void place(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    float3 p = seen(int2((float2(at.xy) + 0.5) * size / float2(CX, CY)), size);
    cells[CELLS + 1 + at.y * CX + at.x] = float4(p, p.z * p.z);
}
#endif

#ifdef GATHER
groupshared float4 tile_pos[64];
groupshared float3 tile_lit[64];
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void gather(uint3 at : SV_DispatchThreadID, uint gi : SV_GroupIndex) {
    bool valid = at.x < PX && at.y < PY;
    float2 size = float2(width, height);
    float3 p = valid ? seen(int2((float2(at.xy) + 0.5) * size / float2(PX, PY)), size) : float3(0, 0, 1);
    float r2 = 0.25 + 0.0025 * p.z * p.z;
    float whole = 0;
    float3 toward = 0, light = 0, red = 0, green = 0, blue = 0;
    // Cooperative 64-cell tiling cuts global UAV loads 64x while preserving
    // the exact 0..CELLS accumulation order (AnyXboxX#475).
    for (uint base = 0; base < CELLS; base += 64) {
        tile_pos[gi] = cells[CELLS + 1 + base + gi];
        tile_lit[gi] = cells[base + gi].rgb;
        GroupMemoryBarrierWithGroupSync();
        if (valid) {
            for (uint k = 0; k < 64; k++) {
                float4 c = tile_pos[k];
                float3 L = tile_lit[k];
                float3 d = c.xyz - p;
                float dd = dot(d, d);
                float w = c.w / (dd + r2);
                float3 dir = d / sqrt(dd + 0.25 * r2);
                whole += w;
                toward += w * dir;
                light += w * L;
                red += w * L.r * dir;
                green += w * L.g * dir;
                blue += w * L.b * dir;
            }
        }
        GroupMemoryBarrierWithGroupSync();
    }
    if (!valid) return;
    whole = max(whole, 1e-6);
    light /= whole;
    toward /= whole;
    uint at0 = (at.y * PX + at.x) * 4;
    points[at0] = float4(red / whole - light.r * toward, light.r);
    points[at0 + 1] = float4(green / whole - light.g * toward, light.g);
    points[at0 + 2] = float4(blue / whole - light.b * toward, light.b);
    points[at0 + 3] = float4(p.z, 0.0, 0.0, 0.0);
}
#endif

#ifdef AIR
RWTexture3D<float3> scattered : register(u0);
// No more than a multiple of the light around: of the room as the last
// seconds showed it, and of the surface seen in this direction (towards a
// window the air outside may be as bright as the sky is).
[RootSignature(RS)]
[numthreads(8, 8, 2)]
void clear_air(uint3 at : SV_DispatchThreadID) {
    uint w, h, d;
    scattered.GetDimensions(w, h, d);
    if (at.x >= w || at.y >= h || at.z >= d) return;
    float3 seen_there = sample_points((float2(at.xy) + 0.5) / float2(w, h));
    float far = float(at.z) / float(d);
    float cap = air * lerp(dot(cells[CELLS].rgb, LUMA), dot(seen_there, LUMA), far);
    float3 c = scattered[at];
    if (any(isnan(c)) || any(isinf(c))) {
        scattered[at] = 0.0;
        return;
    }
    c = max(c, 0.0);
    float has = dot(c, LUMA);
    if (has > cap && has > 1e-9) scattered[at] = c * (cap / has);
}
#endif

#ifdef MARCH
RWTexture2D<float4> marched : register(u0);
// No more than a multiple of the room's light as the last seconds showed
// it (`air` is that multiple here); the hue and the fourth channel stay.
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void clear_march(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    marched.GetDimensions(w, h);
    if (at.x >= w || at.y >= h) return;
    float4 c = marched[at.xy];
    if (any((asuint(c.rgb) & 0x7f800000u) == 0x7f800000u)) {
        marched[at.xy] = float4(0.0, 0.0, 0.0, (isnan(c.a) || isinf(c.a)) ? 0.0 : c.a);
        return;
    }
    float cap = air * dot(cells[CELLS].rgb, LUMA);
    float has = dot(c.rgb, LUMA);
    if (has > cap && has > 1e-9) marched[at.xy] = float4(c.rgb * (cap / has), c.a);
}
#endif

#ifdef REFL
RWTexture2D<float4> reflected : register(u0);
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void clear_refl(uint3 at : SV_DispatchThreadID) {
    uint w, h;
    reflected.GetDimensions(w, h);
    if (at.x >= w || at.y >= h) return;
    float3 seen_there = sample_points((float2(at.xy) + 0.5) / float2(w, h));
    float cap = refl * max(dot(cells[CELLS].rgb, LUMA), dot(seen_there, LUMA));
    float4 c = reflected[at.xy];
    if (any(isnan(c.rgb)) || any(isinf(c.rgb))) {
        reflected[at.xy] = float4(0.0, 0.0, 0.0, (isnan(c.a) || isinf(c.a)) ? 0.0 : c.a);
        return;
    }
    c.rgb = max(c.rgb, 0.0);
    float has = dot(c.rgb, LUMA);
    if (cells[CELLS].a > 0.0 && has > cap && has > 1e-9) {
        reflected[at.xy] = float4(c.rgb * (cap / has), c.a);
    }
}
#endif

#ifdef HOLD
RWTexture2D<uint> held : register(u0);
float3 unpack9(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack9(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float top = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(top, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (top * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}
// Final specular reflection history (`CS_UpdateHistory`): held between `least`
// and `air` (reused as max multiplier) times local ambient radiance to eliminate
// overbright glossy glaze / bloom blocks indoors and black asphalt outdoors.
[RootSignature(RS)]
[numthreads(8, 8, 1)]
void hold(uint3 at : SV_DispatchThreadID) {
    if (at.x >= width || at.y >= height) return;
    float3 around = constant > 0.0
        ? float3(1.0, 0.98, 0.94) * constant
        : sample_points((float2(at.xy) + 0.5) / float2(width, height)) * level;
    float l = dot(around, LUMA);
    if (l <= 1e-6) return;
    float3 c = unpack9(held[at.xy]);
    if (any(isnan(c)) || any(isinf(c)) || (c.g < 0.05 * min(c.r, c.b) && max(c.r, c.b) > 50.0)) {
        c = 0.0;
    }
    float has = dot(c, LUMA);
    if (air > 0.0 && has > air * l) c *= (air * l) / max(has, 1e-6);
    float low = least * l;
    if (low > 0.0 && has < low) c = has > 1e-4 ? c * (low / has) : around * least;
    held[at.xy] = pack9(c);
}
#endif

#ifdef RAISE
RWTexture2D<uint> light : register(u0);
float3 unpack(uint v) {
    return float3(v & 511, (v >> 9) & 511, (v >> 18) & 511) * exp2(float(v >> 27) - 24.0);
}
uint pack(float3 c) {
    c = clamp(c, 0.0, 65408.0);
    float top = max(c.r, max(c.g, c.b));
    int e = clamp(int(floor(log2(max(top, 1e-9)))) + 16, 0, 31);
    float scale = exp2(float(24 - e));
    if (top * scale >= 511.5) { e = min(e + 1, 31); scale = exp2(float(24 - e)); }
    uint3 m = min(uint3(c * scale + 0.5), 511u);
    return m.r | (m.g << 9) | (m.b << 18) | (uint(e) << 27);
}
// 32x32 groups (1024 threads) keep each 32x32 pixel tile resident on one SM so
// the 12-tap spiral depth taps share L1 texture cache (AnyXboxX#486).
[RootSignature(RS)]
[numthreads(32, 32, 1)]
void raise(uint3 at : SV_DispatchThreadID) {
    float2 size = float2(width, height);
    if (at.x >= width || at.y >= height) return;
    int2 p = int2(at.xy);
    float3 c = seen(p, size);
    float3 l = seen(p + int2(-1, 0), size), r = seen(p + int2(1, 0), size);
    float3 u = seen(p + int2(0, -1), size), d = seen(p + int2(0, 1), size);
    float3 dx = p.x == 0 ? r - c : (p.x + 1 >= int(width) ? c - l : (abs(l.z - c.z) < abs(r.z - c.z) ? c - l : r - c));
    float3 dy = p.y == 0 ? d - c : (p.y + 1 >= int(height) ? c - u : (abs(u.z - c.z) < abs(d.z - c.z) ? c - u : d - c));
    float3 n = normalize(cross(dy, dx) + float3(0, 0, -1e-9));
    if (dot(n, c) > 0.0) n = -n;
    // How open it is (as in beloooo's cd9f78ce), skipping off-screen taps.
    float open = 1.0;
    float px = min(radius * size.y / (2.0 * tan_y * c.z), 128.0);
    float noise = frac(52.9829189 * frac(dot(float2(p), float2(0.06711056, 0.00583715))));
    if (strength > 0.0 && px >= 1.5 && c.z < 400.0) {
        float sum = 0.0;
        int valid = 0;
        const int N = 12;
        for (int i = 0; i < N; i++) {
            float a = i * 2.399963 + noise * 6.2831853;
            float2 o = float2(cos(a), sin(a)) * (sqrt((i + 0.5) / N) * px);
            int2 q = p + int2(round(o));
            if (any(q < 0) || any(q >= int2(size))) continue;
            valid += 1;
            float3 v = seen(q, size) - c;
            float vv = dot(v, v);
            float over = dot(v, n) - 0.004 * c.z - 0.005;
            sum += max(over, 0.0) / sqrt(vv + 1e-6) * saturate(1.0 - vv / (radius * radius));
        }
        if (valid > 0) {
            open = saturate(1.0 - strength * 2.0 * sum / float(valid));
        }
    }
    float3 found;
    if (constant > 0.0) {
        found = float3(1.0, 0.98, 0.94) * constant * open;
        light[p] = pack(found);
        return;
    }
    // Between the four points around, those at its own depth first; which
    // four is dithered a little.
    float2 g = (float2(p) + 0.5) / size * float2(PX, PY) - 0.5 + (noise - 0.5) * 0.5;
    int2 g0 = int2(floor(g));
    float2 f = g - float2(g0);
    float4 sr = 0, sg = 0, sb = 0;
    float sum = 0;
    for (int k = 0; k < 4; k++) {
        int2 q = clamp(g0 + int2(k & 1, k >> 1), int2(0, 0), int2(PX, PY) - 1);
        uint at0 = (q.y * PX + q.x) * 4;
        float z = points[at0 + 3].x;
        float2 b = float2(k & 1 ? f.x : 1 - f.x, k >> 1 ? f.y : 1 - f.y);
        float w = (b.x * b.y + 1e-3) / (0.02 + abs(z - c.z) / c.z);
        sr += w * points[at0];
        sg += w * points[at0 + 1];
        sb += w * points[at0 + 2];
        sum += w;
    }
    float3 all = float3(sr.w, sg.w, sb.w) / sum;
    found = all + 1.2 * float3(dot(sr.xyz, n), dot(sg.xyz, n), dot(sb.xyz, n)) / sum;
    found = clamp(found, 0.2 * all, 2.5 * all);
    found = lerp(found, cells[CELLS].rgb, 0.2) * level * open;
    float3 floor_light = float3(1.0, 0.98, 0.94) * floor_level * level * open;
    found = max(found, floor_light);
    float3 was = unpack(light[p]);
    float left = dot(was, LUMA);
    // Scrub NaN/Inf and uninitialized magenta/violet probe garbage in diffuse GI
    // (`was` is indirect diffuse light only, so purple posters/albedo & emissive UI stay intact;
    // VIctory-NVIDIA#29 / Belobug-AMD).
    if (any(isnan(was)) || any(isinf(was))) {
        was = 0.0;
    }
    if (was.g < 0.5 * min(was.r, was.b) && max(was.r, was.b) > 0.02) {
        was = 0.0;
    }
    if (was.b > 2.5 * was.g && was.b > was.r && was.b > 0.02) {
        was = 0.0;
    }
    // Once radiance cells are initialized (`cells[CELLS].a > 0`), always bound
    // `was` even inside concave creases (`open -> 0`), while flooring the upper
    // cap base at 15% of room radiance or the indoor ambient floor so dim
    // interiors and dark corners keep soft shading without collapsing to pitch
    // black, while recessed eye sockets still clamp raw outdoor sky leaks.
    float found_luma = dot(found, LUMA);
    if (cells[CELLS].a > 0.0) {
        float bound = fresh > 0.0 ? lerp(1.0, most, saturate(cells[CELLS].a / fresh)) : most;
        float room_luma = dot(cells[CELLS].rgb, LUMA);
        float cap_base = max(found_luma, max(0.15 * level * room_luma, dot(floor_light, LUMA)));
        float top = max(cap_base * bound, 0.0);
        float has = dot(was, LUMA);
        was *= min(1.0, top / max(has, 1e-9));
    }
    float3 written = max(was, found * least);
    light[p] = pack(written);
    if (probe != 0 && (at.x & 15) == 0 && (at.y & 15) == 0) {
        uint ignored;
        stats.InterlockedAdd(0, 1, ignored);
        stats.InterlockedAdd(4, uint(min(left, 1000.0) * 256.0), ignored);
        stats.InterlockedAdd(8, uint(min(found_luma, 1000.0) * 256.0), ignored);
        stats.InterlockedAdd(12, uint(min(dot(written, LUMA), 1000.0) * 256.0), ignored);
    }
}
#endif
"#;

/// Thread-group width/height of [`SOURCE`]'s `raise` pass.
const RAISE_GROUP: u32 = 32;

/// Bytes of the cells' buffer and of the points'.
const CELLS: u64 = 64 * 36;
const CELL_BYTES: u64 = (2 * CELLS + 1) * 16;
const POINT_BYTES: u64 = 160 * 90 * 64;
/// Where the slow mean is among the cells (float4s).
pub(crate) const CELLS_AT: u32 = CELLS as u32;

/// Views of what the march clamp writes, a ring.
const MARCH_VIEWS: u32 = 64;

struct Pass {
    root: ID3D12RootSignature,
    keep: ID3D12PipelineState,
    mean: ID3D12PipelineState,
    place: ID3D12PipelineState,
    gather: ID3D12PipelineState,
    raise: ID3D12PipelineState,
    air: ID3D12PipelineState,
    refl: ID3D12PipelineState,
    hold: ID3D12PipelineState,
    /// The marched in-scatter clamp (`VICTORY_AMBIENT_MARCH`) and its views.
    march: Option<(ID3D12PipelineState, ID3D12DescriptorHeap)>,
    cells: ID3D12Resource,
    points: ID3D12Resource,
    stats: ID3D12Resource,
    step: u32,
    next: AtomicU32,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for Pass {}
// SAFETY: as above.
unsafe impl Sync for Pass {}

fn make(device: &ID3D12Device, buffer: impl Fn(u64) -> Option<ID3D12Resource>) -> windows::core::Result<Pass> {
    let mut root: Option<ID3D12RootSignature> = None;
    let mut pipeline = |part: &str, entry| -> windows::core::Result<ID3D12PipelineState> {
        let code = crate::host::compile(&format!("#define {part}\n{SOURCE}"), entry, s!("cs_5_1"))?;
        // SAFETY: the root signature is inside the shader; plain creations.
        unsafe {
            if root.is_none() {
                root = Some(device.CreateRootSignature(0, &code)?);
            }
            let desc = D3D12_COMPUTE_PIPELINE_STATE_DESC {
                pRootSignature: std::mem::transmute_copy(&root),
                CS: D3D12_SHADER_BYTECODE { pShaderBytecode: code.as_ptr().cast(), BytecodeLength: code.len() },
                ..Default::default()
            };
            device.CreateComputePipelineState(&desc)
        }
    };
    let keep = pipeline("KEEP", s!("keep"))?;
    let mean = pipeline("KEEP", s!("mean"))?;
    let place = pipeline("PLACE", s!("place"))?;
    let gather = pipeline("GATHER", s!("gather"))?;
    let raise = pipeline("RAISE", s!("raise"))?;
    let air = pipeline("AIR", s!("clear_air"))?;
    let refl = pipeline("REFL", s!("clear_refl"))?;
    let hold = pipeline("HOLD", s!("hold"))?;
    let kind = D3D12_DESCRIPTOR_HEAP_TYPE_CBV_SRV_UAV;
    let march = (march() > 0.0)
        .then(|| -> windows::core::Result<_> {
            let clamp = pipeline("MARCH", s!("clear_march"))?;
            // SAFETY: plain heap creation.
            let views: ID3D12DescriptorHeap = unsafe {
                device.CreateDescriptorHeap(&D3D12_DESCRIPTOR_HEAP_DESC {
                    Type: kind,
                    NumDescriptors: MARCH_VIEWS,
                    Flags: D3D12_DESCRIPTOR_HEAP_FLAG_SHADER_VISIBLE,
                    NodeMask: 0,
                })?
            };
            Ok((clamp, views))
        })
        .and_then(|made| made.inspect_err(|e| tracing::warn!(error = %e, "no clamp of the marched in-scatter")).ok());
    let made = |bytes| buffer(bytes).ok_or_else(windows::core::Error::empty);
    Ok(Pass {
        root: root.ok_or_else(windows::core::Error::empty)?,
        keep,
        mean,
        place,
        gather,
        raise,
        air,
        refl,
        hold,
        march,
        cells: made(CELL_BYTES)?,
        points: made(POINT_BYTES)?,
        stats: made(256)?,
        // SAFETY: a plain query.
        step: unsafe { device.GetDescriptorHandleIncrementSize(kind) },
        next: AtomicU32::new(0),
    })
}

fn pass() -> Option<&'static Pass> {
    static PASS: OnceLock<Option<Pass>> = OnceLock::new();
    PASS.get_or_init(|| {
        let host = host()?;
        make(&host.device, |bytes| host.uav_buffer(bytes))
            .inspect_err(|e| tracing::warn!(error = %e, "no stand-in for the indirect light"))
            .ok()
    })
    .as_ref()
}

/// The probe's sums and the cells (with the slow mean), once the passes are
/// made and the stand-in is on.
pub(crate) fn probe_buffers() -> Option<(ID3D12Resource, ID3D12Resource)> {
    if !on() {
        return None;
    }
    pass().map(|p| (p.stats.clone(), p.cells.clone()))
}

fn uav_barrier_of(resource: Option<&ID3D12Resource>) -> D3D12_RESOURCE_BARRIER {
    let borrowed = match resource {
        // SAFETY: `r` stays live through `ResourceBarrier`; `ManuallyDrop` skips `Release`.
        Some(r) => Some(unsafe { std::mem::transmute_copy::<ID3D12Resource, ID3D12Resource>(r) }),
        None => None,
    };
    D3D12_RESOURCE_BARRIER {
        Type: D3D12_RESOURCE_BARRIER_TYPE_UAV,
        Flags: D3D12_RESOURCE_BARRIER_FLAG_NONE,
        Anonymous: D3D12_RESOURCE_BARRIER_0 {
            UAV: std::mem::ManuallyDrop::new(D3D12_RESOURCE_UAV_BARRIER {
                pResource: std::mem::ManuallyDrop::new(borrowed),
            }),
        },
    }
}

fn uav_barrier(list: &ID3D12GraphicsCommandList, resource: Option<&ID3D12Resource>) {
    // SAFETY: an open list; any named resource stays live across the call.
    unsafe { list.ResourceBarrier(&[uav_barrier_of(resource)]) };
}

/// Whether the light of a frame has been kept: the estimate has something
/// to begin with.
static KEPT: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// What the passes read and write, as views in the heap the list has set.
pub(crate) struct Views {
    /// The image the title's program just wrote.
    pub(crate) image: D3D12_GPU_DESCRIPTOR_HANDLE,
    /// The scene's depth, for [`Role::Resolve`].
    pub(crate) depth: Option<D3D12_GPU_DESCRIPTOR_HANDLE>,
    /// The host texture behind `image`, for resource-scoped UAV barriers
    /// (falls back to a global UAV barrier if `None`).
    pub(crate) target: Option<ID3D12Resource>,
    /// The size of the image, and how many slices it has.
    pub(crate) size: [u32; 2],
    pub(crate) slices: u32,
}

/// Upper cap multiplier on indirect light (`VICTORY_AMBIENT_MOST`, default 4.0;
/// 2.0 avoids hazy overbright interiors when ray tracing is off).
fn most() -> f32 {
    static MOST: OnceLock<f32> = OnceLock::new();
    *MOST.get_or_init(|| {
        std::env::var("VICTORY_AMBIENT_MOST")
            .ok()
            .and_then(|v| v.trim().parse::<f32>().ok())
            .filter(|&v| v.is_finite() && v > 0.0)
            .unwrap_or(4.0)
    })
}

/// After the title's program of that role, in `list` with the program's view
/// heap set. Replaces the list's compute root signature and pipeline; false
/// if it recorded nothing.
pub(crate) fn after(list: &HostList, role: Role, views: &Views) -> bool {
    use std::sync::atomic::Ordering;
    let Some(pass) = pass() else {
        return false;
    };
    let (level, constant) = match set() {
        Set::Off => return false,
        Set::Estimate(level) => (level, 0.0f32),
        Set::Constant(level) => (0.0, level),
    };
    if role != Role::Radiance && constant == 0.0 && !KEPT.load(Ordering::Relaxed) {
        return false;
    }
    if role == Role::March || (role == Role::Air && (constant > 0.0 || air() == 0.0)) {
        return false;
    }
    if role == Role::RayMiss && (constant > 0.0 || refl() == 0.0) {
        return false;
    }
    if role == Role::Reflect && reflect() == [0.0, 0.0] {
        return false;
    }
    static SAID: Mutex<u32> = Mutex::new(0);
    {
        let mut said = SAID.lock().unwrap_or_else(|e| e.into_inner());
        if *said < 8 {
            *said += 1;
            tracing::info!(?role, size = ?views.size, level, constant, "a stand-in for the indirect light");
        }
    }
    list.keep(&pass.cells);
    list.keep(&pass.points);
    list.keep(&pass.stats);
    if let Some(target) = &views.target {
        list.keep(target);
    }
    let [strength, radius] = shade();
    let (least, upper) = bounds();
    let floats = [level, constant, strength, radius, 0.1, 0.45];
    let mut words = [0u32; 18];
    for (to, v) in words.iter_mut().zip(floats) {
        *to = v.to_bits();
    }
    words[6] = views.size[0];
    words[7] = views.size[1];
    words[8] = follow(role).to_bits();
    words[9] = upper.to_bits();
    words[10] = air().to_bits();
    words[11] = u32::from(crate::lightprobe::file_on());
    words[12] = settle().map_or(0.0, |s| s.jump).to_bits();
    words[13] = settle().map_or(0.0, |s| s.fresh).to_bits();
    words[14] = (frame_ms(role) / 1000.0).to_bits();
    words[15] = refl().to_bits();
    let effective_floor = if role == Role::Resolve {
        let auto = if exposure_floor() > 0.0 {
            current_exposure().map_or(0.0, |e| (exposure_floor() / e.clamp(0.0004, 1.0)).min(8.0))
        } else {
            0.0
        };
        floor().max(auto)
    } else {
        floor()
    };
    words[16] = effective_floor.to_bits();
    words[17] = least.to_bits();
    if role == Role::Reflect {
        let [most, low] = reflect();
        words[10] = most.to_bits();
        words[17] = low.to_bits();
    }
    let l = &list.list;
    let target = views.target.as_ref();
    // SAFETY: an open list with the heap of the views set; the title's
    // program has just used both views, so their textures are in the states
    // these views need; buffers and target the list keeps.
    unsafe {
        l.SetComputeRootSignature(&pass.root);
        l.SetComputeRoot32BitConstants(0, 18, words.as_ptr().cast(), 0);
        if let Some(depth) = views.depth {
            l.SetComputeRootDescriptorTable(1, depth);
        }
        l.SetComputeRootDescriptorTable(2, views.image);
        l.SetComputeRootUnorderedAccessView(3, pass.cells.GetGPUVirtualAddress());
        l.SetComputeRootUnorderedAccessView(4, pass.points.GetGPUVirtualAddress());
        l.SetComputeRootUnorderedAccessView(5, pass.stats.GetGPUVirtualAddress());
        match role {
            Role::Radiance => {
                // Wait only on the title's radiance UAV before sampling it,
                // then synchronize our private `cells` buffer after `mean`
                // so `Air`, `RayMiss`, and `Resolve` observe `cells[CELLS]`
                // with no UAV RAW/WAW hazard.
                l.ResourceBarrier(&[uav_barrier_of(target)]);
                l.SetPipelineState(&pass.keep);
                l.Dispatch(8, 9, 1);
                l.ResourceBarrier(&[uav_barrier_of(Some(&pass.cells))]);
                l.SetPipelineState(&pass.mean);
                l.Dispatch(1, 1, 1);
                l.ResourceBarrier(&[uav_barrier_of(Some(&pass.cells))]);
                KEPT.store(true, Ordering::Relaxed);
            }
            Role::Air => {
                l.ResourceBarrier(&[uav_barrier_of(target)]);
                l.SetPipelineState(&pass.air);
                l.Dispatch(views.size[0].div_ceil(8), views.size[1].div_ceil(8), views.slices.div_ceil(2));
                l.ResourceBarrier(&[uav_barrier_of(target)]);
            }
            Role::RayMiss => {
                l.ResourceBarrier(&[uav_barrier_of(target)]);
                l.SetPipelineState(&pass.refl);
                l.Dispatch(views.size[0].div_ceil(8), views.size[1].div_ceil(8), 1);
                l.ResourceBarrier(&[uav_barrier_of(target)]);
            }
            Role::Reflect => {
                l.ResourceBarrier(&[uav_barrier_of(target)]);
                l.SetPipelineState(&pass.hold);
                l.Dispatch(views.size[0].div_ceil(8), views.size[1].div_ceil(8), 1);
                l.ResourceBarrier(&[uav_barrier_of(target)]);
            }
            // Handled before dispatch via `clear_march`.
            Role::March => {}
            Role::Resolve => {
                if constant == 0.0 {
                    // `place` and `gather` only read `depth` (SRV) and `cells`,
                    // independent of the title's `light` UAV, so they overlap
                    // with the title's resolve write to `light` (AnyXboxX#475).
                    l.SetPipelineState(&pass.place);
                    l.Dispatch(8, 9, 1);
                    l.ResourceBarrier(&[uav_barrier_of(Some(&pass.cells))]);
                    l.SetPipelineState(&pass.gather);
                    l.Dispatch(20, 12, 1);
                    l.ResourceBarrier(&[uav_barrier_of(Some(&pass.points)), uav_barrier_of(target)]);
                } else {
                    l.ResourceBarrier(&[uav_barrier_of(target)]);
                }
                l.SetPipelineState(&pass.raise);
                l.Dispatch(views.size[0].div_ceil(RAISE_GROUP), views.size[1].div_ceil(RAISE_GROUP), 1);
                l.ResourceBarrier(&[uav_barrier_of(target)]);
            }
        }
    }
    true
}

/// Holds `image` (the gathered in-scatter a reconstruct is about to read:
/// to unordered access, clamped, back) to [`march`] times the slow mean's
/// luminance, hue kept. Sets the list's view heap and replaces its compute
/// root signature and pipeline; false if it recorded nothing.
pub(crate) fn clear_march(list: &HostList, image: This) -> bool {
    use std::sync::atomic::Ordering;
    let (Some(pass), Some(host)) = (pass(), host()) else {
        return false;
    };
    let Some((clamp, views)) = &pass.march else {
        return false;
    };
    if !matches!(set(), Set::Estimate(_)) || !KEPT.load(Ordering::Relaxed) {
        return false;
    }
    let Some(texture) = crate::resource::host_resource(image) else {
        return false;
    };
    // SAFETY: a live resource.
    let d = unsafe { texture.GetDesc() };
    // (R16G16B16A16_FLOAT.)
    if d.Format.0 != 10
        || d.Dimension != D3D12_RESOURCE_DIMENSION_TEXTURE2D
        || d.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_UNORDERED_ACCESS.0 == 0
    {
        static SAID: std::sync::Once = std::sync::Once::new();
        SAID.call_once(|| {
            tracing::warn!(format = d.Format.0, flags = d.Flags.0, width = d.Width, "marched in-scatter: an image that cannot be clamped");
        });
        return false;
    }
    let view = D3D12_UNORDERED_ACCESS_VIEW_DESC {
        Format: d.Format,
        ViewDimension: D3D12_UAV_DIMENSION_TEXTURE2D,
        Anonymous: D3D12_UNORDERED_ACCESS_VIEW_DESC_0 { Texture2D: D3D12_TEX2D_UAV { MipSlice: 0, PlaneSlice: 0 } },
    };
    const WRITTEN: u32 = D3D12_RESOURCE_STATE_UNORDERED_ACCESS.0 as u32;
    const READ: u32 =
        (D3D12_RESOURCE_STATE_NON_PIXEL_SHADER_RESOURCE.0 | D3D12_RESOURCE_STATE_PIXEL_SHADER_RESOURCE.0) as u32;
    crate::list::ensure_state(list, image, WRITTEN);
    list.keep(texture);
    list.keep(&pass.cells);
    list.keep(&pass.points);
    list.keep(&pass.stats);
    let slot = pass.next.fetch_add(1, Ordering::Relaxed) % MARCH_VIEWS;
    let mut words = [0u32; 18];
    words[10] = march().to_bits();
    let l = &list.list;
    uav_barrier(l, None);
    // SAFETY: an open list; a slot of our heap; a live texture the list
    // keeps, in the unordered-access state; buffers the list keeps.
    unsafe {
        let cpu = D3D12_CPU_DESCRIPTOR_HANDLE {
            ptr: views.GetCPUDescriptorHandleForHeapStart().ptr + (slot * pass.step) as usize,
        };
        let gpu = D3D12_GPU_DESCRIPTOR_HANDLE {
            ptr: views.GetGPUDescriptorHandleForHeapStart().ptr + u64::from(slot * pass.step),
        };
        host.device.CreateUnorderedAccessView(texture, None::<&ID3D12Resource>, Some(&view), cpu);
        l.SetDescriptorHeaps(&[Some(views.clone())]);
        l.SetComputeRootSignature(&pass.root);
        l.SetComputeRoot32BitConstants(0, 18, words.as_ptr().cast(), 0);
        l.SetComputeRootDescriptorTable(2, gpu);
        l.SetComputeRootUnorderedAccessView(3, pass.cells.GetGPUVirtualAddress());
        l.SetComputeRootUnorderedAccessView(4, pass.points.GetGPUVirtualAddress());
        l.SetComputeRootUnorderedAccessView(5, pass.stats.GetGPUVirtualAddress());
        l.SetPipelineState(clamp);
        l.Dispatch((d.Width as u32).div_ceil(8), d.Height.div_ceil(8), 1);
    }
    uav_barrier(l, None);
    crate::list::ensure_state(list, image, READ);
    #[cfg(feature = "diagnostics")]
    {
        crate::capture::note_target(image);
        list.note_op(|| format!("ambient March target={image:p}"));
    }
    static TOLD: Mutex<u32> = Mutex::new(0);
    let mut said = TOLD.lock().unwrap_or_else(|e| e.into_inner());
    if *said < 4 {
        *said += 1;
        tracing::info!(width = d.Width, height = d.Height, most = march(), "the marched in-scatter is held to the light of the room");
    }
    true
}

/// Whether the title's diffuse light history is kept clean (`VICTORY_AMBIENT_CLEAN`,
/// on by default; `0`: off). Copies the resolve output before the ambient lift
/// and restores it before the next frame's resolve so camera cuts/turns never
/// leave translucent ghosts (jasonc123456@1fe30dab).
fn clean() -> bool {
    static ON: OnceLock<bool> = OnceLock::new();
    *ON.get_or_init(|| std::env::var_os("VICTORY_AMBIENT_CLEAN").is_none_or(|v| v != "0"))
}

struct KeptHistory {
    title: usize,
    copy: ID3D12Resource,
    frame: u64,
    pending: bool,
}
// SAFETY: D3D12 objects are free-threaded.
unsafe impl Send for KeptHistory {}

static HISTORY: Mutex<Vec<KeptHistory>> = Mutex::new(Vec::new());

/// Evicts any saved diffuse-history entry for a title resource being destroyed
/// so `put_history_back` never dereferences a freed pointer and its VRAM copy
/// is retired safely.
pub(crate) fn released(address: usize) {
    let mut history = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    let mut i = 0;
    while i < history.len() {
        if history[i].title == address {
            let removed = history.swap_remove(i);
            crate::host::retire_resource(removed.copy);
        } else {
            i += 1;
        }
    }
}

fn moved_state(
    list: &HostList,
    resource: crate::object::This,
    state: u32,
) -> Option<(&'static ID3D12Resource, u32)> {
    let (texture, before) = list.transition(resource, state)?;
    let after = crate::host::legal_state(texture, state);
    let before = crate::host::legal_state(texture, before);
    if before != after {
        let barrier = crate::host::transition(
            texture,
            D3D12_RESOURCE_STATES(before as i32),
            D3D12_RESOURCE_STATES(after as i32),
        );
        // SAFETY: an open list; the list's states hold the texture.
        unsafe { list.list.barriers(&[barrier]) };
    }
    Some((texture, before))
}

/// Copies the resolve's diffuse image before our ambient lift writes into it.
pub(crate) fn keep_history(list: &HostList, resource: crate::object::This) {
    const COPY_SOURCE: u32 = 0x800;
    if !clean() || !matches!(set(), Set::Estimate(_)) {
        return;
    }
    let Some(texture) = crate::resource::host_resource(resource) else {
        return;
    };
    let Some(host) = host() else { return };
    // SAFETY: a live resource.
    let desc = unsafe { texture.GetDesc() };
    if desc.Flags.0 & D3D12_RESOURCE_FLAG_ALLOW_SIMULTANEOUS_ACCESS.0 != 0 {
        return;
    }
    let mut history = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    let at = match history.iter().position(|k| k.title == resource as usize) {
        Some(i) => {
            // SAFETY: a live resource.
            let had = unsafe { history[i].copy.GetDesc() };
            if (had.Width, had.Height, had.Format) == (desc.Width, desc.Height, desc.Format) {
                i
            } else {
                let old = history.remove(i);
                crate::host::retire_resource(old.copy);
                usize::MAX
            }
        }
        None => usize::MAX,
    };
    let at = if at == usize::MAX {
        let heap = D3D12_HEAP_PROPERTIES {
            Type: D3D12_HEAP_TYPE_DEFAULT,
            ..Default::default()
        };
        let mut d = desc;
        d.Flags = D3D12_RESOURCE_FLAG_NONE;
        let mut copy: Option<ID3D12Resource> = None;
        // SAFETY: plain creation; stays COPY_SOURCE between uses.
        let made = unsafe {
            host.device.CreateCommittedResource(
                &heap,
                D3D12_HEAP_FLAG_NONE,
                &d,
                D3D12_RESOURCE_STATE_COPY_SOURCE,
                None,
                &mut copy,
            )
        };
        let Some(copy) = made.ok().and(copy) else {
            static TOLD: std::sync::Once = std::sync::Once::new();
            TOLD.call_once(|| {
                tracing::warn!("no copy of the diffuse light history: the stand-in's raise goes into it")
            });
            return;
        };
        tracing::info!(
            width = desc.Width,
            height = desc.Height,
            "the diffuse light's history is kept as the title made it (VICTORY_AMBIENT_CLEAN)"
        );
        history.push(KeptHistory {
            title: resource as usize,
            copy,
            frame: 0,
            pending: false,
        });
        history.len() - 1
    } else {
        at
    };
    let kept = &mut history[at];
    list.keep(&kept.copy);
    let back = moved_state(list, resource, COPY_SOURCE);
    let Some(texture) = back
        .map(|b| b.0)
        .or_else(|| crate::resource::host_resource(resource))
    else {
        return;
    };
    // SAFETY: open list; both textures alive and in matching states.
    unsafe {
        list.list.barriers(&[crate::host::transition(
            &kept.copy,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
            D3D12_RESOURCE_STATE_COPY_DEST,
        )]);
        list.list.CopyResource(&kept.copy, texture);
        list.list.barriers(&[crate::host::transition(
            &kept.copy,
            D3D12_RESOURCE_STATE_COPY_DEST,
            D3D12_RESOURCE_STATE_COPY_SOURCE,
        )]);
    }
    // Always restore out of COPY_SOURCE (UNORDERED_ACCESS = 0x8 if first touch on list).
    let restore = back.map_or(0x8, |(_, before)| before);
    moved_state(list, resource, restore);
    kept.frame = crate::frame::current();
    kept.pending = true;
}

/// Restores clean un-lifted diffuse history before the next frame's resolve dispatch.
pub(crate) fn put_history_back(list: &HostList) {
    const COPY_DEST: u32 = 0x400;
    if !clean() {
        return;
    }
    let now = crate::frame::current();
    let mut history = HISTORY.lock().unwrap_or_else(|e| e.into_inner());
    for kept in history.iter_mut().filter(|k| k.pending && k.frame < now) {
        kept.pending = false;
        if crate::resource::host_resource(kept.title as crate::object::This).is_none() {
            continue;
        }
        list.keep(&kept.copy);
        let back = moved_state(list, kept.title as crate::object::This, COPY_DEST);
        let Some(texture) = back
            .map(|b| b.0)
            .or_else(|| crate::resource::host_resource(kept.title as crate::object::This))
        else {
            continue;
        };
        // SAFETY: open list; copy in COPY_SOURCE, target in COPY_DEST.
        unsafe { list.list.CopyResource(texture, &kept.copy) };
        // Always leave the target out of COPY_DEST before Dispatch runs:
        // restore its previous state when known, or NON_PIXEL_SHADER_RESOURCE
        // (0x40) if this was its first touch in the list.
        let restore = back.map_or(0x40, |(_, before)| before);
        moved_state(list, kept.title as crate::object::This, restore);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings() {
        assert_eq!(parse(""), Set::Off);
        assert_eq!(parse("off"), Set::Off);
        assert_eq!(parse("0"), Set::Off);
        assert_eq!(parse("0.8"), Set::Estimate(0.8));
        assert_eq!(parse("constant:0.3"), Set::Constant(0.3));
        assert_eq!(parse("constant:nan"), Set::Off);
        assert_eq!(multiple(""), 0.0);
        assert_eq!(multiple("0"), 0.0);
        assert_eq!(multiple("-1"), 0.0);
        assert_eq!(multiple("nan"), 0.0);
        assert_eq!(multiple(" 1.5 "), 1.5);
    }

    #[test]
    fn roles_by_name() {
        assert_eq!(role(b"..CS_ScreenProbes_TemporalResolve_a911b49c_Wrapped.."), Some(Role::Resolve));
        assert_eq!(role(b"..CS_CacheScreenSpaceRadiance_eed2fc35.."), Some(Role::Radiance));
        assert_eq!(role(b"CS_AccumulateLocalLightInscatteringVolume_7e2f5eef"), Some(Role::Air));
        assert_eq!(role(b"..CS_RayMiss_de6b91a6_Wrapped.."), Some(Role::RayMiss));
        assert_eq!(role(b"..CS_RayMiss_Volumetrics_12345678.."), None);
        assert_eq!(role(b"..CS_UpdateHistory_e877ef6e_Wrapped.."), Some(Role::Reflect));
        assert_eq!(role(b"..CS_ReconstructRaymarchClassifiedFull_2d384eab_Wrapped.."), Some(Role::March));
        assert_eq!(role(b"CS_ReconstructRaymarchClassifiedNoAlpha_2d384eab"), Some(Role::March));
        assert_eq!(role(b"CS_ReconstructRaymarch_High_66f3d2a0"), None);
        assert_eq!(role(b"CS_TemporalResolve"), None);
    }

    /// The passes compile with the system's compiler and the host takes them.
    #[test]
    fn the_passes_build() {
        let Some(host) = host() else {
            return;
        };
        make(&host.device, |bytes| host.uav_buffer(bytes)).expect("the passes");
        crate::host::compile(&format!("#define MARCH\n{SOURCE}"), s!("clear_march"), s!("cs_5_1"))
            .expect("the march clamp");
    }
}
