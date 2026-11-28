<#
.SYNOPSIS
Compara dos registros locales de Victory RTX sin modificarlos.
.DESCRIPTION
Los tiempos se miden desde el primer resumen de FPS de cada registro. Se incluye
el resumen cuyo timestamp cae en el tramo elegido. Los draw calls por cuadro
acotan la carga y no identifican una escena. Para un A/B, usa la misma ruta,
consola, resolución, escala y estado de caché, con un solo cambio intencional.
Con VICTORY_PRESENT_TIMES=1 antes de iniciar el motor, el log incluye
intervalos entre presentaciones aceptadas por DXGI para percentiles por cuadro.
IntervalsOver50Ms cuenta los cuadros por debajo de 20 FPS. SampledIntervalSeconds
suma la duración de los intervalos seleccionados, que pueden pertenecer a tramos separados.
.PARAMETER UntilWindowClose
Sólo incluye resúmenes, avisos y lotes fechados hasta el primer WM_CLOSE del motor,
incluida esa marca de tiempo. Cada registro debe tener la marca de cierre.
.EXAMPLE
.\scripts\compare-nvidia-runs.ps1 -Baseline 'C:\logs\A.log' -Candidate 'C:\logs\B.log' -MinDraws 3500 -MaxDraws 5000
.EXAMPLE
.\scripts\compare-nvidia-runs.ps1 A.log B.log -BaselineFromSeconds 60 -CandidateFromSeconds 70 -DurationSeconds 90 -AsJson
#>
[CmdletBinding()]
param(
    [Parameter(Mandatory, Position = 0)] [string] $Baseline,
    [Parameter(Mandatory, Position = 1)] [string] $Candidate,
    [ValidateRange(0, 2147483647)] [int] $MinDraws = 3500,
    [ValidateRange(0, 2147483647)] [int] $MaxDraws = 0,
    [ValidateRange(0, 2147483647)] [int] $BaselineFromSeconds = 0,
    [ValidateRange(0, 2147483647)] [int] $CandidateFromSeconds = 0,
    [ValidateRange(0, 2147483647)] [int] $DurationSeconds = 0,
    [switch] $AsJson,
    [switch] $UntilWindowClose
)

$ErrorActionPreference = 'Stop'
if ($MaxDraws -ne 0 -and $MaxDraws -lt $MinDraws) {
    throw 'MaxDraws debe ser igual o superior a MinDraws, o cero para omitir el límite superior.'
}

$number = '\d+(?:\.\d+)?'
$perfPattern = "(?<fps>$number) fps,\s*(?<ms>$number) ms/frame.*?,\s*(?<draws>\d+) draws/frame"
$cadencePattern = "present cadence\s+accepted=(?<accepted>\d+)\s+occluded=(?<occluded>\d+)\s+rejected=(?<rejected>\d+)\s+overflow=(?<overflow>\d+)\s+draws_per_frame=(?<draws>$number)\s+intervals_us=\[(?<intervals>[\d, ]*)\]"
$warningPatterns = [ordered]@{
    TexturesReuploaded = 'a (?:small )?texture picked by index changed after'
    MissingBakeSources = 'a skin-bake draw samples textures that are not on the card'
    DescriptorAllocationWarnings = 'no free dynamic descriptor slots: a table is not made'
    DescriptorDrawWarnings = 'a draw whose descriptor table found no free slots is dropped'
    ShaderTranslationFailures = "shader does not translate; its pipelines' draws are dropped"
    TextureCopyBackTilingFailures = 'a texture copied back could not be tiled'
}
$headerPattern = '^\s*(?<field>GPU|Perfil|Build)\s*:\s*(?<value>.+)$'
$scriptHeaderPattern = '^\s*Starting VIctory-RTX-Mc3yohan \((?<build>release|diag), (?<profile>.+)\) on .+ with \d+ switches'
$commitPattern = '^\s*fork\s+VIctory-RTX-Mc3yohan\s+\((?<commit>[0-9a-fA-F]+|local)\)'
$adapterPattern = 'host D3D12 adapter adapter="(?<adapter>[^"]+)"'
$consolePattern = '^\s*console\s+(?<console>Xbox Series [XS])\b'
$presentPattern = 'presenting to a window width=(?<width>\d+) height=(?<height>\d+)'
$scalePattern = 'what="the title''s own upscaler''s render scale" value="(?<scale>[\d.]+)"'
$digestPattern = '^\s*Motor\s*:[^|]*\|\s*BLAKE3\s+(?<digest>[0-9a-fA-F]{64})\b'
$intentionalPattern = 'dropped [\d.]+/frame without a host pipeline: (?:skipped shader|a stage of the ray-tracing structure builder, which is off)'
$linePatterns = @(
    $perfPattern, $headerPattern, $scriptHeaderPattern, $commitPattern, $consolePattern, $digestPattern, $intentionalPattern,
    'host D3D12 adapter', 'presenting to a window', 'render scale', '\bfork\s+', 'present cadence'
) + @($warningPatterns.Values)
$culture = [Globalization.CultureInfo]::InvariantCulture
$ansiPattern = '\x1B\[[0-?]*[ -/]*[@-~]'

