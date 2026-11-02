// Windows' receivers for drags out of the window (drag-promise): File
// Explorer on a folder of the test's own, an app that takes real files only
// (a WinForms form taking FileDrop, as many apps do), Explorer's copy
// dialogs, and the app's own windows. All through PowerShell, in the
// signed-in session the tests run in (runners.mjs starts them there). Each
// opens its window where it's told, beside the app's, and closes only its own.

import { execFileSync, spawn } from "node:child_process";
import { existsSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";
import { setTimeout as sleep } from "node:timers/promises";

/** user32, for the windows these find and place. */
const USER32 = `Add-Type -AssemblyName UIAutomationClient, UIAutomationTypes
Add-Type @'
using System; using System.Text; using System.Collections.Generic; using System.Runtime.InteropServices;
public static class Win {
  public delegate bool EnumProc(IntPtr h, IntPtr l);
  [DllImport("user32.dll")] public static extern bool EnumWindows(EnumProc f, IntPtr l);
  [DllImport("user32.dll")] public static extern bool IsWindowVisible(IntPtr h);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet=CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [DllImport("user32.dll")] public static extern bool SetProcessDPIAware();
  [DllImport("user32.dll")] public static extern bool SetWindowPos(IntPtr h, IntPtr a, int x, int y, int cx, int cy, uint f);
  [DllImport("user32.dll")] public static extern bool PostMessage(IntPtr h, uint m, IntPtr w, IntPtr l);
  [DllImport("user32.dll")] public static extern int GetSystemMetrics(int i);
  public static List<IntPtr> Tops() { var l = new List<IntPtr>(); EnumWindows((h, p) => { if (IsWindowVisible(h)) l.Add(h); return true; }, IntPtr.Zero); return l; }
  public static string Text(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
  public static string Cls(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
  public static uint Pid(IntPtr h) { uint p; GetWindowThreadProcessId(h, out p); return p; }
}
'@
[Win]::SetProcessDPIAware() | Out-Null
function Dialogs { [Win]::Tops() | Where-Object { $p = (Get-Process -Id ([Win]::Pid($_)) -ErrorAction SilentlyContinue).ProcessName; $p -eq 'explorer' -and [Win]::Cls($_) -notmatch '^(CabinetWClass|Shell_.*|Progman|WorkerW|.*Tray.*)$' -and [Win]::Text($_) } }
`;

/** Runs `script` (written to a file: its here-strings and blocks span lines) and gives what it printed. */
function powershell(script) {
  const file = path.join(tmpdir(), `vodscribe-e2e-${process.pid}-explorer.ps1`);
  writeFileSync(file, script);
  try {
    return execFileSync(
      "powershell.exe",
      ["-NoProfile", "-NonInteractive", "-ExecutionPolicy", "Bypass", "-File", file],
      {
        encoding: "utf8",
      },
    ).trim();
  } finally {
    rmSync(file, { force: true });
  }
}

/** The primary screen's size in pixels. */
export function screenSize() {
  const [w, h] = powershell(`${USER32} "$([Win]::GetSystemMetrics(0)) $([Win]::GetSystemMetrics(1))"`)
    .split(" ")
    .map(Number);
  return { w, h };
}

/** Moves process `pid`'s main window (the visible one with a title) to `rect` ({x, y, w, h}, its outer box). */
export function placeWindowOf(pid, rect) {
  const done = powershell(
    `${USER32}
     $h = [Win]::Tops() | Where-Object { [Win]::Pid($_) -eq ${pid} -and [Win]::Text($_) } | Select-Object -First 1
     if (-not $h) { 'none'; exit }
     [Win]::SetWindowPos($h, [IntPtr]::Zero, ${rect.x}, ${rect.y}, ${rect.w}, ${rect.h}, 0x44) | Out-Null; 'placed'`,
  );
  if (done !== "placed") throw new Error(`process ${pid} has no window to place`);
}

/**
 * Explorer's dialogs up now (copying, an error): each one's title and the
 * texts in it, as UI Automation reads them.
 */
export function explorerDialogs() {
  const out = powershell(
    `${USER32}
     $all = @(foreach ($h in Dialogs) {
       $texts = @()
       try {
         $e = [System.Windows.Automation.AutomationElement]::FromHandle($h)
         foreach ($x in $e.FindAll([System.Windows.Automation.TreeScope]::Descendants, [System.Windows.Automation.Condition]::TrueCondition)) {
           if ($x.Current.Name) { $texts += $x.Current.Name } }
       } catch {}
       [pscustomobject]@{ title = [Win]::Text($h); class = [Win]::Cls($h); texts = $texts }
     })
     ConvertTo-Json -Depth 3 -InputObject $all`,
  );
  return out ? [JSON.parse(out)].flat() : [];
}

/** Closes Explorer's dialogs left up (a copy's, an error's). */
export function closeExplorerDialogs() {
  powershell(
    `${USER32} foreach ($h in Dialogs) { [Win]::PostMessage($h, 0x10, [IntPtr]::Zero, [IntPtr]::Zero) | Out-Null }`,
  );
}

/**
 * Opens a File Explorer window on folder `dir`, at `rect` (screen pixels):
 * where to drop on it (`drop`, in its list of files), and `close()`, which
 * closes that window only, and any dialog Explorer left up.
 */
export async function openExplorer(dir, rect) {
  const placed = powershell(
    `${USER32}
     Start-Process explorer.exe '${dir}'
     $win = $null
     for ($i = 0; $i -lt 75 -and -not $win; $i++) {
       Start-Sleep -Milliseconds 200
       foreach ($w in (New-Object -ComObject Shell.Application).Windows()) {
         try { if ($w.Document.Folder.Self.Path -eq '${dir}') { $win = $w } } catch {}
       }
     }
     if (-not $win) { 'none'; exit }
     [Win]::SetWindowPos([IntPtr]$win.HWND, [IntPtr]::Zero, ${rect.x}, ${rect.y}, ${rect.w}, ${rect.h}, 0x40) | Out-Null
     "$($win.HWND)"`,
  );
  if (placed === "none") throw new Error(`no Explorer window opened on ${dir}`);
  await sleep(800);
  const close = () => {
    powershell(
      `foreach ($w in (New-Object -ComObject Shell.Application).Windows()) {
         try { $p = $w.Document.Folder.Self.Path } catch { $p = '' }
         if ($p -eq '${dir}') { $w.Quit() }
       }`,
    );
    closeExplorerDialogs();
  };
  return { drop: { x: rect.x + Math.round(rect.w / 2), y: rect.y + Math.round(rect.h * 0.6) }, close };
}

/** The WinForms receiver: takes a drop only as real files (FileDrop), logs what it got; or, with $refuse, refuses every drag. */
const RECEIVER = `param([string]$log, [int]$x, [int]$y, [int]$w, [int]$h, [int]$refuse)
Add-Type -AssemblyName System.Windows.Forms
function L($m) { Add-Content $log $m }
$f = New-Object System.Windows.Forms.Form
$f.Text = 'Real files only'; $f.AllowDrop = $true; $f.TopMost = $true
$f.StartPosition = 'Manual'; $f.Location = New-Object System.Drawing.Point($x, $y); $f.Size = New-Object System.Drawing.Size($w, $h)
$f.Add_DragEnter({ param($s, $e)
  if ($refuse) { $e.Effect = 'None'; L 'refused' }
  elseif ($e.Data.GetDataPresent([System.Windows.Forms.DataFormats]::FileDrop)) { $e.Effect = 'Copy' } else { $e.Effect = 'None' } })
$f.Add_DragDrop({ param($s, $e)
  $files = @($e.Data.GetData([System.Windows.Forms.DataFormats]::FileDrop))
  if (-not $files -or -not $files[0]) { L 'gave nothing' }
  foreach ($p in $files) { if ($p) { if (Test-Path $p) { L "took $p" } else { L "missing $p" } } } })
$f.Add_Shown({ L 'ready' })
[System.Windows.Forms.Application]::Run($f)
`;

/**
 * Opens an app that takes real files only (CF_HDROP: Windows PowerShell's
 * WinForms form, named "Windows PowerShell" by its program) at `rect`:
 * where to drop on it, what it has said (`said()`: "gave nothing", "took
 * <path>", "missing <path>"), and `close()`. With `refuses`, it refuses
 * every drag instead (and says "refused").
 */
export async function openFilesOnly(work, rect, { refuses = false } = {}) {
  const script = path.join(work, "files-only.ps1");
  const log = path.join(work, "files-only.log");
  writeFileSync(script, RECEIVER);
  writeFileSync(log, "");
  const child = spawn(
    "powershell.exe",
    [
      ...["-NoProfile", "-ExecutionPolicy", "Bypass", "-File", script, log],
      ...[rect.x, rect.y, rect.w, rect.h, refuses ? 1 : 0].map(String),
    ],
    { stdio: "ignore" },
  );
  const said = () => (existsSync(log) ? readFileSync(log, "utf8").split(/\r?\n/).filter(Boolean) : []);
  for (let waited = 0; !said().includes("ready"); waited += 100) {
    if (waited > 20_000) throw new Error("the app that takes real files only didn't open");
    await sleep(100);
  }
  await sleep(500);
  return {
    drop: { x: rect.x + Math.round(rect.w / 2), y: rect.y + Math.round(rect.h / 2) },
    said,
    close: () => child.kill(),
  };
}
