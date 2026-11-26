# VIctory-RTX (NVIDIA RTX / Wave32 Fork)

An NVIDIA-focused fork of [VIctory](https://github.com/Marvisak/VIctory) built on top of
[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) (`c9c932c`),
a compatibility and porting layer that runs an Xbox Series title natively on Windows 11
from files supplied by the user, translating D3D12.X and GFX10 shaders to Direct3D 12
Shader Model 6.6 on **Wave32 NVIDIA GPUs** (tuned on RTX 4090 + hybrid Intel Core i9).

**This repository contains no game files, code or assets.** You must supply your own
dump outside this folder (`victory.toml`). Every commit is audited by `cargo xtask policy`
([docs/asset-policy.md](docs/asset-policy.md)). It is not affiliated with or endorsed by
the game's developer or publisher.

---

## Goal

**Reach a stable 30+ FPS native (60+ FPS visual with Smooth Motion) without ray tracing**
at clean visual quality (fast texture streaming, natural indoor/outdoor indirect lighting,
no face/eye/border glitches), then push higher frame rates and optionally enable real
Wave32 ray tracing (`-RayTracing`).

---

## Current Status (2026-11-26 — branch `nvidia-rtx4090`)

Reference test hardware: **Windows 11, Intel Core i9-12900KF (8P/16T + 8E/8T), 64 GB RAM,
NVIDIA GeForce RTX 4090 (24 GB GDDR6X, 32 GiB Resizable BAR, Wave32, 1M view heap)**,
Series X profile (ray tracing off).

| Metric | Measured Status |
|---|---|
| **Menu / UI** | **60 FPS** (`16.7 ms/frame`) with native 60 FPS unlock (`-Fps 60`, game clock stays `1.00x`) |
| **Series X `VICTORY_OUTPUT=1920x1080` + Ports 1, 2, 5 (NVIDIA in-place), 6, 7** | **22.6 FPS median (`44.3 ms/frame`, `31.5 ms` preparing), 26.8 FPS peak (`37.3 ms/frame`)** in real 3D gameplay (up from `16–17 FPS`, **`+60%` FPS**) |
| **Exclusive NVIDIA 1M-Heap Port 5 (`VICTORY_PORT_CBV=1` in-place)** | **`16,750` table CBVs/frame (`6.7 MB/frame`) read in place** by the GPU (`0` unaligned fallbacks), slashing `card_steps` fills from `~12,000` → **`~175/frame`** |
| **Port 2 (`12B` diff) + Port 6 (Guard) + Port 7 (Journal)** | Port 2 **`99.9993%`** accuracy (`0.50–1.48` misses/frame across `70K` slots/frame); Port 6 fault wait **`0.42–0.88 ms`**; Port 7 heap-sync wait **`0.19–0.31 ms`** & journal lock wait **`0.09 ms`** |
| **RTX 4090 `-GpuTiming` Breakdown (`diag` build)** | **`33.7 ms`** noted GPU commands (`dispatch 17.5 ms`, `indirect 11.8 ms`, `mesh 2.3 ms`, `alias 1.2 ms`), **`10.3 ms`** preludes (`65` lists), **`4.0 ms`** compares (`20` lists) — top 3 compute passes alone take **`7.42 ms`** (`55679a0547649d0b` `3.47 ms`, `17af15c4015cb4b6` `2.40 ms`, `89d08e561897d4e5` `1.55 ms`) |
| **Wave32 Shader Coverage** | **`25,784` shaders translated to Wave32 (`99.98%`)** — only **`6`** Wave64-only shaders remain (195 unsupported GS containers) |
| **Pre-warmed D3D12 Pipeline Library** | **`host-pipelines.bin` (`1.34 GB`, `2,564` PSOs) pre-warmed with NVIDIA in-place `VICTORY_PORT_CBV=1`**, **`0` driver refusals (`0x80070057`)** |
| **RTX 4090 `CARD_COMPARE` Microbenchmark** | **`17.31 ms → 14.40 ms` (`+20.3%`, `705 → 848 GB/s`)** via 6-step parallel tree reduction + VRAM `differs_gpu` |
| **Visual Quality & Active Issue** | Faces animate, `x=0` dotted border fixed, cornea/sclera sky-leak fixed (`Role::RayMiss` + crease clamp), D3D12 errors `538` & `646` fixed, Wave32 LDS atomic ordering fixed, zero indirect draw drops. **Open issue:** very dark interior/cutscene lighting under the new Ports 1–7 + 1080p stack (investigating next — test `-NoPortCbv` to isolate) |
| **Memory** | `~15–17 GB` dedicated VRAM, `~40–46 GB` private commit (**64 GB RAM strongly recommended** for now) |

---

## What Has Been Implemented in This Fork

### 1. Renderer Ports 1, 2, 5, 6, 7 & Native Output Scaling (Merged from `avatarmanz@c9c932c`)

| Switch | What it does |
|---|---|
| `VICTORY_PORT_CONSTANTS=1` | The title's constants heap is read by the card in place: no uploads, no fills (`0` forgotten regions) |
| `VICTORY_PORT_DESCRIPTORS=1` | 11 byte-verified Scarlett descriptor stores report written slots directly + 12-byte key diff (`changed_key12`) & `copy_simple` mark ordering (**`99.9993%` hit rate**, `0.08` misses/frame) |
| `VICTORY_PORT_CBV=1` | **NVIDIA 1M-heap in-place Port 5 (`7b908a2`):** translated shaders read table/heap constant buffers in place as raw byte-address SRVs (`raw_views=0`, no 4M twin heap needed) — **`16,750` CBVs/frame (`6.7 MB`) zero-copy**, `card_steps` fills `~12,000 → ~175/frame` |
| `VICTORY_PORT_GUARD=1` | Shared write-guard lock with per-64 KiB block claims + parallel per-list refresh planning + coalesced `VirtualProtect` (`-2 ms`) |
| `VICTORY_PORT_JOURNAL=1` | Early heap sync triggered at title submit + 64-sharded journal locks (`-1 to -3 ms`) |
| `VICTORY_OUTPUT=1920x1080` | Native title output scaling on Series X (jumps backbuffer pool `int3`, saves `~16 ms` GPU; `-Full1440p` / `-Full4K` available) |
| `VICTORY_UPSCALER=6` | Forces the title's own temporal upscaler instead of FidelityFX (avoids unpatched FidelityFX descriptor writes under Port 2) |

### 2. NVIDIA Wave32 Shader Translator & Pipeline Stability
- **Wave32 guarded joins, active-loop lowering & LDS atomic ordering** (ported from `codec-d/cool_fox`):
  per-instruction `lanes.guards` tracking in `victory-shader/src/resource.rs`,
  `DIVERGENT_MASK_EXCHANGE` / `scc_some`, `active_loop_only` single-latch loop widening,
  commuting-only LDS atomic region check (`lds_meetings`), and clean pre-driver rejection of
  `wave == Some(64)` shaders on Wave32 hosts.
- **Wave32 RT / BVH / tiled cascade shadows** + `VICTORY_BVH_NO_HIT=1` + `VICTORY_WAVE_WIDE=last`
  + `VICTORY_NAN_SCRUB=1` + `MED3|LEGACY` portable float paths + Tier-A pure-BVH auto-skip.
- **D3D12 correctness fixes** (ported from `trustno1-coder/VIctory-NVIDIA` & `cool_fox`):
  separate `Compare` submission for `COMMON` state decay (D3D12 error `538`),
  strict `Role::Resolve` depth SRV binding & dimension match (D3D12 error `646`),
  `TextureQueue::reschedule` & `sweep_batch` wrap-around.
- **Offline pre-translation & pipeline library warming** (`victory shaders` + `victory warm --used --save-library`).
  *Critical:* translator env vars (`VICTORY_SHADER_OPT=63`, `VICTORY_WAVE_WIDE=last`,
  `VICTORY_NAN_SCRUB=1`, `VICTORY_BVH_NO_HIT=1`, `VICTORY_PORT_CBV=1`) must match between `shaders`/`warm` and live runs.

### 3. GPU Bandwidth, Shared-Memory Reduction & Synchronization
- **Resizable BAR VRAM Windows (`VICTORY_WINDOWS_BAR_MB=4096`, `VICTORY_WINDOW_KB=4096`):**
  moves loose vertex/index/mesh windows off 25 GB/s System RAM (`L0`) into 1,008 GB/s GDDR6X
  (`D3D12_HEAP_TYPE_GPU_UPLOAD`), automatically capping at 80% VRAM budget while keeping
  window count compact (`4 MiB` windows avoid loose-window overlap overhead).
- **Plain OS-RAM `Card::staging` (`shadow.rs`):** when `VICTORY_SEND_OWN=1` (default) and
  `upcheck` is off, carded buffers' CPU sides use demand-paged `VirtualAlloc` instead of
  D3D12 `L0` `shared_buffer`, saving **~1.1 GB of WDDM Shared GPU Memory** and ~3,700
  D3D12 resource/residency objects.
- **VRAM `differs_gpu` + 6-step tree reduction in `CARD_COMPARE` (`host.rs`, `host/torn.rs`):**
  removes ~12,000 uncoalesced 4-byte PCIe UAV stores/frame in favor of one VRAM-to-RAM
  `CopyBufferRegion` per submission, and speeds up hashing by `+20.3%` on RTX 4090 with
  bit-exact CPU parity (`torn::hash`).
- **Coalesced `CARD_COMPARE` spans, `CopyBufferRegion`, and `VICTORY_JOIN_BARRIERS=1`**,
  plus scoped resource UAV barriers inside `ambient.rs` and removal of redundant cross-queue waits.

### 4. CPU Frame Preparation, Descriptors, Textures & Visual Fixes
- **NVIDIA 1M View Heap + O(1) `u64` bitmap dynamic slab allocator (`descriptor/host.rs`):**
  lazy 64-slot per-list slabs, batched `release_dynamic_many`, and immediate `tables_done()`
  on flight landing (`host.rs`) with zero refused descriptor tables.
- **`WaitOnAddress` / `WakeByAddressAll` blocking fence waits (`fence.rs`)** and
  `ahead::drain()` before skin-bake uploads (`texture/ahead.rs`, `draw.rs`).
- **O(dirty) `DIRTY_CARDED` tracking (`shadow.rs`)** + `writes_with_kind` view-memory separation
  + `!flying()` `mapped()` gating + `VICTORY_LAND_THREADS=6`.
- **Ambient light stand-in (`ambient.rs`):** `Role::RayMiss` (`CS_RayMiss_*`) specular sky
  cap preventing glowing eyes indoors, `x=0` border normal/AO tap fix, corner clamp floor
  (`0.15 * level * room_luma`), strict `Role::Resolve` depth SRV validation (D3D12 error `646`),
  and time-based settling (`VICTORY_AMBIENT_SETTLE`).
- **Safe internal render scale guard (`play-nvidia.ps1`):** clamps `-Scale <= 0.667` on
  `-Full4K` / `-SeriesS` to prevent RT allocator pool overflow (`int 3` at `+0x2219650`),
  while allowing up to `1.0` at `1920x1080` / `2560x1440` `VICTORY_OUTPUT`.

---

## Known Bottlenecks & Where Collaborators Can Help Most

Now that we reached **22.6–26.8 FPS (`37.3–44.3 ms/frame`)** with full `-GpuTiming` profiling working on RTX 4090, these are the highest-leverage items to cross **stable 30–60 FPS** with clean visuals:

1. **Fix Dark Interior / Cutscene Lighting Regression (Top Visual Priority):**
   - Cutscenes/dark interiors appear overly dark (only emissive lights bright) under the new Ports 1–7 + 1080p stack even though `ambient.rs` logs `Radiance`, `Resolve`, `Air`, `RayMiss` active.
   - **Task:** A/B isolate `-NoPortCbv` vs `-NoPortDescriptors` vs `-Full4K` to determine whether an in-place raw CBV read (`VICTORY_PORT_CBV=1`), a missed descriptor write, or `ambient.rs` clamping is suppressing diffuse/indirect lighting.
2. **Optimize Top RTX 4090 GPU Compute Passes (`-GpuTiming` Measured):**
   - Full `-GpuTiming` breakdown shows **`33.7 ms`** in noted GPU commands (`dispatch 17.5 ms`, `indirect 11.8 ms`, `mesh 2.3 ms`, `alias 1.2 ms`), plus **`10.3 ms`** in copy preludes (`65` lists) and **`4.0 ms`** in `CARD_COMPARE` (`20` lists).
   - Just **3 compute shaders take `7.42 ms` (`22%` of GPU command time):** `55679a0547649d0b` (`3.47 ms`), `17af15c4015cb4b6` (`2.40 ms`), and `89d08e561897d4e5` (`1.55 ms`).
   - **Task:** Identify & optimize/skip safe RT-off dead passes among the top compute dispatches, and reduce the **`24.9 ms`** gap between noted GPU commands and total list wall time (global NULL UAV barriers + list submission overhead across ~210 lists/frame).
3. **CPU `victory-submit` Thread (`~31.5 ms preparing/frame`):**
   - Now down from `56.6 ms → 31.5 ms` thanks to Ports 1–7 + 12B `PORT_DESCRIPTORS`, **which makes `victory-submit` the main ceiling for >31 FPS** (`copies 7.8–8.7 ms / 51–66 MB`, `host copies` `24 MB sent up` + `45 MB fetched back` with `~8–9 ms` card waits).
   - **Task:** Cut the `104 MB → 51–66 MB` remaining CPU buffer copies and `45 MB` read-backs (`card_before` / `land` / `VICTORY_SEND_BAR=1` A/B on RTX 4090).
4. **Hook Remaining 2–3 Unpatched Descriptor Writers (`VICTORY_PORT_DESCRIPTORS=1`):**
   - 12B diff already delivers **`99.9993%`** accuracy (`0.50–1.48` misses/frame across `70K` slots/frame).
   - Remaining tiny trickle of missed writers: `title+0x18849c`, `title+0x4e9b4e2`, `title+0x2474afc`, plus an external DLL store (`+0x23cd9f`). Hooking the title sites will push Port 2 to `100.00%` (`0` misses).
5. **Remaining 6 Wave64-Only Shaders (`02384a60fda6f2d0` = 1.0 drop/frame):**
   - Only 1 non-RT compute shader (`02384a60fda6f2d0`) drops during gameplay (all other ~353 drops/frame are RT BVH builder stages intentionally off without `-RayTracing`).
6. **Memory Reduction for 32 GB RAM Systems:**
   - Cut idle scratch/staging/list pools and tune `VICTORY_RESIDENCY=1` (`residency.rs`).

---

## How to Build, Pre-Warm & Run (Windows 11 + NVIDIA RTX)

### Requirements
- Windows 10/11 x64, AVX2 CPU, Direct3D 12 GPU with Shader Model 6.6 + Mesh Shaders (RTX 30/40/50 series).
- Rust toolchain (pinned by `rust-toolchain.toml`) + MSVC C++ Build Tools.
- Your own title dump placed **outside** this repository.

### 1. Configure & Build
```powershell
copy victory.example.toml victory.toml
# Edit victory.toml -> set game_dir = 'D:\path\to\your\dump' (outside this repo)

# Fast release build (for gameplay)
cargo build --locked --release -p victory

# Diagnostics build (for -Panel, -GpuTiming, VICTORY_TIMELINE)
cargo build --locked --profile diag -p victory --features diagnostics
```

### 2. Pre-Translate Shaders & Warm Pipeline Library (Once per Translator / Driver Update)
Use the **exact** translator switches that `scripts\play-nvidia.ps1` uses so the cache key matches 100%:
```powershell
$env:VICTORY_SHADER_OPT = '63'
$env:VICTORY_WAVE_WIDE  = 'last'
$env:VICTORY_NAN_SCRUB  = '1'
$env:VICTORY_BVH_NO_HIT = '1'
$env:VICTORY_PORT_CBV   = '1'

target\release\victory.exe shaders --jobs 16 <path\to\your\dump>
target\release\victory.exe warm --used --jobs 16 --save-library
```

### 3. Run via `scripts\play-nvidia.ps1` (Normal, Non-Elevated PowerShell)
`play-nvidia.ps1` auto-detects the executable from `victory.toml` (or pass `-Exe <path>`):

```powershell
# Series X 1080p output + Ports 1, 2, 5, 6, 7 + ReBAR 4 GB VRAM windows + 60 FPS unlock (recommended)
.\scripts\play-nvidia.ps1

# Series X 1080p native internal (scale 1.0) + Ports 1, 2, 5, 6, 7
.\scripts\play-nvidia.ps1 -Scale 1.0

# Series X 1440p output (1706x960 internal -> 2560x1440) + Ports 1, 2, 5, 6, 7
.\scripts\play-nvidia.ps1 -Full1440p

# Series X 4K output (2560x1440 internal, original baseline resolution)
.\scripts\play-nvidia.ps1 -Full4K

# Series X 1080p + NVIDIA Smooth Motion (2x visual FPS)
.\scripts\play-nvidia.ps1 -SmoothMotion

# 15s GPU per-pass & top-40 shader profiling (auto-uses diag build with diagnostics feature)
.\scripts\play-nvidia.ps1 -GpuTiming

# Safe fallback mode (disables port patches, ReBAR windows, and thin lists)
.\scripts\play-nvidia.ps1 -Safe
```

*(Other useful switches: `-SendBar`, `-TexStore`, `-SeriesS`, `-RayTracing`, `-Panel`, `-NoPortCbv`, `-NoPortDescriptors`, `-NoPortGuard`, `-NoPortJournal`, `-NoPortConstants`, `-Residency`.)*

### Verification Before Committing
```powershell
cargo test --locked --release -p victory-shader --lib -- divergent_lds_atomics
cargo test --locked --release -p victory-gpu --lib -- torn ambient warp_card_compare host_ram_card_staging changed_key12 room_only_wakes idle_sweep_restarts
cargo run --locked -p xtask -- policy
```

---

## Credits

Built on the combined work of the VIctory open-source ecosystem (commit history preserves upstream authors):
- **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)** — core loader, HLE runtime, D3D12 backend, and GFX10-to-DXIL recompiler.
- **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)** — direct upstream base (`c9c932c`: `VICTORY_THIN`, `VICTORY_ASYNC_SUBMIT`, `VICTORY_RELAY`, `VICTORY_LAND`, `VICTORY_CARD_STEPS`, Renderer Ports 1, 2, 5, 6, 7, `VICTORY_OUTPUT` / `VICTORY_UPSCALER`, `VICTORY_AMBIENT_SETTLE`, `VICTORY_TEXTURE_STORE`, FPS unlock, frame record/replay).
- **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)** — face animation, legacy FMA reflections, 32-bit int typed views, target format variants, texture read-back.
- **[codec-d/cool_fox](https://github.com/codec-d/cool_fox)** — Wave32 guarded joins & active-loop lowering, immediate table release, `WaitOnAddress` fence waits, bake upload drain, residency budgets.
- **[trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA)**, **DexNom/VIctory**, **Belobug/VIctory-Belobug-AMD**, **FQY55kai/ZJOU-VICTORY**, **myokex/victory-amd**, **Geckomacabre**, **cxy0072019-sketch**, and **romainhedouin**.

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE) and [docs/asset-policy.md](docs/asset-policy.md).
