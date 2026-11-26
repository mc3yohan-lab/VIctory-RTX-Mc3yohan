# VIctory-RTX-Mc3yohan — NVIDIA RTX / Wave32 Launcher (maintained by Mc3yohan)
# Built on Marvisak/VIctory + avatarmanz/VIctory-7900XTX + community NVIDIA/Wave32 fixes.
# Tuned for NVIDIA RTX 4090 (24 GB VRAM, Wave32, 1M view heap) & hybrid Intel Core i9:
# - avatarmanz/VIctory-7900XTX (`c9c932c`): Renderer Ports 1, 2, 5, 6, 7 (VICTORY_PORT_CONSTANTS,
#   VICTORY_PORT_DESCRIPTORS, VICTORY_PORT_CBV, VICTORY_PORT_GUARD, VICTORY_PORT_JOURNAL),
#   native VICTORY_OUTPUT + VICTORY_UPSCALER=6, Wave32 NVIDIA RT/BVH/tiled-cascade-shadow PRs,
#   FPS unlock (VICTORY_FRAME_INTERVAL_MS), two-plane compose, TablePlan, AVX2 SYNC_IN_PLACE,
#   SPANS_QUICK, UNDER_FUSED, LANE_CORE, upq copy queue.
# - fxceless/victory-fork + trustno1-coder/VIctory-NVIDIA + codec-d/cool_fox + ScarlettPort:
#   Faces move, legacy FMA reflections, 32-bit int typed views, VICTORY_TARGET_VARIANTS=1,
#   D3D12 538 Compare state-decay fix, D3D12 646 ambient depth SRV fix, Wave32 LDS atomic ordering,
#   TextureQueue::reschedule, sweep_batch wrap-around, VICTORY_NAN_SCRUB=1, watchdog disarm.
# - VIctory-RTX-Mc3yohan original work: NVIDIA 1M-heap in-place Port 5 (VICTORY_PORT_CBV=1 with
#   raw_views=0), ReBAR 4 GB VRAM windows (WindowKb=4096), plain OS-RAM Card::staging, 6-step VRAM
#   CARD_COMPARE tree reduction, 12B PORT_DESCRIPTORS header diff (99.9993% accuracy), DIRTY_CARDED
#   O(dirty), Tier-A pure-BVH auto-skip, Role::RayMiss eye fix, coalesced VirtualProtect.
#
# Usage:
#   scripts\play-nvidia.ps1                        # Series X 1080p output + Ports 1,2,5,6,7 + ReBAR + 60 FPS unlock (fastest)
#   scripts\play-nvidia.ps1 -Scale 1.0             # Series X native 1920x1080 internal + 1080p output
#   scripts\play-nvidia.ps1 -Full1440p             # Series X 2560x1440 output (1706x960 internal -> 1440p upscaled)
#   scripts\play-nvidia.ps1 -Full4K                # Series X 3840x2160 output (2560x1440 internal, console default)
#   scripts\play-nvidia.ps1 -SmoothMotion          # Launches victory_sm.exe for NVIDIA App Smooth Motion (2x visual FPS)
#   scripts\play-nvidia.ps1 -GpuTiming             # 15s diag profiling: exact GPU ms per pass & top-40 shaders + memory breakdown
#   scripts\play-nvidia.ps1 -SendBar               # Put submission upload memory in VRAM via ReBAR (cuts GPU copy ms)
#   scripts\play-nvidia.ps1 -TexStore              # Enable disk cache of sharp mip levels (faster texture streaming on revisits)
#   scripts\play-nvidia.ps1 -RayTracing            # Test real Wave32 NVIDIA Ray Tracing + exact cascade shadows
#   scripts\play-nvidia.ps1 -SeriesS               # Series S 1080p + 4K sharp HUD @ 720p internal (0.667)
#   scripts\play-nvidia.ps1 -WaitTextures          # Sets VICTORY_TEXTURE_NO_WAIT=0 (slower streaming, avoids partial BCn blocks)
#   scripts\play-nvidia.ps1 -NoPortCbv             # Disable Renderer Port 5 (uses non-CBV shader cache key)
#   scripts\play-nvidia.ps1 -NoPortDescriptors     # Disable Renderer Port 2 (full view-heap scan fallback)
#   scripts\play-nvidia.ps1 -NoPortGuard           # Disable Renderer Port 6 (shared guard lock + parallel planning)
#   scripts\play-nvidia.ps1 -NoPortJournal         # Disable Renderer Port 7 (early heap sync + 64-sharded journal)
#   scripts\play-nvidia.ps1 -NoPortConstants       # Disable Renderer Port 1 (VICTORY_PORT_CONSTANTS)
#   scripts\play-nvidia.ps1 -Safe                  # Safe fallback path (disables port patches, ReBAR windows, and thin lists)
#   scripts\play-nvidia.ps1 -NoLog                 # Disable automatic log capture
#
# Run from a normal (non-elevated) PowerShell window so NVIDIA keeps its DXCache in the usual place.
param(
    [string]$Exe = '',
    [switch]$SeriesS,
    [double]$Scale = 0.0,
    [double]$Fps = 60.0,
    [switch]$Full1440p,
    [switch]$Full4K,
    [switch]$FidelityFx,
    [switch]$WaitTextures,
    [switch]$TexStore,
    [switch]$SkipRtHashes,
    [switch]$SkipRtAggressive,
    [switch]$LaneCore,
    [switch]$NoAmbient,
    [switch]$NoPortConstants,
    [switch]$PortDescriptors,
    [switch]$NoPortDescriptors,
    [switch]$NoPortCbv,
    [switch]$NoPortGuard,
    [switch]$NoPortJournal,
    [switch]$UpQueue,
    [switch]$SweepPerHeap,
    [switch]$XmaRefill,
    [switch]$SendBar,
    [switch]$Residency,
    [switch]$GpuTiming,
    [switch]$Quality,
    [switch]$Safe,
    [switch]$RayTracing,
    [switch]$SmoothMotion,
    [switch]$WideShaderOpt,
    [int]$WindowsBarMb = 8192,
    [int]$WindowKb = 4096,
    [int]$CardBuffersMinKb = 64,
    [int]$DynamicSlots = 393216,
    [switch]$RecordPipelines,
    [switch]$Panel,
    [ValidateSet('', 'quality', 'balanced', 'performance')][string]$Fsr4K = '',
    [string]$WindowMax = '',
    [switch]$NoLog,
    [ValidateSet('release', 'diag')][string]$Build = 'release',
    [hashtable]$More = @{}
)

