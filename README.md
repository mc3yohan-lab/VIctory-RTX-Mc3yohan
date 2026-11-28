**English** · [Español](README.es.md)

# VIctory-RTX-Mc3yohan (NVIDIA RTX / Wave32 Fork)

[![CI](https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan/actions/workflows/ci.yml/badge.svg?branch=nvidia-rtx4090)](https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan/actions/workflows/ci.yml)

> Maintained by **Mc3yohan** ([github.com/Mc3yohan](https://github.com/Mc3yohan)) · License **GPL-3.0-or-later**

An NVIDIA-focused fork of [Marvisak/VIctory](https://github.com/Marvisak/VIctory) built on top of
[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX) (`c9c932c`) and
selected community NVIDIA/Wave32 fixes (full provenance in [Credits & Attribution](#credits--attribution)),
translating Xbox Series X|S **D3D12.X** and **GFX10 (RDNA2)** shaders to host Direct3D 12 Shader Model 6.6
on **Wave32 NVIDIA GPUs** (tuned on RTX 4090 + hybrid Intel Core i9, with automatic multi-GPU VRAM/RAM profiles).

---

## Legal Notice & Asset Policy

1. **Zero game files or copyrighted assets:** This repository contains **no** game executables, proprietary DLLs, shaders, textures, models, audio, video, cryptographic keys, or screenshots. You must supply your own legally obtained console files stored **outside** this repository (`victory.toml` or via the desktop launcher).
2. **Interoperability & technical research only:** This software is an API and shader compatibility layer in the spirit of Wine/Proton. It does not circumvent DRM or technological protection measures; it only translates console APIs and shader bytecode locally on user-supplied files.
3. **No affiliation or endorsement:** Independent open-source project, not affiliated with, sponsored by, or endorsed by Microsoft, Xbox, NVIDIA, AMD, or any game developer or publisher.
4. **Automated asset-policy enforcement (`cargo xtask policy`):** Every commit is verified against the strict allowlist in [docs/asset-policy.md](docs/asset-policy.md) blocking binaries, images, and embedded data.
   - *Documented upstream exception:* `crates/victory-formats/src/bcpack-tables.bin` (~78 KB) contains constant format decoding tables for Xbox BCPack and is explicitly excluded from the repository's GPL-3.0 license (see [docs/asset-policy.md](docs/asset-policy.md#the-one-exception)).
5. **Source-only distribution:** Cover art shown in the desktop launcher is read strictly from your local folder at runtime and never bundled in Git.

---

## Goal

**Deliver stable 30+ FPS native (60+ FPS visual with Smooth Motion) without ray tracing**
at clean visual quality (fast texture streaming, natural indoor/outdoor indirect lighting,
zero face/eye/wall-reflection glitches), then push higher frame rates and optionally enable real
Wave32 ray tracing (`-RayTracing`).

---

## Current Status (2026-11-27 — branch `nvidia-rtx4090`, Translator `v6`)

Reference test hardware: **Windows 11 x64, Intel Core i9-12900KF (8P/16T + 8E/8T), 64 GB RAM,
NVIDIA GeForce RTX 4090 (24 GB GDDR6X, 32 GiB Resizable BAR, Wave32, SM 6.2 `native16=true`, 48 KiB LDS, 1M view heap)**,
Series X profile (ray tracing off).

The 15-minute RTX 4090 run at native 1080p reached 16.8 FPS median in the boat after excluding observed menus and transitions, with an 8.0 FPS minimum across measurement windows. The earlier 720p and scaled 1080p figures below describe different workloads. Launcher 0.9.1 exposes the native setup under **Nítido 1080p**. [Image quality, measurements and next tests (Spanish)](docs/rtx4090-informe-2026-11-27.md)

| Metric | Measured Status |
|---|---|
| **Menu / UI** | **60.0 FPS** (`16.7 ms/frame`, `0.0 ms` GPU wait) with native 60 FPS unlock (`-Fps 60`, simulation clock stays `1.00x`) |
| **Fluid 720p (`1280×720`, `Scale 1.0`, `release`, v6)** | **22.6–26.6 FPS in the sampled section** (`24.5 FPS` / `40.9 ms`, `prep 26.1 ms`, `copyback 21.0 ms` at `4,857–5,022` draws/frame; `0` warnings/errors, `0` skin-bake errors across `372` draws, `0` shader misses) |
| **Balanced 1080p (`1920×1080`, `Scale 0.667`, `release`, v6)** | **20.1–26.0 FPS in the sampled section** (`0.0–0.6 ms` GPU wait, `3,173` shader hits / `0` misses, `0` warnings/errors — CPU-bound in `victory-submit`) |
| **Wave32 Shader Coverage (`TRANSLATOR_VERSION = 6`, `SHADER_OPT = 191`)** | **`25,784` shaders translated to Wave32 (`99.98%`) in `165 s`** (**`974.9 MB`** clean disk cache); **`2,787` used pipelines pre-warmed (`254.7 MB` `host-pipelines.bin`, `0` driver refusals)**; includes native FP16 (`SM 6.2`), `48 KiB` LDS, linked Mesh+Pixel lowering, 2-D group thread tickets (`64/64` slots verified on RTX 4090 vs `32/64` legacy), lane-mask select fix, `s_min`/`s_max` SCC fix, transmission tile rewrite, and un-skipped Wave32 paired open-water lighting (`WATER_LIGHT_HASHES`) |
| **Native PC FidelityFX 4K (`-Fsr4K`)** | **Verified live on RTX 4090 with signed AMD FSR `3.1.6`** (`FSR SDK v2.4.0`, pixel-exact `fidelityfx` hardware test `1/1 ok` + byte-verified `quality`/`balanced`/`performance` 4K profiles) |
| **Exclusive NVIDIA 1M-Heap Port 5 (`VICTORY_PORT_CBV=1` in-place)** | **`8,700–16,750` table CBVs/frame (`3.7–6.7 MB/frame`) read in place** (`0` unaligned fallbacks), slashing `card_steps` fills from `~12,000` → **`~145/frame`**; with `own.between` SRV L1TEX state barrier + aligned `raw_dwords` + collapsed `smem` dispatch |
| **Top 3 Compute Shaders (`7.42 ms` / 22% GPU time)** | Verified `3/3` with `dxil.dll` + RTX 4090 PSO: `CS_Tree_BatchCull` (`3.47 ms`) & `CS_Tree_BatchCull_ImposterOnly` (`1.55 ms`) promoted from `WAVE64-WIDE` → **`local waves (idiom)`** via disjoint commuting LDS atomics; `CS_Prop_BatchCull` (`2.40 ms`) keeps native early-outs; cut RTX 4090 GPU busy time by `~10 ms/frame` |
| **RTX 4090 `CARD_COMPARE` Microbenchmark** | **`17.31 ms → 14.40 ms` (`+20.3%`, `705 → 848 GB/s`)** via 6-step parallel tree reduction + VRAM `differs_gpu` |
| **Visual Quality, Audio & Stability Upgrades** | **Geometric-mean room-toned sky reflection bound (`ambient.rs` `GEO` + `clear_refl`, adapted from `VIctory-NVIDIA #63`)** eliminating blue-violet indoor wall/paper artifacts & window-blown sky reflections; **post-exposure indoor diffuse floor (`VICTORY_AMBIENT_EXPOSURE_FLOOR=0.0045`)** fixing dark backlit cutscenes; `ambient.rs` bilinear `sample_points`, `isnan`/`isinf` & magenta probe scrubbing, `Role::Reflect` (`CS_UpdateHistory`) clamping (`ambient_most=4.0`, `ambient_reflect=1.2`); whole-surface fingerprinting for linear `R8_UNORM` video planes (`is_placed_movie_plane`); `VICTORY_RAW_VIEW_CLAMP` OOB view protection (`VIctory-NVIDIA #24`); 21-bit PFN reuse (`xmem.rs`, `VIctory-NVIDIA #55`); opt-in host DSP convolution (`-DspConvolution` / `VICTORY_DSP_CONVOLUTION=1`, `Vice-Tay #293`) for filtered dialogue; bounded GPU mapping fence wait (`VICTORY_MAPPING_WAIT_MS`, `Vice-Tay #307`); `HostList::upload` buffer pooling (`VIctory-NVIDIA #57`), single-pass send layout (`laid_out`) & sorted-ranges `joined()` (`Vice-Tay #286`), DirectStorage below-normal workers (`VIctory-NVIDIA #39`), submit-helper priority (`VIctory-NVIDIA #22`), opt-in `-WideCopy` (`AnyXboxX #500`), and native `VIctory-RTX.exe` launcher (`370 ms` startup, automatic `config_version` migration) |
| **Memory Hygiene & Multi-GPU Auto-Tiers** | **4 KiB small-texture placement on non-AMD GPUs (`Vice-Tay #45`: saves ~3.4 GB VRAM + 3.2 GB RAM)**; **placed-view padding zeroing + `VICTORY_OWNERLESS_VIEWS` cap (`Vice-Tay #27`)**; **automatic `<= 24 GiB` RAM (incl. 16 GiB hosts) or `< 8 GiB` free commit retention profile (`ram_profile.rs` / `VICTORY_RAM_16GB=0/1`)**; **`VICTORY_READBACK_POOL=1`**, **`VICTORY_BATCHED_SWEEP=1`**, `STAGING` & `UPLOAD_SPARE` bounded pools + `PRELUDE_IDLE` recycling + `HostList::close` `E_OUTOFMEMORY` retry; **automatic NVIDIA VRAM tier detection** (`24GB+`: `WindowsBarMb=8192, MipDrop=0`; `16GB`: `WindowsBarMb=3584, CardBuffersMb=4096, MipDrop=1`; `12GB`: `WindowsBarMb=2048, CardBuffersMb=2560, MipDrop=1`; `8GB`: `WindowsBarMb=1024, CardBuffersMb=1280, CardBuffersMinKb=1024, MipDrop=2, auto -SeriesS`) |

---

## What Has Been Implemented in This Fork

### 1. Renderer Ports 1, 2, 5, 6, 7, RT & Native Output Scaling
| Switch | What it does |
|---|---|
| `VICTORY_PORT_CONSTANTS=1` | The title's constants heap is read by the card in place: no uploads, no fills (`0` forgotten regions) |
| `VICTORY_PORT_DESCRIPTORS=1` | Opt-in (`-PortDescriptors`): 11 byte-verified Scarlett descriptor stores report written slots directly + 12-byte key diff (`changed_key12`) & `copy_simple` mark ordering |
| `VICTORY_PORT_CBV=1` | **NVIDIA 1M-heap in-place Port 5 (`7b908a2`):** translated shaders read table/heap constant buffers in place as raw byte-address SRVs (`raw_views=0`, no 4M twin heap needed) |
| `VICTORY_PORT_GUARD=1` | Shared write-guard lock with per-64 KiB block claims + parallel per-list refresh planning + coalesced `VirtualProtect` |
| `VICTORY_PORT_JOURNAL=1` | Early heap sync triggered at title submit + 64-sharded journal locks |
| `VICTORY_PORT_RT=1` | Forward-filled RT descriptor views + placed-resource ordering (`avatarmanz 84917c34`) |
| `VICTORY_OUTPUT=1920x1080` | Native title output scaling on Series X (`-Full720p` / `-Full1440p` / `-Full4K` available) |
| `VICTORY_UPSCALER=6` | Forces the title's own temporal upscaler instead of console FidelityFX |

### 2. NVIDIA Wave32 Shader Translator (`v6`) & Pipeline Stability
- **Wave32 lowering & correctness:** guarded joins (`lanes.guards`), single-latch active-loop lowering (`DIVERGENT_MASK_EXCHANGE`), commuting-only LDS atomic region check (`lds_meetings`), 2-D group thread tickets (`VICTORY_THREAD_TICKET=1`, `64/64` verified on RTX 4090), guest-Wave32 numeric lane-mask select (`selects_on_mask_data`), scalar `s_min`/`s_max` SCC propagation, `ir::unspill` 256-slot table, transmission tile allocation rewrite (`water_tile_append.rs`), and un-skipped Wave32 paired open-water lighting (`WATER_LIGHT_HASHES`).
- **Wave32 SM 6.2 / 48 KiB LDS / Linked Mesh+Pixel:** native 16-bit floats (`VICTORY_F16_IN_F32=0`), `VICTORY_HOST_LDS_KIB=48`, linked mesh+pixel payloads under the 32 KiB D3D12 limit, `VICTORY_SHADER_OPT=191`, `VICTORY_WAVE_WIDE=last`, `VICTORY_NAN_SCRUB=1`, `VICTORY_BVH_NO_HIT=1`, and default Wave64 pairing for 28 critical shadow/water/sky shaders.
- **D3D12 correctness & offline warming:** D3D12 error `538` & `646` fixes, `VICTORY_RAW_VIEW_CLAMP=1`, `victory shaders --nvidia-preset` + `victory warm --nvidia-preset --used --save-library` ensuring 100% cache-key parity between offline warming and live runs.

### 3. GPU Bandwidth, Shared-Memory Reduction & Synchronization
- **Resizable BAR VRAM Windows (`VICTORY_WINDOWS_BAR_MB=8192` on 24 GB+, `VICTORY_WINDOW_KB=4096`)** moving loose mesh/vertex windows off PCIe System RAM into GDDR6X.
- **Plain OS-RAM `Card::staging` (`shadow.rs`)**, **VRAM `differs_gpu` + 6-step tree reduction in `CARD_COMPARE`**, **`HostList::upload` pooling (`UPLOAD_SPARE`)**, **single-pass `host/send.rs::laid_out`**, **sorted-ranges `shadow.rs::joined()`**, **opt-in parallel `VICTORY_WIDE_COPY=1`**, and **bounded mapping fence waits (`VICTORY_MAPPING_WAIT_MS`)**.
- **Thread scheduling:** DirectStorage workers below normal priority (`VICTORY_DSTORAGE_PRIORITY=-1`) and submit-chain helpers above normal (`VICTORY_HELPER_PRIORITY=1`).

### 4. Visual Quality, Audio, Input & Native Launcher
- **Calibrated RT-off indirect lighting (`ambient.rs`, `lightprobe.rs`):** geometric-mean room luminance (`cells[GEO]`) + room-toned `clear_refl()` eliminating blue-violet indoor reflections on walls/posters, post-exposure diffuse floor (`VICTORY_AMBIENT_EXPOSURE_FLOOR=0.0045`), bilinear `sample_points`, magenta/NaN probe scrubbing, `Role::Reflect` clamping (`4.0 / 1.2`), and time-based settling (`VICTORY_AMBIENT_SETTLE`).
- **Audio & input:** split-lock MMCSS `"Pro Audio"` XAudio2 worker, opt-in host DSP partitioned-FFT convolution (`-DspConvolution` / `VICTORY_DSP_CONVOLUTION=1`) for filtered dialogue, XInput/DualShock 4 gamepad support, and opt-in PC keyboard & mouse look (`-Kbm`).
- **Native Desktop Launcher (`VIctory-RTX.exe`):** 3-step guided setup wizard, instant `< 400 ms` async startup, automatic `config_version` migration, VRAM/RAM auto-presets, 1-click shader & pipeline pre-warming, live telemetry/log viewer, and sanitized diagnostic report export (redacting username, profile path, and game directory).

---

## Known Bottlenecks & Where Collaborators Can Help Most

1. **CPU `victory-submit` Thread (Primary Ceiling on RTX 4090):**
   - With RTX 4090 GPU wait at `0.0–0.6 ms/frame`, `victory-submit` (`prep ~26–33 ms` + `copyback ~21 ms`) is the #1 ceiling above 30 FPS. Key remaining hotspots: descriptor sync, relay fetch, and `LAND` merge (per-draw mirrored table caching and off-submit landing).
2. **8 GB & 12–16 GB VRAM Tuning (RTX 3070 / 4060 / 5060 / 5070 Ti):**
   - Avoiding WDDM Shared GPU Memory spill is critical on 8–16 GB cards; `play-nvidia.ps1` and `VIctory-RTX.exe` auto-scale BAR/buffer/mip tiers, and `ram_profile.rs` guards `<= 24 GiB` RAM (incl. 16 GiB) or `< 8 GiB` free-commit systems.
3. **Native NVIDIA DLSS Super Resolution Backend:**
   - `fidelityfx.rs` already intercepts all 7 D3D12 inputs (`color`, `depth`, `motion_vectors`, `exposure`, `reactive_mask`, `transparency_mask`, `output`) plus `jitter`, `motion_scale`, and `reset`, running signed AMD FSR `3.1.6` live on RTX 4090. Adding an optional NVIDIA Streamline / NGX DLSS backend behind the same hook is the next upscaler milestone.
4. **Remaining 6 Wave64-Only Shaders (`02384a60fda6f2d0` = 1 drop/frame)** & in-world offscreen UI screens (e.g., phone screen rendering in early cutscenes).

---

## How to Build, Pre-Warm & Run (Windows 10/11 + NVIDIA RTX)

### Requirements
- Windows 10/11 x64, AVX2 CPU, Direct3D 12 GPU with Shader Model 6.6 + Mesh Shaders (RTX 30/40/50 series).
- Rust toolchain (pinned by `rust-toolchain.toml`) + MSVC C++ Build Tools.
- Your own legally obtained console dump placed **outside** this repository.

### 1. Build
```powershell
# Build both the runtime and the native desktop launcher (VIctory-RTX.exe)
cargo build --locked --release -p victory -p victory-launcher

# Optional diagnostics build (for -GpuTiming / VICTORY_CAPTURE / VICTORY_TIMELINE)
cargo build --locked --profile diag -p victory --features diagnostics
```

### 2. Easiest Way to Run: Native Desktop Launcher (`VIctory-RTX.exe`)
Double-click **`Launch-VIctory-RTX.cmd`** (or `target\release\VIctory-RTX.exe`) — **no PowerShell flags, no manual `victory.toml` editing**:
1. Choose your external console dump folder (auto-generates `victory.toml`; cover art is loaded locally from your folder at runtime).
2. Click **Preparar gráficos / Build Shaders** once to pre-translate Wave32 v6 shaders (`~975 MB`) and pre-warm `host-pipelines.bin` (`~255 MB`).
3. Pick a preset (**Recomendado**, **Fluido 720p** native `1280×720`, **Equilibrado 1080p**, **Nítido 1080p**, **Alta 1440p**, **4K FSR 3.1**, or **Modo Seguro**), optionally toggle keyboard & mouse look, and press **JUGAR / PLAY**.
4. Press **`F3`** in-game for the live telemetry HUD, and use the **Logs** tab for sanitized diagnostic reports.

### 3. CLI / PowerShell Alternative (`scripts\play-nvidia.ps1`)
Copy `victory.example.toml` → `victory.toml`, set `game_dir`, then pre-warm once and run (Normal, Non-Elevated PowerShell):
```powershell
.\target\release\victory.exe shaders --nvidia-preset --jobs 16
.\target\release\victory.exe warm --nvidia-preset --used --jobs 16 --save-library

# Balanced 1080p (720p internal -> 1080p title upscaler, auto VRAM tier)
.\scripts\play-nvidia.ps1

# Crisp native 720p (1280x720 internal -> 720p output)
.\scripts\play-nvidia.ps1 -Full720p -Scale 1.0

# Crisp native 1080p (1920x1080 internal -> 1080p output)
.\scripts\play-nvidia.ps1 -Scale 1.0

# Keyboard & mouse look (F2 or ` toggles mouse-look; Esc/P unlocks)
.\scripts\play-nvidia.ps1 -Kbm -MouseSens 1.25

# Optional: host DSP convolution for filtered dialogue / parallel large copies
.\scripts\play-nvidia.ps1 -DspConvolution
.\scripts\play-nvidia.ps1 -WideCopy

# Native PC FidelityFX 4K upscaling (place official signed AMD amd_fidelityfx_upscaler_dx12.dll
# in %LOCALAPPDATA%\victory\fidelityfx or set fidelityfx_dir in victory.toml)
.\scripts\play-nvidia.ps1 -Fsr4K balanced

# NVIDIA Smooth Motion copy (enable Smooth Motion for victory_sm.exe in NVIDIA App)
.\scripts\play-nvidia.ps1 -SmoothMotion

# 15s GPU per-pass & top-40 shader profiling (uses diag build)
.\scripts\play-nvidia.ps1 -GpuTiming

# Safe fallback mode
.\scripts\play-nvidia.ps1 -Safe
```

### 4. Integration with External Launchers (`runners.json`)
`victory.exe` supports `--nvidia-preset` on `shaders`, `warm`, and `run` so external front-ends without per-runner environment blocks get full RTX optimization while preserving console profile flags:
```json
{
  "id": "victory-rtx-mc3yohan",
  "name": "VIctory-RTX-Mc3yohan (NVIDIA RTX)",
  "gitUrl": "https://github.com/mc3yohan-lab/VIctory-RTX-Mc3yohan.git",
  "buildCommands": [
    "rustup show active-toolchain || rustup toolchain install",
    "cargo build --locked --release -p victory"
  ],
  "exe": "target\\release\\victory.exe",
  "preLaunchArgs": ["shaders --nvidia-preset \"{gameFolder}\""],
  "launchArgs": "run \"{gameExe}\" --execute --nvidia-preset",
  "notes": "NVIDIA RTX / Wave32 / ReBAR / Ports 1-7"
}
```

### Verification Before Committing
```powershell
cargo run --locked -p xtask -- policy
cargo clippy --workspace --all-targets --locked -- -D warnings
cargo clippy --workspace --all-targets --all-features --locked -- -D warnings
cargo test --locked --release -p victory-shader --lib -- divergent_lds_atomics
cargo test --locked --release -p victory-gpu --lib -- torn ambient warp_card_compare host_ram_card_staging changed_key12 room_only_wakes idle_sweep_restarts
cargo test --locked -p victory-launcher --bin VIctory-RTX
```

---

## Credits & Attribution

`VIctory-RTX-Mc3yohan` is built on the open-source VIctory ecosystem (all upstream Git commit
history and authors are preserved). Each project's specific contributions integrated here:

| Project / Author | What Was Taken / Integrated |
|---|---|
| **[Marvisak/VIctory](https://github.com/Marvisak/VIctory)** | Original project & foundation: PE/XEX loader, HLE runtime, D3D12 backend, memory watch/shadowing, and GFX10-to-DXIL shader recompiler. |
| **[avatarmanz/VIctory-7900XTX](https://github.com/avatarmanz/VIctory-7900XTX)** (`c9c932c` + `7ab13ed`, `77137b9`, `84917c34`) | Direct upstream base: `VICTORY_THIN`, `VICTORY_ASYNC_SUBMIT`, `VICTORY_RELAY`, `VICTORY_LAND`, `VICTORY_CARD_STEPS`, Renderer Ports 1, 2, 5, 6, 7 & RT Part 1, `VICTORY_AMBIENT_RT` band, native `VICTORY_OUTPUT` / `VICTORY_UPSCALER`, `VICTORY_AMBIENT_SETTLE`, `VICTORY_TEXTURE_STORE`, FPS cap unlock, two-plane compose, `TablePlan`, AVX2 `SYNC_IN_PLACE`, `SPANS_QUICK`, `UNDER_FUSED`, `upq`, and frame record/replay. |
| **[codec-d/cool_fox](https://github.com/codec-d/cool_fox) & [DoubleGhosty/cool_fox](https://github.com/DoubleGhosty/cool_fox)** *(and `Davidjc13` / `Rubenpannen`)* | Wave32 guarded joins (`lanes.guards`), single-latch active-loop lowering (`DIVERGENT_MASK_EXCHANGE`), commuting-atomic LDS ordering (`lds_meetings`), immediate table release, `WaitOnAddress` fence waits, skin-bake upload drain (`ahead::drain`), `TextureQueue::reschedule`, `sweep_batch` wrap-around, VRAM residency budgets, `shadow::register` overwritten-copy retirement, stale `texback` ticket re-registration, and removed-device fence guards. |
| **[trustno1-coder/VIctory-NVIDIA](https://github.com/trustno1-coder/VIctory-NVIDIA) & [rorodev6/victory-nvidia-megafork](https://github.com/rorodev6/victory-nvidia-megafork)** *(incl. `Jeremy Goldberg`, `Indy2l`, `Dastano`)* | D3D12 error `538`/`646` fixes, `mimalloc`, release-mode `PLACED_RAW`/`PLACED_TWINS` gating, `VICTORY_TEXBACK_ASYNC=1`, Blackwell `VICTORY_THIN=0` guard, `VICTORY_NAN_SCRUB=1`, `rt_off_hashes.txt`, `VICTORY_FAIR_DUE=1`, `VICTORY_VIDEO_PLANES=1`, `VICTORY_TABLES_EARLY=1`, `VICTORY_HELPER_PRIORITY` (`#22`), `VICTORY_RAW_VIEW_CLAMP` (`#24`), 2-D group thread tickets (`#30`), DirectStorage worker priority (`#39`), Wave32 transmission tile allocation (`#45`), lane-mask select (`#52`), `ir::unspill` fix (`#53`), 21-bit PFN reuse (`#55`), `HostList::upload` pool & smart spare trim (`#57`), and geometric-mean room-toned reflection bound (`#63`). |
| **[fxceless/victory-fork](https://github.com/fxceless/victory-fork)** *(incl. `cccw`, `Filip Michalski`, `Exopus`, `PiloteCode`)* | Face animation (16-bit typed integer views), legacy FMA reflection fix, 32-bit int typed views, `VICTORY_TARGET_VARIANTS=1`, `SLOT_STRIDE_CONSTANT`, texture read-back, `xgamesave` resilience, aliased texture `Held` pin (`#1336`), `HostHeap::NEXT_ID` dynamic slot ownership (`#1324`), `submit_split` slot release (`#1320`), panic-safe pipeline queue (`#1353`), `VICTORY_PIPELINE_COMPUTE_WAIT=1` (`#1312`), `VICTORY_FEED_ONE_CALL=1` (`#1137`), and `Shadow::differing` first-slice saturation fix (`#1407`). |
| **[Belobug/VIctory-Belobug-AMD](https://github.com/Belobug/VIctory-Belobug-AMD)** *(incl. `zhu1025960209-web`, `IronMan`)* & **[DexNom/VIctory](https://github.com/DexNom/VIctory)** | Release ThinLTO + `codegen-units=1`, zero-latency deferred `PreludeUpload`, `from_base` mip clamping + `TEXTURE2DARRAY` SRVs, tiled `texture.is_none()` guard, `by_address` VRAM `USED` leak fix, `SPARE_BATCH` pool, release-mode `F3` telemetry HUD (`panel.rs` + `#348`), opt-in PC keyboard & mouse look (`#295`), global `MISSED_ALL` descriptor warning (`#354`), and PC FidelityFX DX12 bridge (`external.rs`, `fidelityfx.rs`, `fsr_4k.rs`, adapted here for NVIDIA FSR 3.1.x + RDNA 4 FSR 4.x). |
| **[TaynanGT/Vice-Tay](https://github.com/TaynanGT/Vice-Tay)** | Placed-view padding zeroing + `VICTORY_OWNERLESS_VIEWS` cap (`#27`), 4 KiB small-texture placement (`#45`), `#116` correctness + perf bundle (RAII `TexturePass`, `Prefill`/`PrefillClaim`, `Step::RawCopy`, `XMemFreeDefault`/`title_virtual_free` guards, `#450` loose-window budget), single-pass send layout & sorted-ranges `joined()` (`#286`), opt-in host DSP convolution (`#293`), bounded mapping fence wait (`#307`), and scalar `s_min`/`s_max` SCC fix (`#315`). |
| **[LooperSalty/AnyXboxX](https://github.com/LooperSalty/AnyXboxX)** | `ambient.rs` cooperative 64-cell gather tiling (`#475`) & 32×32 `raise` groups (`#486`), plus opt-in parallel large `copy_mapped` (`VICTORY_WIDE_COPY=1`, `#500`). |
| **[Peppacodes/playablegames-peppa](https://github.com/Peppacodes/playablegames-peppa)** *(incl. `XaviFortes`, `Skyfrye`, `KevinPratap`)* | Lock-free `Shadow::has_pending`, binary-searched sorted `POOL`, `watch::ask_ahead` early-return, `VICTORY_WRITE_JOBS=1`, `VICTORY_WATCH_TEXTURES_JOINED=1`, `HostList::keep` dedup, and `Batch::last_ahead` 1-entry cache. |
| **[Geckomacabre/vx-layer](https://github.com/Geckomacabre/vx-layer)** | `ambient.rs` `Role::Reflect` (`CS_UpdateHistory`), bilinear `sample_points`, `isnan`/`isinf` & magenta probe scrubbing, `VICTORY_READBACK_POOL=1`, and `VICTORY_PIPELINE_SAVE_ON_EXIT_ONLY=1`. |
| **[TheUnrealZaka/VIctoryUnified-DLSS](https://github.com/TheUnrealZaka/VIctoryUnified-DLSS)** *(incl. `louketa/foxhound`)* | `VICTORY_WIDE_THREADS`, `VICTORY_EXACT_NO_WAIT=1`, opt-in `VICTORY_WC_NO_BACK`/`VICTORY_WC_BAR`, and split-lock MMCSS `"Pro Audio"` XAudio2 worker (`audio.rs`). |
| **[polackangry/POLSKAGUROM](https://github.com/polackangry/POLSKAGUROM)** | `VICTORY_BATCHED_SWEEP=1` (`SweepSchedule`). |
| **VIctory-RTX3090** | Mid-list depth-plane materialization (`planes_now`), `VICTORY_SMALL_RECHECK`, clean `WM_CLOSE`/`WM_DESTROY` exit fix, and shared wall-clock `FrameDeadline`. |
| **WASDeKeys/victory-gtx1660 (`Impre-visible`, `antikohza`)** | `STAGING` upload pool cap, `COMMIT_SCARCE_BYTES` shedding, `PRELUDE_IDLE` recycling, `HostList::close` `E_OUTOFMEMORY` retry, `VICTORY_CARD_AWAKE=idle`, and opt-in `VICTORY_TEXTURE_MIP_DROP`. |
| **ZigZagBoi1/ScarlettPort & vitutu1/victory-perf** | Title watchdog disarm (`VICTORY_NO_WATCHDOG=1`), render-scale hooks, `!reserved` levels-on-demand pool skip, `VICTORY_SAME_HOST_HEAPS`, loose-window shared read-lock lookup, quiet `0x1c8`/`0x218` pipeline log (`#147`), and desktop launcher UX inspiration. |
| **Mc3yohan (`VIctory-RTX-Mc3yohan`, this fork)** | **NVIDIA 1M-heap in-place Port 5** (`VICTORY_PORT_CBV=1`), **BatchCull Wave32 local-waves unlock**, **Wave32 FP16 native (`SM 6.2`), 48 KiB LDS & linked Mesh+Pixel lowering**, **un-skipped Wave32 paired open-water lighting**, **post-exposure indoor diffuse floor (`VICTORY_AMBIENT_EXPOSURE_FLOOR`)**, **whole-surface movie-plane fingerprinting**, **`F3` `CpuMeter::sample` crash fix**, **auto-VRAM tiering (`8/12/16/24 GB+`)** + **`<= 24 GiB` RAM / `< 8 GiB` commit `ram_profile.rs`**, **plain OS-RAM `Card::staging`**, **6-step VRAM `CARD_COMPARE` (`+20.3%`)**, **12-byte `PORT_DESCRIPTORS`**, **`DIRTY_CARDED` O(dirty)**, **NVIDIA FSR 3.1.6 adaptation**, **`--nvidia-preset` for external launchers**, **native `VIctory-RTX.exe` launcher**, and `scripts/play-nvidia.ps1`. |

---

## License

Licensed under **GPL-3.0-or-later** — see [LICENSE](LICENSE) and [docs/asset-policy.md](docs/asset-policy.md).
