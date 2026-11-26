# Launches the Xbox Series title tuned for NVIDIA RTX 4090 (24 GB VRAM, Wave32, 1M view heap)
# and Intel Core i9-12900KF (8P/16T + 8E/8T), combining the best of:
# - avatarmanz/VIctory-7900XTX latest (`722c0e1`): Renderer Patches 1+2 (VICTORY_PORT_CONSTANTS,
#   VICTORY_PORT_DESCRIPTORS), all 4 Wave32 NVIDIA RT/BVH/tiled-cascade-shadow PRs (#4,#66,#82,#142),
#   FPS unlock (VICTORY_FRAME_INTERVAL_MS), two-plane compose (VICTORY_PLANES), TablePlan, PR #24/#121,
#   VICTORY_SHOW_NEW_LATER, AVX2 SYNC_IN_PLACE, SPANS_QUICK, UNDER_FUSED, LANE_CORE, upq copy queue.
# - fxceless/victory-fork: Faces move (PR #927), legacy FMA water-reflection fix (PR #1026),
#   32-bit int typed views (PR #684), VICTORY_TARGET_VARIANTS=1, xgamesave resilience.
# - trustno1-coder/VIctory-NVIDIA + Korinocho + DexNom: VICTORY_NAN_SCRUB=1, rt_off_hashes.txt,
#   VICTORY_FAIR_DUE=1, VICTORY_VIDEO_PLANES=1, VICTORY_TABLES_EARLY=1, hooks.rs render scale /
#   own upscaler / watchdog disarm, Wave64 mask recovery (PR #155), Smooth Motion.
# - Our RTX 4090 work: wide indirect (>65,535), MED3|LEGACY float shortcuts, VICTORY_BVH_NO_HIT=1,
#   VICTORY_WAVE_WIDE=last, O(1) VICTORY_SKIP_PIPELINES in release, PR #23 shadow::exact guard,
#   coalesced VirtualProtect, warm host-pipelines.bin.
#
# Usage:
#   scripts\play-nvidia.ps1                        # Series X 1440p + Patch 1 (fast textures, ambient settle, 0 indirect drops)
#   scripts\play-nvidia.ps1 -Fps 60                # Unlock 30 FPS cap to 60 FPS (game clock stays 1.00x)
#   scripts\play-nvidia.ps1 -TexStore              # Enable disk cache of sharp mip levels (faster texture streaming on revisits)
#   scripts\play-nvidia.ps1 -RayTracing            # Test real Wave32 NVIDIA Ray Tracing + exact cascade shadows
#   scripts\play-nvidia.ps1 -SeriesS               # Series S 1080p + 4K sharp HUD @ 720p internal (0.667)
#   scripts\play-nvidia.ps1 -Scale 0.75            # Series X 1440p @ 75% internal + temporal upscaler
#   scripts\play-nvidia.ps1 -SmoothMotion          # Launches victory_sm.exe for NVIDIA App Smooth Motion (2x visual FPS)
#   scripts\play-nvidia.ps1 -WaitTextures          # Sets VICTORY_TEXTURE_NO_WAIT=0 (slower streaming, avoids partial BCn blocks)
#   scripts\play-nvidia.ps1 -SkipRtHashes          # Diagnostic only: skips 1,034 rt_off_hashes (kills indoor voxel bounce GI)
#   scripts\play-nvidia.ps1 -PortDescriptors       # Renderer Patch 2 (11/11 title descriptor store sites hooked + race fixed)
#   scripts\play-nvidia.ps1 -NoPortConstants       # Disable Renderer Patch 1 (VICTORY_PORT_CONSTANTS)
#   scripts\play-nvidia.ps1 -UpQueue               # Experimental dedicated D3D12 copy queue + joined barriers
#   scripts\play-nvidia.ps1 -SweepPerHeap          # Coolfox per-heap buffer pass (VICTORY_SWEEP_PER_HEAP=1)
#   scripts\play-nvidia.ps1 -NoAmbient             # Disables indirect light stand-in
#   scripts\play-nvidia.ps1 -Panel                 # Live diagnostics overlay (auto-switches to diag build)
#   scripts\play-nvidia.ps1 -Quality               # Conservative sync (RELAY=1, CARD_STEPS=0, NO_WAIT=0)
#   scripts\play-nvidia.ps1 -Safe                  # Safe fallback path (disables port patches and thin lists)
#   scripts\play-nvidia.ps1 -Full4K                # Series X 3840x2160 native output
#   scripts\play-nvidia.ps1 -NoLog                 # Disable automatic log capture
#
# Run from a normal (non-elevated) PowerShell window so NVIDIA keeps its DXCache in the usual place.
param(
    [string]$Exe = 'C:\Users\Yohan\Downloads\VersionXbox\game_scarlett_master.exe',
    [switch]$SeriesS,
    [double]$Scale = 0.0,
    [double]$Fps = 0.0,
    [switch]$Full4K,
    [switch]$WaitTextures,
    [switch]$TexStore,
    [switch]$SkipRtHashes,
    [switch]$LaneCore,
    [switch]$NoAmbient,
    [switch]$NoPortConstants,
    [switch]$PortDescriptors,
    [switch]$UpQueue,
    [switch]$SweepPerHeap,
    [switch]$XmaRefill,
    [switch]$Quality,
    [switch]$Safe,
    [switch]$RayTracing,
    [switch]$SmoothMotion,
    [switch]$WideShaderOpt,
    [int]$CardBuffersMinKb = 64,
    [int]$DynamicSlots = 393216,
    [switch]$RecordPipelines,
    [switch]$Panel,
    [string]$WindowMax = '',
    [switch]$NoLog,
    [ValidateSet('release', 'diag')][string]$Build = 'release',
    [hashtable]$More = @{}
)