$ErrorActionPreference = 'Stop'

if ($GpuTiming -and $Build -eq 'release') {
    Write-Host "Note: -GpuTiming requires the 'diag' build; switching Build -> diag."
    $Build = 'diag'
}

# Check commit memory headroom before starting (world gameplay uses 40-46 GB private commit).
try {
    $os = Get-CimInstance Win32_OperatingSystem
    $freePhysGb = [math]::Round($os.FreePhysicalMemory / 1MB, 1)
    $freeCommitGb = [math]::Round($os.FreeVirtualMemory / 1MB, 1)
    Write-Host ("Memory check: {0} GB free RAM, {1} GB free commit" -f $freePhysGb, $freeCommitGb)
    if ($freeCommitGb -lt 44) {
        Write-Warning "Free Windows commit ($freeCommitGb GB) is tight for open-world play (needs ~40-46 GB). Close heavy apps if crashes.log shows error 1455."
    }
} catch {}

$settings = @{
    # --- Core fast path (avatarmanz latest + RTX 4090 ReBAR tuning) ---
    # NO_WAIT=1 keeps texture streaming non-blocking; 25 ms stage + CATCHUP=1 + 12 ms early
    # drain scene-entry backlogs without 250 ms hitches or perpetually missing textures.
    VICTORY_TEXTURE_NO_WAIT     = $(if ($WaitTextures) { '0' } else { '1' })
    VICTORY_TEXTURE_STAGE_MS    = '25'
    VICTORY_TEXTURE_CATCHUP     = '1'
    VICTORY_EARLY_TEXTURES_MS   = '12'
    VICTORY_BAKE_EXACT          = '1'
    VICTORY_BAKE_SOURCES        = '1'
    VICTORY_FAIR_DUE            = '1'
    VICTORY_SHOW_NEW_LATER      = '1'
    VICTORY_UPLOAD_AHEAD        = '1'
    VICTORY_SHOW_AFTER          = '1'
    VICTORY_VIDEO_PLANES        = '1'
    VICTORY_RELAY               = '2'
    VICTORY_CARD_STEPS          = '2'
    VICTORY_WIDE_BACK           = '1'
    VICTORY_CARD_REHASH_RUNS    = '1'
    VICTORY_JOIN_COPIES         = '1'
    VICTORY_JOIN_BARRIERS       = '1'
    VICTORY_INDIRECT_WINDOW     = '1'
    VICTORY_RELAY_COPY          = '1'
    VICTORY_INDIRECT_BATCH      = '2'
    VICTORY_INDIRECT_REST       = '0'
    VICTORY_LAND_SKIP           = '1'
    VICTORY_LAND_THREADS        = '6'
    VICTORY_MAP_THREADS         = '1'
    VICTORY_TITLE_PRIORITIES    = '0'
    VICTORY_CARD_AWAKE          = 'idle'
    VICTORY_D3D12_DRED          = 'light'
    VICTORY_D3D12_BACKGROUND    = '0'
    VICTORY_MEMORY_MAP          = '0'

    # --- NVIDIA 1M view heap: zero refused tables / zero dropped draws ---
    VICTORY_TABLES_EARLY        = '1'
    VICTORY_DYNAMIC_SLOTS       = "$DynamicSlots"

    # --- Pipeline workers, format variants & watchdog safety ---
    VICTORY_PIPELINE_LOWEST     = '0'
    VICTORY_PIPELINE_WORKERS    = '8'
    VICTORY_PIPELINE_WAIT_ONCE  = '1'
    VICTORY_TARGET_VARIANTS     = '1'
    VICTORY_NO_WATCHDOG         = '1'
    VICTORY_XMA_REFILL          = '1'

    # --- Shader translator (MUST match `victory shaders` and `victory warm`) ---
    # Default SHADER_OPT=63 (bit 64 grouped 4-dword scalar loads blackened water in fxceless run 277 with 0 ms gain).
    VICTORY_SHADER_OPT          = $(if ($WideShaderOpt) { '127' } else { '63' })
    VICTORY_WAVE_WIDE           = 'last'
    VICTORY_NAN_SCRUB           = '1'
}

