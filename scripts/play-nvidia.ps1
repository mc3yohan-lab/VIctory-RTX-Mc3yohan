# Launches the Xbox Series title tuned for NVIDIA RTX 4090 (24 GB VRAM, Wave32, 1M view heap)
# and Intel Core i9-12900KF (8P/16T + 8E/8T), combining the best of:
# - avatarmanz/VIctory-7900XTX latest (VICTORY_SHOW_NEW_LATER, AVX2 SYNC_IN_PLACE, SPANS_QUICK,
#   UNDER_FUSED, METER_THREAD, LANE_CORE, upq copy queue)
# - Korinocho/victory-fork (22.2+ FPS 1440p perf set + VICTORY_TABLES_EARLY=1)
# - trustno1-coder/VIctory-NVIDIA (Faces move PR #927, VICTORY_NAN_SCRUB=1, rt_off_hashes.txt,
#   VICTORY_FAIR_DUE=1, VICTORY_VIDEO_PLANES=1, VICTORY_TARGET_VARIANTS=1, hooks.rs render scale /
#   own upscaler / watchdog disarm, Series S 4K HUD compose.rs, Smooth Motion)
# - Our RTX 4090 work: wide indirect (>65,535), MED3|LEGACY float shortcuts, VICTORY_BVH_NO_HIT=1,
#   VICTORY_WAVE_WIDE=last, O(1) VICTORY_SKIP_PIPELINES in release, coalesced VirtualProtect, warm host-pipelines.bin.
#
# Usage:
#   scripts\play-nvidia.ps1                        # Series X 1440p native (clean textures, bounded ambient, RT-off hashes, release)
#   scripts\play-nvidia.ps1 -Scale 0.75            # Series X 1440p @ 75% internal + temporal upscaler (push ~30 FPS)
#   scripts\play-nvidia.ps1 -SeriesS               # Series S 1080p + 4K sharp HUD @ 720p internal (0.667)
#   scripts\play-nvidia.ps1 -SeriesS -Scale 0.5    # Max FPS Series S (540p internal + 4K HUD)
#   scripts\play-nvidia.ps1 -SmoothMotion          # Launches victory_sm.exe for NVIDIA App Smooth Motion (2x visual FPS)
#   scripts\play-nvidia.ps1 -FastTextures          # Sets VICTORY_TEXTURE_NO_WAIT=1 (slightly faster, may cause rainbow textures)
#   scripts\play-nvidia.ps1 -NoAmbient             # Disables indirect light stand-in
#   scripts\play-nvidia.ps1 -UpQueue               # Experimental dedicated D3D12 copy queue + joined barriers
#   scripts\play-nvidia.ps1 -Panel                 # Live diagnostics overlay (auto-switches to diag build)
#   scripts\play-nvidia.ps1 -Quality               # Conservative sync (RELAY=1, CARD_STEPS=0)
#   scripts\play-nvidia.ps1 -Safe                  # Safe fallback path
#   scripts\play-nvidia.ps1 -Full4K                # Series X 3840x2160 native output
#   scripts\play-nvidia.ps1 -NoLog                 # Disable automatic log capture
#
# Run from a normal (non-elevated) PowerShell window so NVIDIA keeps its DXCache in the usual place.
param(
    [string]$Exe = 'C:\Users\Yohan\Downloads\VersionXbox\game_scarlett_master.exe',
    [switch]$SeriesS,
    [double]$Scale = 0.0,
    [switch]$Full4K,
    [switch]$FastTextures,
    [switch]$NoAmbient,
    [switch]$UpQueue,
    [switch]$Quality,
    [switch]$Safe,
    [switch]$RayTracing,
    [switch]$SmoothMotion,
    [switch]$NoWideShaderOpt,
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
    VICTORY_TEXTURE_NO_WAIT     = $(if ($FastTextures) { '1' } else { '0' })
    VICTORY_TEXTURE_STAGE_MS    = '40'
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
    VICTORY_INDIRECT_WINDOW     = '1'
    VICTORY_RELAY_COPY          = '1'
    VICTORY_INDIRECT_BATCH      = '2'
    VICTORY_INDIRECT_REST       = '0'
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
    VICTORY_WAVE_WIDE           = 'last'
    VICTORY_NAN_SCRUB           = '1'
}

if (-not $NoWideShaderOpt) {
    $settings.VICTORY_SHADER_OPT = '127'
}
if ($CardBuffersMinKb -gt 0) {
    $settings.VICTORY_CARD_BUFFERS_MIN_KB = "$CardBuffersMinKb"
}
if ($UpQueue) {
    $settings.VICTORY_UP_QUEUE      = '1'
    $settings.VICTORY_JOIN_BARRIERS = '1'
}

if ($Safe) {
    $settings.VICTORY_UNDER_WIDE = '0'
    $settings.VICTORY_LAND_ECHO  = '0'
    $settings.Remove('VICTORY_UPLOAD_AHEAD')
} else {
    $settings.VICTORY_THIN       = '1'
    $settings.VICTORY_JOIN_PARTS = '8'
}

if ($Quality) {
    $settings.VICTORY_RELAY           = '1'
    $settings.VICTORY_CARD_STEPS      = '0'
    $settings.VICTORY_TEXTURE_NO_WAIT = '0'
}

if (-not $NoAmbient) {
    $settings.VICTORY_AMBIENT      = '0.8'
    $settings.VICTORY_AMBIENT_AIR  = '1.5'
    $settings.VICTORY_AMBIENT_MOST = '2'
}

if ($RayTracing) {
    $settings.VICTORY_BVH_BUILDER    = '1'
    $settings.VICTORY_SKIP_PIPELINES = 'none'
} else {
    $settings.VICTORY_BVH_NO_HIT     = '1'
    $rtHashesPath = Join-Path $PSScriptRoot 'rt_off_hashes.txt'
    if (Test-Path -LiteralPath $rtHashesPath) {
        $settings.VICTORY_SKIP_PIPELINES = (Get-Content -LiteralPath $rtHashesPath -Raw).Trim()
    } else {
        $settings.VICTORY_SKIP_PIPELINES = 'none'
    }
}

if ($SeriesS) {
    $settings.VICTORY_COMPOSE      = '1'
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