$ErrorActionPreference = 'Stop'

if ($Panel -and $Build -eq 'release') {
    Write-Host "Note: -Panel requires the 'diag' build; switching Build -> diag."
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
    # --- Core fast path (avatarmanz latest + Korinocho perf set) ---
    # NO_WAIT=1 keeps texture streaming at ~0.6-1.4 ms/frame (NO_WAIT=0 caused 50-377 ms stalls);
    # SHOW_NEW_LATER=1 + FAIR_DUE=1 prevent pooled-slot corruption without stalling the stream.
    VICTORY_TEXTURE_NO_WAIT     = $(if ($WaitTextures) { '0' } else { '1' })
    VICTORY_TEXTURE_STAGE_MS    = '8'
    VICTORY_TEXTURE_CATCHUP     = '0'
    VICTORY_EARLY_TEXTURES_MS   = '3'
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
    VICTORY_LAND_THREADS        = '4'
    VICTORY_MAP_THREADS         = '1'
    VICTORY_TITLE_PRIORITIES    = '0'
    VICTORY_CARD_AWAKE          = '1'
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

    # --- Shader translator (MUST match `victory shaders` and `victory warm`) ---
    # Default SHADER_OPT=63 (bit 64 grouped 4-dword scalar loads blackened water in fxceless run 277 with 0 ms gain).
    VICTORY_SHADER_OPT          = $(if ($WideShaderOpt) { '127' } else { '63' })
    VICTORY_WAVE_WIDE           = 'last'
    VICTORY_NAN_SCRUB           = '1'
}

if (-not $NoPortConstants -and -not $Safe) {
    # Renderer Patch 1 (avatarmanz): in-place 185 MB constants heap (verified 0 forgotten regions, saves ~10,800 fills/frame).
    $settings.VICTORY_PORT_CONSTANTS = '1'
}
if ($PortDescriptors -and -not $Safe) {
    # Renderer Patch 2: opt-in only (missed 78K-137K view records from unpatched title memcpy/writers in prologue).
    $settings.VICTORY_PORT_DESCRIPTORS = '1'
}
if ($TexStore) {
    $texStoreDir = Join-Path $env:LOCALAPPDATA 'victory\cache\textures'
    New-Item -ItemType Directory -Force -Path $texStoreDir | Out-Null
    $settings.VICTORY_TEXTURE_STORE = $texStoreDir
}
if ($CardBuffersMinKb -gt 0) {
    $settings.VICTORY_CARD_BUFFERS_MIN_KB = "$CardBuffersMinKb"
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
    $settings.VICTORY_AMBIENT      = '0.8'
    $settings.VICTORY_AMBIENT_AIR  = '1.5'
    $settings.VICTORY_AMBIENT_MOST = '2'
    $settings.VICTORY_AMBIENT_REFL = '2.0'
}

if ($RayTracing) {
    $settings.VICTORY_BVH_BUILDER    = '1'
    $settings.VICTORY_SKIP_PIPELINES = 'none'
} else {
    $settings.VICTORY_BVH_NO_HIT     = '1'
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

if ($SeriesS) {
    $settings.VICTORY_PLANES       = '1'
    $settings.VICTORY_OWN_UPSCALER = '1'
    # Series S at scale 1.0 hits a title breakpoint on start (trustno1-coder 033be27f); default to 720p internal (0.667).
    if ($Scale -le 0.0) { $Scale = 0.667 }
} elseif (-not $Full4K) {
    $settings.VICTORY_DISPLAY_MODE = '2560x1440'
}

if ($Scale -gt 0.0) {
    $clamped = [math]::Min(1.0, [math]::Max(0.2, $Scale))
    $settings.VICTORY_RENDER_SCALE = ('{0:0.###}' -f $clamped).Replace(',', '.')
    $settings.VICTORY_OWN_UPSCALER = '1'
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
if (-not (Test-Path -LiteralPath $Exe)) {
    throw "No executable at $Exe"
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
$mode = if ($SeriesS) { 'Series S 1080p+4K HUD' } elseif ($Full4K) { 'Series X 3840x2160' } else { 'Series X 2560x1440' }
if ($Scale -gt 0.0) { $mode += " @ scale $($settings.VICTORY_RENDER_SCALE)" }
Write-Host "Starting VIctory ($Build, $mode) on NVIDIA RTX 4090 with $($settings.Count) switches..."

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