if (-not $NoPortConstants -and -not $Safe) {
    # Renderer Port 1: in-place 185 MB constants heap (verified 0 forgotten regions, saves ~10,800 fills/frame).
    $settings.VICTORY_PORT_CONSTANTS = '1'
}
if ((-not $NoPortDescriptors -or $PortDescriptors) -and -not $Safe) {
    # Renderer Port 2: 12-byte descriptor header diff + copy_simple mark ordering (99.9993% accuracy, 0.08 misses/frame).
    $settings.VICTORY_PORT_DESCRIPTORS = '1'
}
if (-not $NoPortCbv -and -not $Safe) {
    # Renderer Port 5: read constant buffers in place as raw bytes (~22,000 fills/frame -> 0, -4.7 ms).
    # Note: shader cache keys on VICTORY_PORT_CBV=1 (must match `victory shaders` / `victory warm`).
    $settings.VICTORY_PORT_CBV = '1'
}
if (-not $NoPortGuard -and -not $Safe) {
    # Renderer Port 6: shared write-guard lock with per-64 KiB claims + parallel refresh planning (-2 ms).
    $settings.VICTORY_PORT_GUARD = '1'
}
if (-not $NoPortJournal -and -not $Safe) {
    # Renderer Port 7: early heap sync at title submit + 64-sharded journal locks (-1 to -3 ms).
    $settings.VICTORY_PORT_JOURNAL = '1'
}
if ($TexStore) {
    $texStoreDir = Join-Path $env:LOCALAPPDATA 'victory\cache\textures'
    New-Item -ItemType Directory -Force -Path $texStoreDir | Out-Null
    $settings.VICTORY_TEXTURE_STORE = $texStoreDir
}
if ($CardBuffersMinKb -gt 0) {
    $settings.VICTORY_CARD_BUFFERS_MIN_KB = "$CardBuffersMinKb"
}
if ($WindowsBarMb -gt 0 -and -not $Safe) {
    # Resizable BAR (D3D12_HEAP_TYPE_GPU_UPLOAD): moves loose vertex/index/mesh windows
    # from 25 GB/s L0 System RAM into 1,008 GB/s GDDR6X VRAM (auto-caps at 80% VRAM budget).
    $settings.VICTORY_WINDOWS_BAR_MB = "$WindowsBarMb"
}
if ($WindowKb -gt 0 -and -not $Safe) {
    $settings.VICTORY_WINDOW_KB = "$WindowKb"
}
if ($SendBar -and -not $Safe) {
    $settings.VICTORY_SEND_BAR = '1'
}
if ($Residency) {
    $settings.VICTORY_RESIDENCY = '1'
}
if ($GpuTiming) {
    $settings.VICTORY_GPU_TIMING        = '1'
    $settings.VICTORY_MEMORY_CATEGORIES = '1'
}
if ($UpQueue) {
    $settings.VICTORY_UP_QUEUE      = '1'
    $settings.VICTORY_JOIN_BARRIERS = '1'
}
if ($SweepPerHeap) {
    $settings.VICTORY_SWEEP_PER_HEAP = '1'
}
if ($XmaRefill) {
    $settings.VICTORY_XMA_REFILL = '1'
}
if ($Fps -gt 0.0) {
    $intervalMs = [math]::Max(1.0, [math]::Min(1000.0, 1000.0 / $Fps))
    $settings.VICTORY_FRAME_INTERVAL_MS = ('{0:0.###}' -f $intervalMs).Replace(',', '.')
}

