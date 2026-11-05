param([int]$ParentPid, [string]$PickPath = '')
Add-Type @"
using System;
using System.Text;
using System.Collections.Generic;
using System.Runtime.InteropServices;
public class DialogCloser {
  public delegate bool EnumProc(IntPtr hWnd, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern bool EnumChildWindows(IntPtr parent, EnumProc cb, IntPtr lParam);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr hWnd, out uint pid);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr hWnd);
  [DllImport("user32.dll", CharSet = CharSet.Auto)] public static extern int GetClassName(IntPtr hWnd, StringBuilder sb, int max);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr hWnd, uint msg, IntPtr wParam, IntPtr lParam);
  [DllImport("user32.dll", CharSet = CharSet.Auto)] public static extern IntPtr SendMessage(IntPtr hWnd, uint msg, IntPtr wParam, string lParam);
  public static HashSet<uint> Pids = new HashSet<uint>();
  public static List<IntPtr> Dialogs = new List<IntPtr>();
  public static List<IntPtr> Edits = new List<IntPtr>();
  public static void Collect() {
    Dialogs.Clear();
    EnumWindows((h, l) => {
      uint p; GetWindowThreadProcessId(h, out p);
      if (Pids.Contains(p) && IsWindowVisible(h)) {
        var sb = new StringBuilder(256); GetClassName(h, sb, 256);
        if (sb.ToString() == "#32770") Dialogs.Add(h);
      }
      return true;
    }, IntPtr.Zero);
  }
  public static void FindEdits(IntPtr dialog) {
    Edits.Clear();
    EnumChildWindows(dialog, (h, l) => {
      var sb = new StringBuilder(256); GetClassName(h, sb, 256);
      if (sb.ToString() == "Edit" && IsWindowVisible(h)) Edits.Add(h);
      return true;
    }, IntPtr.Zero);
  }
  [DllImport("user32.dll")] public static extern bool SetForegroundWindow(IntPtr hWnd);
  public static void SetText(IntPtr edit, string text) { SendMessage(edit, 0x000C, IntPtr.Zero, text); }
}
"@
$children = @(Get-CimInstance Win32_Process -Filter "ParentProcessId=$ParentPid" | Select-Object -ExpandProperty ProcessId)
if ($children.Count -eq 0) { 'no child process, so no dialog is open'; exit 0 }
foreach ($c in $children) { [void][DialogCloser]::Pids.Add([uint32]$c) }
# Native dialogs (rfd) run inside the app process itself, so its own windows count too.
[void][DialogCloser]::Pids.Add([uint32]$ParentPid)
[DialogCloser]::Collect()
if ([DialogCloser]::Dialogs.Count -eq 0) { "child processes $($children -join ',') have no visible dialog window"; exit 0 }
$dialog = [DialogCloser]::Dialogs[0]
if ($PickPath -ne '') {
  Add-Type -AssemblyName System.Windows.Forms
  [void][DialogCloser]::SetForegroundWindow($dialog)
  Start-Sleep -Milliseconds 700
  $escaped = [regex]::Replace($PickPath, '[+^%~(){}[]]', { param($m) '{' + $m.Value + '}' })
  [System.Windows.Forms.SendKeys]::SendWait($escaped)
  [System.Windows.Forms.SendKeys]::SendWait("{ENTER}")
  "typed $PickPath into dialog $dialog"
} else {
  [void][DialogCloser]::PostMessage($dialog, 0x0111, [IntPtr]2, [IntPtr]::Zero)
  "canceled dialog $dialog"
}
