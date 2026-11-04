param([Parameter(Mandatory)][string]$ExePath, [Parameter(Mandatory)][string]$Buttons, [Parameter(Mandatory)][string]$OutJson)
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System;
using System.Runtime.InteropServices;
public static class DriveWindowNative {
 [DllImport("user32.dll")] public static extern bool SetCursorPos(int x, int y);
 [DllImport("user32.dll")] public static extern void mouse_event(uint flags, uint dx, uint dy, uint data, UIntPtr extra);
}
'@
$process = $null
try {
 $process = Start-Process -FilePath $ExePath -PassThru
 $deadline = (Get-Date).AddSeconds(30)
 do { Start-Sleep -Milliseconds 200; $process.Refresh() } while (-not $process.HasExited -and $process.MainWindowHandle -eq [IntPtr]::Zero -and (Get-Date) -lt $deadline)
 if ($process.HasExited -or $process.MainWindowHandle -eq [IntPtr]::Zero) { throw 'Application did not create a main window within 30 seconds.' }
 Start-Sleep -Seconds 2
 $window = [System.Windows.Automation.AutomationElement]::FromHandle($process.MainWindowHandle)
 if (-not $window -or $window.Current.ProcessId -ne $process.Id) { throw 'Could not find the application window by process.' }
 $scope = [System.Windows.Automation.TreeScope]::Descendants
 $all = $window.FindAll($scope, [System.Windows.Automation.Condition]::TrueCondition)
 Write-Output 'Automation elements:'
 foreach ($element in $all) { Write-Output ("{0}: {1}" -f $element.Current.ControlType.ProgrammaticName, $element.Current.Name) }
 $records = @()
 foreach ($name in $Buttons.Split(',')) {
  $name = $name.Trim()
  $condition = New-Object System.Windows.Automation.AndCondition (New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty, $name)), (New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::ControlTypeProperty, [System.Windows.Automation.ControlType]::Button))
  $button = $window.FindFirst($scope, $condition); $invoked = $false
  if ($button) {
   try {
    $pattern = $null
    if ($button.TryGetCurrentPattern([System.Windows.Automation.InvokePattern]::Pattern, [ref]$pattern)) { $pattern.Invoke() } else {
     $rect = $button.Current.BoundingRectangle
     if ($rect.IsEmpty -or $rect.Width -le 0 -or $rect.Height -le 0) { throw 'Button has no clickable bounds.' }
     if (-not [DriveWindowNative]::SetCursorPos([int]($rect.X + $rect.Width / 2), [int]($rect.Y + $rect.Height / 2))) { throw 'Could not move mouse.' }
     [DriveWindowNative]::mouse_event(2, 0, 0, 0, [UIntPtr]::Zero); [DriveWindowNative]::mouse_event(4, 0, 0, 0, [UIntPtr]::Zero)
    }; $invoked = $true
   } catch { Write-Warning ("Could not invoke {0}: {1}" -f $name, $_) }
  }
  Start-Sleep -Seconds 1
  $status = $window.FindFirst($scope, (New-Object System.Windows.Automation.PropertyCondition ([System.Windows.Automation.AutomationElement]::NameProperty, 'status')))
  $value = ''
  if ($status) { $p = $null; if ($status.TryGetCurrentPattern([System.Windows.Automation.ValuePattern]::Pattern, [ref]$p)) { $value = $p.Current.Value } elseif ($status.TryGetCurrentPattern([System.Windows.Automation.TextPattern]::Pattern, [ref]$p)) { $value = $p.DocumentRange.GetText(-1) } else { $value = $status.Current.Name } }
  $records += [pscustomobject]@{ button = $name; found = [bool]$button; invoked = $invoked; status = $value }
 }
 $json = ConvertTo-Json -InputObject @($records) -Depth 4
 Set-Content -LiteralPath $OutJson -Value $json -Encoding UTF8
 Write-Output $json
} catch { Write-Error $_; exit 1 } finally { if ($process -and -not $process.HasExited) { Stop-Process -Id $process.Id -Force -ErrorAction SilentlyContinue } }