function Get-LogTime([string] $Line) {
    if ($Line -match '^\s*(?<time>\d{4}-\d{2}-\d{2}T\d{2}:\d{2}:\d{2}(?:\.\d+)?Z)\b') {
        $stamp = [DateTimeOffset]::MinValue
        if ([DateTimeOffset]::TryParse($Matches.time, $culture, [Globalization.DateTimeStyles]::None, [ref]$stamp)) {
            return $stamp
        }
    }
    return $null
}

function Test-TimeSlice($Time, $FirstWindow, [int] $From, [int] $Duration, $CloseTime) {
    if ($null -ne $CloseTime -and ($null -eq $Time -or $Time -gt $CloseTime)) { return $false }
    if ($From -eq 0 -and $Duration -eq 0) { return $true }
    if ($null -eq $Time -or $null -eq $FirstWindow) { return $false }
    $seconds = ($Time - $FirstWindow).TotalSeconds
    return ($seconds -ge $From -and ($Duration -eq 0 -or $seconds -lt ([double]$From + $Duration)))
}

function Get-Median([double[]] $Values) {
    if ($Values.Count -eq 0) { return $null }
    $sorted = @($Values | Sort-Object)
    $middle = [int][Math]::Floor($sorted.Count / 2)
    if ($sorted.Count % 2) { return $sorted[$middle] }
    return ($sorted[$middle - 1] + $sorted[$middle]) / 2
}

function Get-NativeCadence($Batches, $FirstWindow, [int] $From, $CloseTime) {
    if ($Batches.Count -eq 0) { return $null }
    $intervals = [Collections.Generic.List[long]]::new()
    $accepted = 0L
    $occluded = 0L
    $rejected = 0L
    $timeOverflow = 0L
    $overflow = 0L
    $atMost40 = 0L
    $atMost33 = 0L
    $above50 = 0L
    $totalUs = 0L
    $selected = 0

    foreach ($batch in $Batches) {
        if (-not (Test-TimeSlice $batch.At $FirstWindow $From $DurationSeconds $CloseTime)) { continue }
        $occluded += $batch.Occluded
        $rejected += $batch.Rejected
        $timeOverflow += $batch.Overflow
        if ($batch.Draws -lt $MinDraws -or ($MaxDraws -gt 0 -and $batch.Draws -gt $MaxDraws)) { continue }
        $selected++
        $accepted += $batch.Accepted
        $overflow += $batch.Overflow
        if ($batch.Intervals.Length -eq 0) { continue }
        foreach ($sample in ($batch.Intervals -split ',\s*')) {
            $us = [long]$sample
            $intervals.Add($us)
            $totalUs += $us
            if ($us -gt 50000) { $above50++ }
            if ($us -le 40000) { $atMost40++ }
            if ($us -le 33333) { $atMost33++ }
        }
    }

    $p95 = $null
    $p99 = $null
    $pct40 = $null
    $pct33 = $null
    $pctAbove50 = $null
    $worstMs = $null
    if ($intervals.Count -gt 0) {
        $intervals.Sort()
        $p95 = $intervals[[int][Math]::Ceiling(0.95 * $intervals.Count) - 1] / 1000.0
        $p99 = $intervals[[int][Math]::Ceiling(0.99 * $intervals.Count) - 1] / 1000.0
        $pct40 = [Math]::Round(100.0 * $atMost40 / $intervals.Count, 1)
        $pct33 = [Math]::Round(100.0 * $atMost33 / $intervals.Count, 1)
        $pctAbove50 = [Math]::Round(100.0 * $above50 / $intervals.Count, 2)
        $worstMs = $intervals[$intervals.Count - 1] / 1000.0
    }
    [pscustomobject]@{
        SelectedBatches = $selected
        AcceptedPresentsInSelectedBatches = $accepted
        IntervalCount = $intervals.Count
        PresentP95Ms = $p95
        PresentP99Ms = $p99
        LongestIntervalMs = $worstMs
        SampledIntervalSeconds = $totalUs / 1000000.0
        IntervalsOver50Ms = $above50
        PercentOver50Ms = $pctAbove50
        IntervalsAtMost40Ms = $atMost40
        PercentAtMost40Ms = $pct40
        IntervalsAtMost33_333Ms = $atMost33
        PercentAtMost33_333Ms = $pct33
        OccludedInTimeSlice = $occluded
        RejectedInTimeSlice = $rejected
        OverflowIntervalsInTimeSlice = $timeOverflow
        OverflowIntervalsInSelectedBatches = $overflow
    }
}

function Read-Run([string] $LiteralPath, [int] $From) {
    $item = Get-Item -LiteralPath $LiteralPath
    if ($item.PSIsContainer) { throw "Elige un archivo de registro, no una carpeta: $($item.Name)" }
    $closeTime = $null
    if ($UntilWindowClose) {
        $marker = 'window closed (WM_CLOSE): suspending the title'
        $closeTime = Select-String -LiteralPath $item.FullName -SimpleMatch $marker -Encoding UTF8 | ForEach-Object {
            $line = $_.Line -replace $ansiPattern, ''
            if ($line -match 'victory_hle::modules::lifecycle:\s+window closed \(WM_CLOSE\): suspending the title\b') {
                Get-LogTime $line
            }
        } | Where-Object { $null -ne $_ } | Select-Object -First 1
        if ($null -eq $closeTime) { throw "Falta la marca de tiempo de WM_CLOSE del motor en $($item.Name)." }
    }
    $metadata = [ordered]@{
        RuntimeAdapter = $null; Console = $null; PresentSize = $null; RenderScale = $null
        GPU = $null; Perfil = $null; Build = $null; RuntimeCommit = $null; BinaryBlake3 = $null
    }
    $quality = [ordered]@{}
    foreach ($kind in $warningPatterns.Keys) { $quality[$kind] = 0 }
    $quality.ReportedIntentionalSkipReasons = 0
    $events = [Collections.Generic.List[object]]::new()
    $windows = [Collections.Generic.List[object]]::new()
    $cadenceBatches = [Collections.Generic.List[object]]::new()
    $firstWindow = $null
    $windowCount = 0
    $untimedWindows = 0
    $timeCount = 0
    $lastDescriptorDrops = $null

    Select-String -LiteralPath $item.FullName -Pattern $linePatterns -Encoding UTF8 | ForEach-Object {
        $line = $_.Line -replace $ansiPattern, ''
        if ($line -match $perfPattern) {
            $fps = [double]::Parse($Matches.fps, $culture)
            $ms = [double]::Parse($Matches.ms, $culture)
            $draws = [long]$Matches.draws
            $windowCount++
            $at = Get-LogTime $line
            if ($null -eq $at) { $untimedWindows++ }
            elseif ($null -eq $firstWindow) { $firstWindow = $at }

            if (Test-TimeSlice $at $firstWindow $From $DurationSeconds $closeTime) {
                $timeCount++
                if ($line -match 'commands dropped while making descriptor tables since start:\s*(\d+)') {
                    $lastDescriptorDrops = [long]$Matches[1]
                }
                if ($draws -ge $MinDraws -and ($MaxDraws -eq 0 -or $draws -le $MaxDraws)) {
                    $drops = $null
                    if ($line -match 'dropped (\d+) without a host pipeline\b') { $drops = [long]$Matches[1] }
                    $windows.Add([pscustomobject]@{ FPS = $fps; Ms = $ms; Draws = $draws; PipelineDrops = $drops })
                }
            }
        } elseif ($line -match $cadencePattern) {
            $cadenceBatches.Add([pscustomobject]@{
                At = Get-LogTime $line
                Accepted = [long]$Matches.accepted
                Occluded = [long]$Matches.occluded
                Rejected = [long]$Matches.rejected
                Overflow = [long]$Matches.overflow
                Draws = [double]::Parse($Matches.draws, $culture)
                Intervals = $Matches.intervals
            })
        } elseif ($line -match $headerPattern) {
            $metadata[$Matches.field] = $Matches.value.Trim()
        } elseif ($line -match $scriptHeaderPattern) {
            $metadata.Build = $Matches.build
            $metadata.Perfil = $Matches.profile
        } elseif ($line -match $commitPattern) {
            $metadata.RuntimeCommit = $Matches.commit
        } elseif ($line -match $adapterPattern) {
            $metadata.RuntimeAdapter = $Matches.adapter
        } elseif ($line -match $consolePattern) {
            $metadata.Console = $Matches.console
        } elseif ($line -match $presentPattern) {
            $metadata.PresentSize = "$($Matches.width)x$($Matches.height)"
        } elseif ($line -match $scalePattern) {
            $metadata.RenderScale = [double]::Parse($Matches.scale, $culture)
        } elseif ($line -match $digestPattern) {
            $metadata.BinaryBlake3 = $Matches.digest
        } else {
            $kind = 'ReportedIntentionalSkipReasons'
            if ($line -notmatch $intentionalPattern) {
                $kind = $null
                foreach ($entry in $warningPatterns.GetEnumerator()) {
                    if ($line -match $entry.Value) { $kind = $entry.Key; break }
                }
            }
            if ($kind) { $events.Add([pscustomobject]@{ Kind = $kind; At = Get-LogTime $line }) }
        }
    }

    if ($windowCount -eq 0) { throw "No hay resúmenes de FPS reconocibles en $($item.Name)." }
    $untimedExcluded = if ($From -gt 0 -or $DurationSeconds -gt 0 -or $UntilWindowClose) { $untimedWindows } else { 0 }
    foreach ($event in $events) {
        if (Test-TimeSlice $event.At $firstWindow $From $DurationSeconds $closeTime) { $quality[$event.Kind]++ }
    }

    $p95 = $null
    if ($windows.Count -gt 0) {
        $ms = @($windows.Ms | Sort-Object)
        $p95 = $ms[[int][Math]::Ceiling(0.95 * $ms.Count) - 1]
    }
    $drops = @($windows | Where-Object { $null -ne $_.PipelineDrops } | ForEach-Object { $_.PipelineDrops })
    $meet25 = @($windows | Where-Object { $_.FPS -ge 25 }).Count
    $pct25 = if ($windows.Count) { [Math]::Round(100.0 * $meet25 / $windows.Count, 1) } else { $null }

    [pscustomobject]@{
        File = $item.Name
        Metadata = [pscustomobject]$metadata
        FromSeconds = $From
        WindowCloseUtc = if ($null -ne $closeTime) { $closeTime.UtcDateTime.ToString('o', $culture) } else { $null }
        AllWindows = $windowCount
        TimeWindows = $timeCount
        ExcludedUntimedWindows = $untimedExcluded
        SelectedWindows = $windows.Count
        MinimumFPS = if ($windows.Count) { ($windows | Measure-Object FPS -Minimum).Minimum } else { $null }
        MedianFPS = Get-Median @($windows | ForEach-Object { $_.FPS })
        WindowP95Ms = $p95
        WindowsBelow20FPS = @($windows | Where-Object { $_.FPS -lt 20 }).Count
        WindowsAtLeast25FPS = $meet25
        PercentAtLeast25FPS = $pct25
        WindowsAtLeast30FPS = @($windows | Where-Object { $_.FPS -ge 30 }).Count
        MedianDrawsPerFrame = Get-Median @($windows | ForEach-Object { $_.Draws })
        MedianPipelineDropsPerFrame = Get-Median $drops
        DescriptorDropsSinceStartupAtLastTimeWindow = $lastDescriptorDrops
        QualityLogLinesInTimeSlice = [pscustomobject]$quality
        NativeCadence = Get-NativeCadence $cadenceBatches $firstWindow $From $closeTime
    }
}

$a = Read-Run $Baseline $BaselineFromSeconds
$b = Read-Run $Candidate $CandidateFromSeconds
$differences = @(
    foreach ($field in @('RuntimeAdapter', 'Console', 'PresentSize', 'RenderScale', 'Perfil', 'Build')) {
        if ($null -eq $a.Metadata.$field -or $null -eq $b.Metadata.$field) { "$field sin datos en uno o ambos registros" }
        elseif ($a.Metadata.$field -ne $b.Metadata.$field) { "$field distinto entre registros" }
    }
)
$delta = $null
if ($a.SelectedWindows -gt 0 -and $b.SelectedWindows -gt 0) {
    $delta = [pscustomobject]@{
        MedianFPS = [Math]::Round($b.MedianFPS - $a.MedianFPS, 2)
        WindowP95Ms = [Math]::Round($b.WindowP95Ms - $a.WindowP95Ms, 2)
    }
}
$method = 'Cada muestra es una ventana resumida del runtime. El p95 ordena los ms de las ventanas y toma la posición ceil(0,95 × n). Con menos de 20 ventanas, coincide con el valor máximo. No usa tiempos de cuadros individuales. Los avisos son líneas del tramo temporal y pueden estar limitados por el runtime. Los descartes de pipeline mezclan fallos y omisiones intencionales. Igualar draw calls no garantiza la misma escena ni la calidad visual.'
$cadenceMethod = 'La cadencia usa los intervalos individuales entre presentaciones consecutivas aceptadas por DXGI (S_OK). La oclusión y los rechazos interrumpen la serie. Se filtran lotes por tiempo y draws medios. Los percentiles no miden el refresco final de pantalla ni la generación de cuadros. Overflow cuenta intervalos que no se conservaron.'