if ($Safe) {
    $settings.VICTORY_UNDER_WIDE = '0'
    $settings.VICTORY_LAND_ECHO  = '0'
    $settings.VICTORY_CARD_STEPS = '0'
    $settings.Remove('VICTORY_UPLOAD_AHEAD')
} else {
    $settings.VICTORY_THIN         = '1'
    $settings.VICTORY_THIN_THREADS = '8'
    $settings.VICTORY_JOIN_PARTS   = '8'
    if ($LaneCore) {
        $settings.VICTORY_LANE_CORE = '1'
    }
}

if ($Quality) {
    $settings.VICTORY_RELAY           = '1'
    $settings.VICTORY_CARD_STEPS      = '0'
    $settings.VICTORY_TEXTURE_NO_WAIT = '0'
}

if (-not $NoAmbient -and -not $RayTracing) {
    $settings.VICTORY_AMBIENT       = '1.0'
    $settings.VICTORY_AMBIENT_AIR   = '1.5'
    $settings.VICTORY_AMBIENT_MOST  = '3.0'
    $settings.VICTORY_AMBIENT_SHADE = '0.7,0.5'
    $settings.VICTORY_AMBIENT_REFL  = '2.0'
}

if ($RayTracing) {
    $settings.VICTORY_BVH_BUILDER    = '1'
    $settings.VICTORY_SKIP_PIPELINES = 'none'
    $settings.VICTORY_SKIP_PREFIXES  = 'none'
} else {
    $settings.VICTORY_BVH_NO_HIT     = '1'
    if ($SkipRtAggressive) {
        # Tier B RT skip (adds ray-binning + RaytracingResolve on top of auto Tier-A pure-BVH skips;
        # ambient.rs hooks CS_ScreenProbes_TemporalResolve / CS_RayMiss remain immune in pipeline.rs).
        $settings.VICTORY_SKIP_PREFIXES = 'CS_RaytracingResolve,CS_RayBudgeting_,CS_RaytraceVBuffer_,CS_ScreenProbes_TraceRays_,CS_GenerateRayCoords,CS_GenerateRayBins,CS_GenerateRayBinOffsets,CS_ClassifyRayTiles,CS_SortRayTiles,CS_ResetRayTileArgs,CS_ClearRayBins,CS_ClearRayCounts'
    }
    if ($SkipRtHashes) {
        $rtHashesPath = Join-Path $PSScriptRoot 'rt_off_hashes.txt'
        if (Test-Path -LiteralPath $rtHashesPath) {
            $settings.VICTORY_SKIP_PIPELINES = (Get-Content -LiteralPath $rtHashesPath -Raw).Trim()
        } else {
            $settings.VICTORY_SKIP_PIPELINES = 'none'
        }
    } else {
        $settings.VICTORY_SKIP_PIPELINES = 'none'
    }
}

