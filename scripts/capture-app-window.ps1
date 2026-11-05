param([Parameter(Mandatory)][string]$ExePath, [Parameter(Mandatory)][string]$OutPath, [int]$WaitSeconds = 6)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class CaptureWindowNative {
 [StructLayout(LayoutKind.Sequential)] public struct RECT { public int Left, Top, Right, Bottom; }
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool PrintWindow(IntPtr h, IntPtr dc, uint flags);
}
'@
$process = $null
try {
 $process = Start-Process -FilePath $ExePath -PassThru
 $deadline = (Get-Date).AddSeconds(30)
 do { Start-Sleep -Milliseconds 200; $process.Refresh() } while (-not $process.HasExited -and $process.MainWindowHandle -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline)
 if ($process.HasExited -or $process.MainWindowHandle -eq [IntPtr]::Zero) { throw 'Application did not create a main window within 30 seconds.' }
 Start-Sleep -Seconds $WaitSeconds
 $process.Refresh(); $handle = $process.MainWindowHandle
 if ($handle -eq [IntPtr]::Zero) { throw 'Application main window is no longer available.' }
 $rect = New-Object CaptureWindowNative+RECT
 if (-not [CaptureWindowNative]::GetWindowRect($handle, [ref]$rect)) { throw 'GetWindowRect failed.' }
 $width = $rect.Right - $rect.Left; $height = $rect.Bottom - $rect.Top
 if ($width -le 0 -or $height -le 0) { throw 'Application window has invalid dimensions.' }
 $bitmap = New-Object System.Drawing.Bitmap $width, $height
 try {
  $graphics = [System.Drawing.Graphics]::FromImage($bitmap)
  try { $dc = $graphics.GetHdc(); try { if (-not [CaptureWindowNative]::PrintWindow($handle, $dc, 2)) { throw 'PrintWindow failed.' } } finally { $graphics.ReleaseHdc($dc) } } finally { $graphics.Dispose() }
  $bitmap.Save($OutPath, [System.Drawing.Imaging.ImageFormat]::Png)
 } finally { $bitmap.Dispose() }
 Write-Output ("Window: {0} ({1}x{2})" -f $process.MainWindowTitle, $width, $height)
 Write-Output "Saved: $OutPath"
} catch { Write-Error $_; exit 1 } finally { if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue } }
