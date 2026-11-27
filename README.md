# VIctory-RTX-Mc3yohan (NVIDIA RTX / Wave32 Fork)

> Maintained by **Mc3yohan** ([github.com/Mc3yohan](https://github.com/Mc3yohan)) · GPL-3.0-or-later

An NVIDIA-focused fork of [Marvisak/VIctory](https://github.com/Marvisak/VIctory) built on top of
[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) (`c9c932c`) and
selected community NVIDIA/Wave32 fixes (full provenance in [Credits & Attribution](#credits--attribution)),
translating D3D12.X and GFX10 shaders to Direct3D 12 Shader Model 6.6 on **Wave32 NVIDIA GPUs**
(tuned on RTX 4090 + hybrid Intel Core i9).

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
| **Series X `VICTORY_OUTPUT=1920x1080` + Ports 1, 2, 5 (NVIDIA in-place), 6, 7 + ThinLTO** | **25.7 FPS median (`38.9 ms/frame`), 31–34.7 FPS peak (`28.8–32.0 ms/frame`)**, `17.0 GB` VRAM with `0` shared-memory spill (`GPU 73–83%` busy, `0.0 ms` GPU wait — now CPU-bound in `victory-submit`) |
| **Native PC FidelityFX 4K (`-Fsr4K`)** | **Verified live on RTX 4090 with signed AMD FSR `3.1.6`** (`FSR SDK v2.4.0`, pixel-exact `fidelityfx` hardware test `1/1 ok` + byte-verified `quality`/`balanced`/`performance` 4K profiles) |
| **Exclusive NVIDIA 1M-Heap Port 5 (`VICTORY_PORT_CBV=1` in-place)** | **`8,700–16,750` table CBVs/frame (`3.7–6.7 MB/frame`) read in place** (`0` unaligned fallbacks), slashing `card_steps` fills from `~12,000` → **`~145/frame`**; with `own.between` SRV L1TEX state barrier + aligned `raw_dwords` (`SHADER_OPT=63`) + collapsed `smem` dispatch |
| **Top 3 Compute Shaders (`7.42 ms` / 22% GPU time)** | Verified `3/3` with `dxil.dll` + RTX 4090 PSO: `CS_Tree_BatchCull` (`3.47 ms`) & `CS_Tree_BatchCull_ImposterOnly` (`1.55 ms`) promoted from `WAVE64-WIDE` → **`local waves (idiom)`** via disjoint commuting LDS atomics; `CS_Prop_BatchCull` (`2.40 ms`) keeps native early-outs; cut RTX 4090 GPU busy time by `~10 ms/frame` |
| **Port 2 (`12B` diff) + Port 6 (Guard) + Port 7 (Journal)** | Port 2 **`99.9993%`** accuracy (`0.50–1.48` misses/frame across `55–76K` slots/frame); Port 6 fault wait **`0.42–1.30 ms`**; Port 7 heap-sync wait **`0.15–0.31 ms`** & journal lock wait **`0.09 ms`** |
| **Wave32 Shader Coverage (`TRANSLATOR_VERSION = 2`)** | **`25,784` shaders translated to Wave32 (`99.98%`)** — only **`6`** Wave64-only shaders remain (195 unsupported GS containers); **`2,566 / 2,567`** used pipelines pre-warmed (`0` driver refusals `0x80070057`, `1.58 GB` `host-pipelines.bin`) |
| **RTX 4090 `CARD_COMPARE` Microbenchmark** | **`17.31 ms → 14.40 ms` (`+20.3%`, `705 → 848 GB/s`)** via 6-step parallel tree reduction + VRAM `differs_gpu` |
| **Visual Quality, Input & UX Upgrades** | Fixed dark interiors / downward-camera shadows by removing `TIER_A_NO_HIT` auto-skip (restoring 11 active mixed/screen-space shadow shaders), fixing Port 5 (`VICTORY_CONSTANT_MARKS=1` guard on `rawBufferLoad`, refreshing `View::Constant` title slots in `buffer_range`/`note_buffer`, coherent `SLOT_STRIDES` upload buffer), and making Port 2 opt-in (`-PortDescriptors`) to stop permanent NPC face-bake corruption from unpatched descriptor writers; added **`mimalloc` global Rust allocator** (`Dastano` PR #11: per-thread caches cutting `victory-submit` Windows heap contention by ~3.3 ms CPU / `+2–2.7 FPS`), explicit `VICTORY_ASYNC_SUBMIT=1` + GPU-aware `VICTORY_THIN` (auto ON on RTX 30/40 for 15–20 ms recording offload, auto OFF on RTX 50 Blackwell to prevent whiteout/blur, `-Thin`/`-NoThin` overrides), `VICTORY_AMBIENT_MOST=2.0` (eliminates indoor window glare), `avatarmanz` `VICTORY_PORT_CONSTANTS_ORDER` (fixes geometry pop-in & lamp-lit rooms) + `VICTORY_AMBIENT_RT`; `fxceless` `#1336`/`#1324`/`#1320`/`#1353`/`#1312` (aliased texture `Held` pin, `HostHeap::NEXT_ID`, `submit_split` slot release, panic-safe pipeline workers, `VICTORY_PIPELINE_COMPUTE_WAIT=1` so one-shot scene compute dispatches never drop); `Indy2l` `VICTORY_TEXBACK_ASYNC=1` (dedicated texture-readback writer thread eliminating 124–416 ms streaming hitches); opt-in **`-Kbm` (`VICTORY_KBM_GTA=1`)** keyboard + mouse look (`F2`/`~` locks & auto-hides cursor); upgraded **Release-mode F3 telemetry HUD** (`LOCAL / BUDGET / DED CAP / SHARED GiB`); native PC **FidelityFX FSR 3.1.x / 4.x bridge**; **`--nvidia-preset`** for `AmericasLauncher` |
| **Memory Hygiene & Multi-GPU Auto-Tiers** | **4 KiB small-texture placement on non-AMD GPUs (`Vice-Tay` #45: saves ~3.4 GB VRAM + 3.2 GB RAM)**; **placed-view padding zeroing + `VICTORY_OWNERLESS_VIEWS` cap (`Vice-Tay` #27: eliminates 16.7 GB long-session RAM leak)**; `STAGING` upload pool capped at `512 MiB` + `PRELUDE_IDLE` recycling + `HostList::close` `E_OUTOFMEMORY` retry (`WASDeKeys`); `shadow::register` overwritten-copy retirement (`cool_fox`); **automatic NVIDIA VRAM tier detection in `play-nvidia.ps1`** (`24GB+`: `WindowsBarMb=4096, MipDrop=0`; `16GB`: `WindowsBarMb=3584, CardBuffersMb=4096, MipDrop=1`; `12GB`: `WindowsBarMb=2048, CardBuffersMb=2560, MipDrop=1`; `8GB`: `WindowsBarMb=1536, CardBuffersMinKb=1024, MipDrop=2, auto -SeriesS`) |

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

Now that we reached **25.7 FPS median / 31–34.7 FPS peak** on RTX 4090 (with the GPU `~73–83%` busy and `0.0 ms` GPU wait!), these are the highest-leverage items to push toward **45–60 FPS stable on 4090** and **smooth 20–30 FPS on 8–16 GB RTX cards**:

1. **CPU `victory-submit` Thread (`~30.6 ms preparing/frame` — Primary Ceiling on RTX 4090):**
   - Now that GPU time dropped below `30 ms` on RTX 4090 (`9–13 ms` GPU idle waiting for CPU), `victory-submit` is the #1 ceiling above 30 FPS (`copies ~7–9 ms / 50 MB`, `host copies` `28–31 MB sent up` + `15–18 ms` copyback, `VICTORY_THIN` `38K–49K` calls across 8 threads).
   - **Task:** Cut remaining CPU buffer copies, reduce `land` / `card_before` copyback overhead, and profile `-UpQueue` / `VICTORY_SEND_BAR=1` A/B on 4090/5070 Ti.
2. **8 GB & 12–16 GB VRAM Optimization (RTX 3070 / 4060 / 5050 / 5070 Ti):**
   - Multi-GPU logs (`RTX 5070 Ti 16 GB`, `RTX 5050 Laptop 8 GB`) showed that spilling mesh/vertex windows out of GDDR into WDDM Shared GPU Memory (PCIe) causes a 5x–10x GPU stall (`106–847 ms`).
   - `play-nvidia.ps1` now auto-scales `WindowsBarMb`, `CardBuffersMb`, `VICTORY_TEXTURE_MIP_DROP`, and `-SeriesS` by VRAM tier; further work on smarter texture eviction (`VICTORY_RESIDENCY=1`) and lower commit usage for 24–32 GB RAM PCs is very welcome.
3. **Native NVIDIA DLSS Super Resolution Backend (Next Upscaler Milestone):**
   - Our `fidelityfx.rs` bridge already intercepts all 7 D3D12 inputs (`color`, `depth`, `motion_vectors`, `exposure`, `reactive_mask`, `transparency_mask`, `output`) plus `jitter`, `motion_scale`, and `reset`, and runs signed AMD **FSR `3.1.6`** live on RTX 4090.
   - **Task:** Add an optional NVIDIA Streamline / NGX DLSS backend behind the same hook for Tensor-Core **DLSS 3.8 / DLSS 4 Transformer** upscaling on RTX 30/40/50 GPUs.
4. **Hook Remaining 2–3 Unpatched Descriptor Writers (`VICTORY_PORT_DESCRIPTORS=1`):**
   - 12B diff delivers **`99.9993%`** accuracy (`0.50–1.48` misses/frame across `55–76K` slots/frame).
   - Remaining tiny trickle of missed writers: `title+0x18849c`, `title+0x4e9b4e2`, `title+0x2474afc`, plus an external DLL store (`+0x23cd9f`). Hooking them pushes Port 2 to `100.00%` (`0` misses).
5. **Remaining 6 Wave64-Only Shaders (`02384a60fda6f2d0` = 1.0 drop/frame):**
   - Only 1 non-RT compute shader (`02384a60fda6f2d0`) drops during gameplay (all other ~320–350 drops/frame are RT BVH builder stages intentionally off without `-RayTracing`).

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
Pass `--nvidia-preset` so `shaders`, `warm`, and `run` share the exact same translator keys (`VICTORY_SHADER_OPT=63`, `VICTORY_WAVE_WIDE=last`, `VICTORY_NAN_SCRUB=1`, `VICTORY_BVH_NO_HIT=1`, `VICTORY_PORT_CBV=1`, `VICTORY_CONSTANT_MARKS=1`):
```powershell
target\release\victory.exe shaders --nvidia-preset --jobs 16 <path\to\your\dump>
target\release\victory.exe warm --nvidia-preset --used --jobs 16 --save-library
```

### 3. Run via `scripts\play-nvidia.ps1` (Normal, Non-Elevated PowerShell)
`play-nvidia.ps1` auto-detects the executable from `victory.toml` (or pass `-Exe <path>`) and **automatically tunes VRAM tier (`24GB+` / `16GB` / `12GB` / `8GB`)** via `nvidia-smi`. Press **`F3`** anytime (even in ThinLTO `--release` builds!) to toggle the live telemetry HUD:

```powershell
# Recommended pure-NVIDIA 1080p (720p internal -> 1080p title upscaler, NO FSR, auto VRAM tier)
.\scripts\play-nvidia.ps1

# Sharpest native 1080p (1920x1080 internal -> 1080p output, NO FSR)
.\scripts\play-nvidia.ps1 -Scale 1.0

# Lightest native 720p (1280x720 internal -> 720p output, NO FSR)
.\scripts\play-nvidia.ps1 -Full720p -Scale 1.0

# Enable PC keyboard & mouse look (F2 or ` toggles mouse-look + auto-hides cursor; Esc/P unlocks)
.\scripts\play-nvidia.ps1 -Kbm
.\scripts\play-nvidia.ps1 -Kbm -MouseSens 1.25

# Optional: force a specific VRAM/mip tuning (e.g., testing 8-16 GB cards)
.\scripts\play-nvidia.ps1 -WindowsBarMb 3584 -MipDrop 1
.\scripts\play-nvidia.ps1 -SeriesS -WindowsBarMb 1536 -CardBuffersMinKb 1024 -MipDrop 2

# Native PC FidelityFX 4K upscaling (FSR 3.1.6 on NVIDIA RTX / FSR 4.x on RDNA 4)
# First place AMD's official signed `amd_fidelityfx_upscaler_dx12.dll` from
# https://github.com/GPUOpen-LibrariesAndSDKs/FidelityFX-SDK (Kits/FidelityFX/signedbin)
# into %LOCALAPPDATA%\victory\fidelityfx (or set fidelityfx_dir in victory.toml):
.\scripts\play-nvidia.ps1 -Fsr4K performance   # 1080p internal -> 4K output
.\scripts\play-nvidia.ps1 -Fsr4K balanced      # 1270p internal -> 4K output
.\scripts\play-nvidia.ps1 -Fsr4K quality       # 1440p internal -> 4K output

# Series X 1080p + NVIDIA Smooth Motion (2x visual FPS)
.\scripts\play-nvidia.ps1 -SmoothMotion

# 15s GPU per-pass & top-40 shader profiling (auto-uses diag build with diagnostics feature)
.\scripts\play-nvidia.ps1 -GpuTiming

# Safe fallback mode (disables port patches, ReBAR windows, and thin lists)
.\scripts\play-nvidia.ps1 -Safe
```

*(Other useful switches: `-Panel`, `-Kbm`, `-MouseSens <f>`, `-MouseInvertY`, `-AmbientRt <least,most[,level]>`, `-Thin` / `-NoThin` (auto ON on RTX 30/40, auto OFF on RTX 50 Blackwell), `-PortDescriptors` (opt-in), `-MipDrop <0..3>`, `-CardBuffersMinKb <kb>`, `-SeriesX`, `-SeriesS`, `-Full720p`, `-Scale 1.0`, `-Full1440p`, `-Full4K`, `-SendBar`, `-TexStore`, `-RayTracing`, `-NoPortCbv`, `-NoPortGuard`, `-NoPortJournal`, `-NoPortConstants`, `-Residency`. Logs are automatically saved to `%LOCALAPPDATA%\victory\data\nvidia-run-*.log`.)*

### 4. Integration with `AmericasLauncher` (`Data\runners.json`)
`victory.exe` supports `--nvidia-preset` on `shaders`, `warm`, and `run` so launchers without a per-runner environment block (such as [isaac-btw/AmericasLauncher](https://github.com/isaac-btw/AmericasLauncher)) get full RTX optimization while preserving the Series X / Series S console selector:
```json
{
  "id": "victory-rtx-mc3yohan",
  "name": "VIctory-RTX-Mc3yohan (NVIDIA RTX)",
  "gitUrl": "https://github.com/Mc3yohan/VIctory-RTX-Mc3yohan.git",
  "buildCommands": [
    "rustup show active-toolchain || rustup toolchain install",
    "cargo build --locked --release -p victory"
  ],
  "exe": "target\\release\\victory.exe",
  "preLaunchArgs": ["shaders --nvidia-preset \"{gameFolder}\""],
  "launchArgs": "run \"{gameExe}\" --execute --nvidia-preset",
  "notes": "Best with Nvidia (RTX / Wave32 / ReBAR / Ports 1-7)"
}
```

### Verification Before Committing
```powershell
cargo test --locked --release -p victory-shader --lib -- divergent_lds_atomics
cargo test --locked --release -p victory-gpu --lib -- torn ambient warp_card_compare host_ram_card_staging changed_key12 room_only_wakes idle_sweep_restarts
cargo run --locked -p xtask -- policy
```

---

## Credits & Attribution

`VIctory-RTX-Mc3yohan` is built on the open-source VIctory ecosystem (all upstream Git commit
history and authors are preserved). Each project's specific contributions integrated here:

| Project / Author | What Was Taken / Integrated |
|---|---|
| **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)** | Original project & foundation: PE/XEX loader, HLE runtime, D3D12 backend, memory watch/shadowing, and GFX10-to-DXIL shader recompiler. |
| **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)** (`c9c932c` + `7ab13ed`, `77137b9`) | Direct upstream base: `VICTORY_THIN`, `VICTORY_ASYNC_SUBMIT`, `VICTORY_RELAY`, `VICTORY_LAND`, `VICTORY_CARD_STEPS`, Renderer Ports 1, 2, 5, 6, 7 (`PORT_CONSTANTS`, `PORT_CONSTANTS_ORDER` indirect-draw pop-in / lamp-lit room fix, `PORT_DESCRIPTORS`, `PORT_CBV`, `PORT_GUARD`, `PORT_JOURNAL`), `VICTORY_AMBIENT_RT` band, native `VICTORY_OUTPUT` / `VICTORY_UPSCALER`, `VICTORY_AMBIENT_SETTLE`, `VICTORY_TEXTURE_STORE`, FPS cap unlock, two-plane compose, `TablePlan`, AVX2 `SYNC_IN_PLACE`, `SPANS_QUICK`, `UNDER_FUSED`, `upq`, and frame record/replay. |
| **[codec-d/cool_fox](https://github.com/codec-d/cool_fox) & [DoubleGhosty/cool_fox](https://github.com/DoubleGhosty/cool_fox)** *(and `Davidjc13` / `Rubenpannen`)* | Wave32 guarded joins (`lanes.guards`) & single-latch active-loop lowering (`DIVERGENT_MASK_EXCHANGE`), Wave32 commuting-atomic LDS ordering (`lds_meetings`), immediate table release, `WaitOnAddress` fence waits, skin-bake upload drain (`ahead::drain`), `TextureQueue::reschedule`, `sweep_batch` wrap-around, VRAM residency budgets, and `shadow::register` overwritten-copy retirement (`6d813b4`). |
| **[trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA) & [rorodev6/victory-nvidia-megafork](https://github.com/rorodev6/victory-nvidia-megafork)** *(incl. `Jeremy Goldberg`, `Indy2l`, `Dastano`)* | D3D12 error `538` fix (separate `Compare` submission for `COMMON` state decay), D3D12 error `646` fix (strict `Role::Resolve` depth SRV binding & dimension check), `mimalloc` global Rust allocator (`PR #11`: per-thread caches cutting `victory-submit` Windows heap contention by ~3.3 ms), `VICTORY_TEXBACK_ASYNC=1` dedicated texture-readback thread & split landing telemetry (`2d1dc382`), `VICTORY_THIN=0` Blackwell whiteout/blur guard (`79f36b3b`), `VICTORY_NAN_SCRUB=1`, `rt_off_hashes.txt`, `VICTORY_FAIR_DUE=1`, `VICTORY_VIDEO_PLANES=1`, `VICTORY_TABLES_EARLY=1`, and Smooth Motion copy workflow. |
| **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)** *(incl. `cccw`, `Filip Michalski`, `Exopus`)* | Face animation (16-bit typed integer views), legacy FMA reflection fix, 32-bit int typed views, `VICTORY_TARGET_VARIANTS=1`, `SLOT_STRIDE_CONSTANT`, texture read-back, `xgamesave` resilience, aliased texture `Held` pin (`#1336`), `HostHeap::NEXT_ID` dynamic slot ownership (`#1324`), `submit_split` slot release (`#1320`), panic-safe pipeline queue (`#1353`), and `VICTORY_PIPELINE_COMPUTE_WAIT=1` (`#1312`). |
| **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)** *(incl. `zhu1025960209-web`, `IronMan`)* & **[DexNom/VIctory](https://github.com/DexNom/VIctory)** | Release ThinLTO + `codegen-units=1`, zero-latency deferred `PreludeUpload` texture uploads, `from_base` mip clamping + `TEXTURE2DARRAY` SRVs, tiled `texture.is_none()` guard, `by_address` VRAM `USED` leak fix, `SPARE_BATCH` pool + `BATCHES=32`, release-mode rich `F3` telemetry HUD (`panel.rs` + `#348` `LOCAL/BUDGET/DED CAP/SHARED GiB`), opt-in `VICTORY_KBM_GTA` PC keyboard & mouse look (`#295`), and PC FidelityFX DX12 bridge (`external.rs`, `fidelityfx.rs`, `fsr_4k.rs`, adapted here for NVIDIA FSR 3.1.x + RDNA 4 FSR 4.x). |
| **[TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay)** | Placed-view description padding zeroing + `VICTORY_OWNERLESS_VIEWS` cap (`#27`: eliminates 16.7 GB long-session RAM leak) and 4 KiB small-texture pool placement on non-AMD GPUs (`#45`: saves ~3.4 GB VRAM + 3.2 GB RAM on NVIDIA). |
| **VIctory-RTX3090** | Mid-list depth-plane materialization (`planes_now` / `VICTORY_PLANES_NOW` + 8,192-view ring, adapted for our `fallback_heaps=false` heap rebind), `VICTORY_SMALL_RECHECK` continuous whole-texture LUT/grading rechecks (`16 MiB / 60 ms` budget), clean `WM_CLOSE`/`WM_DESTROY` exit fix (`lifecycle.rs`), and shared wall-clock `FrameDeadline` pipeline waits. |
| **WASDeKeys/victory-gtx1660 (`Impre-visible`, `antikohza`)** | `STAGING` upload pool 4 GiB idle RAM leak fix (`VICTORY_STAGING_KEPT`), `COMMIT_SCARCE_BYTES` cache shedding, `PRELUDE_IDLE` prelude recycling (`VICTORY_MAX_PRELUDES=64`), `HostList::close` `E_OUTOFMEMORY` sweep-and-retry (`988c9de`), `VICTORY_CARD_AWAKE=idle` zero-contention GPU keep-awake, and opt-in `VICTORY_TEXTURE_MIP_DROP=0..3` (off by default). |
| **ZigZagBoi1/ScarlettPort & vitutu1/victory-perf** | `crates/victory-loader/src/hooks.rs` (title watchdog disarm `VICTORY_NO_WATCHDOG=1` and render-scale hooks). |
| **Korinocho, FQY55kai/ZJOU-VICTORY, myokex/victory-amd, Geckomacabre, cxy0072019-sketch, romainhedouin** | Cube-map gradient LOD, Wave32/RT/BVH/tiled-cascade-shadow fixes, and ecosystem diagnostics/research incorporated via upstream & community PRs. |
| **Mc3yohan (`VIctory-RTX-Mc3yohan`, this fork)** | **NVIDIA 1M-heap in-place Port 5** (`VICTORY_PORT_CBV=1` + `VICTORY_CONSTANT_MARKS=1` guarded `rawBufferLoad`, `View::Constant` `buffer_range`/`note_buffer` refresh, coherent `SLOT_STRIDES` upload buffer, `own.between` SRV L1TEX barrier, `raw_dwords` alignment & `SHADER_OPT=63`), **BatchCull Wave32 local-waves unlock** (`CS_Tree_BatchCull` / `ImposterOnly` disjoint commuting LDS atomics + `CS_Prop_BatchCull` barrier-free early-outs), **auto-VRAM tiering (`24GB+`/`16GB`/`12GB`/`8GB` with `CardBuffersMinKb=1024` on 8 GB)** + **plain OS-RAM `Card::staging`**, **6-step VRAM `CARD_COMPARE` tree reduction + `differs_gpu`** (`+20.3%`), **12-byte `PORT_DESCRIPTORS` key diff** (opt-in `-PortDescriptors`), **`DIRTY_CARDED` O(dirty)** (including `loose()` `register_carded` fix), **NVIDIA FSR 3.1.6 provider adaptation**, **`--nvidia-preset` for `AmericasLauncher`**, `Role::RayMiss` indoor eye-glow fix + `ambient.rs` asymmetric jump & `x=0` border fix, and `scripts/play-nvidia.ps1`. |

## License

GPL-3.0-or-later, as VIctory. See [LICENSE](LICENSE) and [docs/asset-policy.md](docs/asset-policy.md).