if ($Fsr4K) {
    # Native PC FidelityFX bridge (FSR 3.1.x on RTX 4090 / FSR 4.x on RDNA 4, 4K output).
    $settings.VICTORY_FSR_4K = $Fsr4K.ToLowerInvariant()
} elseif ($SeriesS) {
    $settings.VICTORY_PLANES   = '1'
    $settings.VICTORY_UPSCALER = $(if ($FidelityFx) { '7' } else { '6' })
    # Series S at scale 1.0 hits a title breakpoint on start (trustno1-coder 033be27f); default to 720p internal (0.667).
    if ($Scale -le 0.0) { $Scale = 0.667 }
} else {
    # Force the title's own temporal upscaler (6) by default so FidelityFX does not write unpatched
    # descriptors under VICTORY_PORT_DESCRIPTORS=1.
    $settings.VICTORY_UPSCALER = $(if ($FidelityFx) { '7' } else { '6' })
    if ($Full1440p) {
        $settings.VICTORY_OUTPUT = '2560x1440'
    } elseif (-not $Full4K) {
        # Upstream native output scaling: 1920x1080 output on Series X (~16 ms less GPU work vs 4K).
        $settings.VICTORY_OUTPUT = '1920x1080'
    }
}

if ($Scale -gt 0.0) {
    # At 3840x2160 (-Full4K) or Series S, >0.667 overflows the title's RT allocator pool (int3 at +0x2219650).
    # At 1920x1080 or 2560x1440 output (`VICTORY_OUTPUT`), scale up to 1.0 fits inside the Series X RT pool.
    $maxScale = if ($SeriesS -or $Full4K) { 0.667 } else { 1.0 }
    if ($Scale -gt $maxScale) {
        Write-Warning ("Scale {0} exceeds safe RT pool max ({1}) for this output mode. Clamping to {1}." -f $Scale, $maxScale)
        $Scale = $maxScale
    }
    $clamped = [math]::Min($maxScale, [math]::Max(0.5, $Scale))
    $settings.VICTORY_RENDER_SCALE = ('{0:0.###}' -f $clamped).Replace(',', '.')
}

if ($RecordPipelines) { $settings.VICTORY_PIPELINE_BOOK = '1' }
if ($Panel)           { $settings.VICTORY_PANEL         = '1' }
if ($WindowMax)       { $settings.VICTORY_WINDOW_MAX    = $WindowMax }

foreach ($k in $More.Keys) { $settings[$k] = "$($More[$k])" }

$victory = Join-Path $PSScriptRoot "..\target\$Build\victory.exe"
if (-not (Test-Path -LiteralPath $victory)) {
    $hint = if ($Build -eq 'diag') {
        "cargo build --locked --profile diag -p victory --features diagnostics"
    } else {
        "cargo build --locked --release -p victory"
    }
    throw "No $victory. Build first: $hint"
}
if (-not $Exe) {
    $gameDir = $env:VICTORY_GAME_DIR
    if (-not $gameDir) {
        $cfgPath = if ($env:VICTORY_CONFIG) { $env:VICTORY_CONFIG } else { Join-Path $PSScriptRoot '..\victory.toml' }
        if (Test-Path -LiteralPath $cfgPath) {
            $m = Select-String -LiteralPath $cfgPath -Pattern "^\s*game_dir\s*=\s*['""]([^'""]+)['""]" | Select-Object -First 1
            if ($m) { $gameDir = $m.Matches[0].Groups[1].Value }
        }
    }
    if ($gameDir -and (Test-Path -LiteralPath $gameDir)) {
        $preferred = Join-Path $gameDir 'game_scarlett_master.exe'
        if (Test-Path -LiteralPath $preferred) {
            $Exe = $preferred
        } else {
            $firstExe = Get-ChildItem -LiteralPath $gameDir -Filter '*.exe' -File -ErrorAction SilentlyContinue | Select-Object -First 1
            if ($firstExe) { $Exe = $firstExe.FullName }
        }
    }
}
if (-not $Exe -or -not (Test-Path -LiteralPath $Exe)) {
    throw "No title executable found. Pass -Exe <path\to\executable.exe> or set game_dir in victory.toml (copy victory.example.toml -> victory.toml)."
}

