param([Parameter(Mandatory=$true)][string]$Exe)
$ErrorActionPreference = 'Stop'
$Exe = (Resolve-Path -LiteralPath $Exe).Path
$work = Join-Path ([IO.Path]::GetTempPath()) ('tsgpui-smoke-' + [guid]::NewGuid())
New-Item -ItemType Directory -Path (Join-Path $work 'notes') -Force | Out-Null
$copy = Join-Path $work 'notes-app.exe'
Copy-Item -LiteralPath $Exe -Destination $copy
Add-Type -AssemblyName System.Windows.Forms
Add-Type @"
using System;
using System.Runtime.InteropServices;
public class TsgpuiSmoke {
 [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr h);
 [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out RECT r);
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x,int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint f,uint x,uint y,uint d,UIntPtr e);
 public struct RECT { public int Left, Top, Right, Bottom; }
}
"@
function Click-At([int]$x,[int]$y) {
  [TsgpuiSmoke]::SetCursorPos($rect.Left + $x, $rect.Top + $y) | Out-Null
  [TsgpuiSmoke]::mouse_event(2,0,0,0,[UIntPtr]::Zero)
  [TsgpuiSmoke]::mouse_event(4,0,0,0,[UIntPtr]::Zero)
  Start-Sleep -Milliseconds 200
}
function Wait-ForFile([string]$file) {
  for ($i=0; $i -lt 50; $i++) { if (Test-Path -LiteralPath $file) { return }; Start-Sleep -Milliseconds 100 }
  throw "Timed out waiting for $file"
}
$process = $null
try {
  $process = Start-Process -FilePath $copy -PassThru
  for ($i=0; $i -lt 100; $i++) { $process.Refresh(); if ($process.HasExited) { throw 'App exited before its window appeared' }; if ($process.MainWindowHandle -ne 0) { break }; Start-Sleep -Milliseconds 100 }
  if ($process.MainWindowHandle -eq 0) { throw 'App window did not appear' }
  [TsgpuiSmoke]::SetForegroundWindow($process.MainWindowHandle) | Out-Null
  Start-Sleep -Seconds 2
  $rect = New-Object TsgpuiSmoke+RECT
  [TsgpuiSmoke]::GetWindowRect($process.MainWindowHandle, [ref]$rect) | Out-Null
  # Tab order: name, Save, Open, Delete, Refresh, editor.
  [System.Windows.Forms.SendKeys]::SendWait('{TAB}smoke{TAB}{TAB}{TAB}{TAB}{TAB}hello smoke')
  Click-At 360 116
  $first = Join-Path $work 'notes/smoke.txt'
  Wait-ForFile $first
  if ((Get-Content -Raw -LiteralPath $first) -ne 'hello smoke') { throw 'Saved note has unexpected content' }
  # Overwrite the draft, reopen the first note, then save it under a second name.
  [System.Windows.Forms.SendKeys]::SendWait('^aWRONG')
  Click-At 420 116
  Start-Sleep -Seconds 1
  Click-At 90 116
  [System.Windows.Forms.SendKeys]::SendWait('^asmoke-copy')
  Click-At 360 116
  $second = Join-Path $work 'notes/smoke-copy.txt'
  Wait-ForFile $second
  if ((Get-Content -Raw -LiteralPath $second) -ne 'hello smoke') { throw 'Reopened note did not restore the saved content' }
  Write-Host 'Notes packaged-app smoke test passed.'
} finally {
  if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force; $process.WaitForExit() }
  Remove-Item -LiteralPath $work -Recurse -Force
}