if ($AsJson) {
    [pscustomobject]@{
        MinDraws = $MinDraws; MaxDraws = $MaxDraws; DurationSeconds = $DurationSeconds
        Method = $method; NativeCadenceMethod = $cadenceMethod; MetadataDifferences = $differences
        Baseline = $a; Candidate = $b; CandidateMinusBaseline = $delta
    } | ConvertTo-Json -Depth 6
    return
}

"Base: $($a.File)"
"Comparación: $($b.File)"
$drawRange = if ($MaxDraws) { "$MinDraws a $MaxDraws" } else { "$MinDraws o más" }
"Filtro: $drawRange draws por cuadro. $method"
if ($differences.Count) { "Contexto de A/B: $($differences -join ', ')." }
if ($BaselineFromSeconds -or $CandidateFromSeconds -or $DurationSeconds) {
    $duration = if ($DurationSeconds) { "$DurationSeconds s" } else { 'hasta el final' }
    "Tramos desde primera ventana: base $BaselineFromSeconds s, comparación $CandidateFromSeconds s, duración $duration."
}
$a, $b | Format-Table File, SelectedWindows, TimeWindows, MinimumFPS, MedianFPS, WindowP95Ms, MedianDrawsPerFrame -AutoSize
$a, $b | Format-Table File, WindowsBelow20FPS, WindowsAtLeast25FPS, PercentAtLeast25FPS, WindowsAtLeast30FPS -AutoSize
if ($delta) {
    "Cambio (comparación menos base): $($delta.MedianFPS) FPS en mediana y $($delta.WindowP95Ms) ms en p95 por ventana."
} else {
    'Faltan ventanas que cumplan ambos filtros para calcular el cambio.'
}
foreach ($run in @($a, $b)) {
    if ($UntilWindowClose) { "WM_CLOSE de $($run.File) en UTC: $($run.WindowCloseUtc)" }
    if ($run.ExcludedUntimedWindows) {
        "$($run.File): se excluyeron $($run.ExcludedUntimedWindows) ventanas sin timestamp al recortar por tiempo."
    }
    "Identidad y avisos de $($run.File)"
    $run.Metadata | Format-List
    $run | Format-List MedianPipelineDropsPerFrame, DescriptorDropsSinceStartupAtLastTimeWindow
    $run.QualityLogLinesInTimeSlice | Format-List
}
if ($null -ne $a.NativeCadence -or $null -ne $b.NativeCadence) {
    $cadenceMethod
    foreach ($run in @($a, $b)) {
        "Cadencia nativa de $($run.File)"
        if ($null -eq $run.NativeCadence) { 'El registro no trae cadencia individual.' }
        else { $run.NativeCadence | Format-List }
    }
}
