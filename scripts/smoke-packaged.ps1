# Launches a packaged TSGPUI app, waits for its main window, captures the frame, and reports
# launch metrics as JSON on stdout (or -ReportPath).
#
# Smoke-test mode (default): one run, counts distinct sampled colors in the captured frame so
# the caller can assert a rendered (non-blank) window, and records window-visible latency and
# idle RSS. Screenshot is saved when -ScreenshotPath is given.
# Benchmark mode: -Runs N launches the app N times and reports min/median/max per metric.
#
# Used by scripts/smoke-packaged.mjs (smoke test) and scripts/benchmark.mjs (Track 8).
param(
  [Parameter(Mandatory)][string]$ExePath,
  [string]$ScreenshotPath = '',
  [int]$WaitSeconds = 6,
  [int]$Runs = 1,
  [string]$ReportPath = ''
)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class SmokeWindowNative {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
}
'@

function Invoke-Run {
  param([int]$Run, [bool]$SaveScreenshot)
  $process = Start-Process -FilePath $ExePath -PassThru
  $sw = [System.Diagnostics.Stopwatch]::StartNew()
  $deadline = [TimeSpan]::FromSeconds(30)
  do { Start-Sleep -Milliseconds 50; $process.Refresh() } while (-not $process.HasExited -and $process.MainWindowHandle -eq [IntPtr]::Zero -and $sw.Elapsed -lt $deadline)
  if ($process.HasExited -or $process.MainWindowHandle -eq [IntPtr]::Zero) {
    throw "Run ${Run}: no main window within 30 seconds."
  }
  $windowMs = [int]$sw.Elapsed.TotalMilliseconds
  Start-Sleep -Seconds $WaitSeconds
  $process.Refresh()
  $handle = $process.MainWindowHandle
  if ($handle -eq [IntPtr]::Zero) { throw "Run ${Run}: main window disappeared before capture." }
  $rect = New-Object SmokeWindowNative+RECT
  if (-not [SmokeWindowNative]::GetWindowRect($handle, [ref]$rect)) { throw "Run ${Run}: GetWindowRect failed." }
  $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
  if ($width -le 0 -or $height -le 0) { throw "Run ${Run}: invalid window dimensions ${width}x${height}." }
  $bitmap = New-Object System.Drawing.Bitmap $width, $height
  try {
    $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
    try {
      $dc = $graphics.GetHdc()
      try { if (-not [SmokeWindowNative]::PrintWindow($handle, $dc, 2)) { throw "Run ${Run}: PrintWindow failed." } }
      finally { $graphics.ReleaseHdc($dc) }
    } finally { $graphics.Dispose() }
    if ($SaveScreenshot) { $bitmap.Save($ScreenshotPath, [System.Drawing.Imaging.ImageFormat]::Png) }
    # Sample a coarse grid; a rendered frame has several distinct colors, a blank one has one.
    $colors = New-Object 'System.Collections.Generic.HashSet[string]'
    $step = 12
    for ($y = 0; $y -lt $height; $y += $step) {
      for ($x = 0; $x -lt $width; $x += $step) {
        $pixel = $bitmap.GetPixel($x, $y)
        [void]$colors.Add("$($pixel.R),$($pixel.G),$($pixel.B)")
      }
    }
  } finally { $bitmap.Dispose() }
  $rssMB = [math]::Round($process.WorkingSet64 / 1MB, 1)
  $result = [pscustomobject]@{
    windowMs = $windowMs
    title = $process.MainWindowTitle
    width = $width
    height = $height
    responding = [bool]$process.Responding
    rssMB = $rssMB
    distinctColors = $colors.Count
  }
  if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue }
  Start-Sleep -Milliseconds 500
  return $result
}

$all = @()
$failure = $null
try { $all = 1..$Runs | ForEach-Object { Invoke-Run -Run $_ -SaveScreenshot:($_ -eq 1 -and $ScreenshotPath -ne '') } }
catch { $failure = "$_" }
finally { Get-Process | Where-Object { $_.Path -eq $ExePath } | Stop-Process -Force -ErrorAction SilentlyContinue }

$all = @($all)
$report = [ordered]@{ exe = $ExePath; runs = $Runs; results = @() }
if ($failure) { $report.error = $failure }
if ($all.Count -gt 0) {
  $report.results = @($all)
  $windows = @($all | ForEach-Object { $_.windowMs } | Sort-Object)
  $rss = @($all | ForEach-Object { $_.rssMB } | Sort-Object)
  $median = { param($sorted) if ($sorted.Count -eq 0) { return $null }; if ($sorted.Count % 2 -eq 1) { $sorted[[int][math]::Floor($sorted.Count / 2)] } else { [math]::Round(($sorted[$sorted.Count / 2 - 1] + $sorted[$sorted.Count / 2]) / 2, 1) } }
  $report.summary = [ordered]@{
    windowMsMin = $windows[0]
    windowMsMedian = & $median $windows
    windowMsMax = $windows[-1]
    rssMBMin = $rss[0]
    rssMBMedian = & $median $rss
    rssMBMax = $rss[-1]
  }
}
$json = $report | ConvertTo-Json -Depth 4
if ($ReportPath -ne '') { Set-Content -Path $ReportPath -Value $json -Encoding UTF8 }
Write-Output $json
if ($failure) { exit 1 }