# Read-only pre-flight GPU check: warn if another CUDA/AI job is occupying the RTX 4090.
try {
    $gpuCsv = (& nvidia-smi --query-gpu=utilization.gpu,memory.used,memory.total,power.draw --format=csv,noheader,nounits 2>$null | Select-Object -First 1)
    if ($gpuCsv) {
        $parts = $gpuCsv.Split(',') | ForEach-Object { $_.Trim() }
        $util = [double]$parts[0]
        $usedMb = [double]$parts[1]
        $totalMb = [double]$parts[2]
        $watts = $parts[3]
        if ($util -ge 25 -or $usedMb -ge 4096) {
            Write-Warning ("RTX 4090 is already busy before launch: {0:0}% GPU, {1:0}/{2:0} MiB VRAM, {3} W. Another CUDA/AI process may cut FPS and cause VRAM paging." -f $util, $usedMb, $totalMb, $watts)
        }
    }
} catch {}

if ($SmoothMotion) {
    $smCopy = Join-Path (Split-Path $victory -Parent) 'victory_sm.exe'
    Copy-Item -LiteralPath $victory -Destination $smCopy -Force
    $victory = $smCopy
    Write-Host "Smooth Motion copy ready: $victory (enable Smooth Motion for victory_sm.exe in NVIDIA App)"
}

$keep = 'VICTORY_CONFIG', 'VICTORY_GAME_DIR', 'VICTORY_CACHE_DIR', 'VICTORY_DATA_DIR'
Get-ChildItem Env:VICTORY_* -ErrorAction SilentlyContinue |
    Where-Object { $_.Name -notin $keep } |
    ForEach-Object { Remove-Item "Env:$($_.Name)" }

foreach ($k in $settings.Keys) {
    Set-Item "Env:$k" $settings[$k]
}

$Exe = (Resolve-Path -LiteralPath $Exe).Path
$consoleArg = if ($SeriesS) { 'series-s' } else { 'series-x' }
$mode = if ($SeriesS) { 'Series S 1080p+4K HUD' } elseif ($Full4K) { 'Series X 3840x2160' } elseif ($Full1440p) { 'Series X 2560x1440' } else { 'Series X 1920x1080' }
if ($Scale -gt 0.0) { $mode += " @ scale $($settings.VICTORY_RENDER_SCALE)" }
if ($Fps -gt 0.0) { $mode += ", cap ${Fps} FPS" }
Write-Host "Starting VIctory-RTX-Mc3yohan ($Build, $mode) on NVIDIA RTX 4090 with $($settings.Count) switches..."

$runArgs = @('run', $Exe, '--console', $consoleArg, '--execute')

Push-Location (Join-Path $PSScriptRoot '..')
$prevEap = $ErrorActionPreference
$ErrorActionPreference = 'Continue'
try {
    if (-not $NoLog) {
        $dataDir = if ($env:VICTORY_DATA_DIR) { $env:VICTORY_DATA_DIR } else { Join-Path $env:LOCALAPPDATA 'victory\data' }
        New-Item -ItemType Directory -Force -Path $dataDir | Out-Null
        $stamp = Get-Date -Format 'yyyyMMdd-HHmmss'
        $logPath = Join-Path $dataDir "nvidia-run-$stamp.log"
        Write-Host "Logging to $logPath"
        $fs = [System.IO.File]::Open($logPath, [System.IO.FileMode]::Create, [System.IO.FileAccess]::Write, [System.IO.FileShare]::ReadWrite)
        $sw = [System.IO.StreamWriter]::new($fs, [System.Text.UTF8Encoding]::new($false))
        $sw.AutoFlush = $true
        try {
            & $victory @runArgs 2>&1 | ForEach-Object {
                $line = if ($_ -is [System.Management.Automation.ErrorRecord]) { $_.Exception.Message } else { "$_" }
                Write-Host $line
                $sw.WriteLine($line)
            }
        } finally {
            $sw.Dispose()
            $fs.Dispose()
        }
    } else {
        & $victory @runArgs
    }
} finally {
    $ErrorActionPreference = $prevEap
    Pop-Location
}
