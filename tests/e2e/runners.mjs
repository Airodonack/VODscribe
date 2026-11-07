// Runs the end-to-end tests on other computers ("runners"), reached over SSH,
// and brings the results back to tests/e2e/out/<runner>/.
//
//   node tests/e2e/runners.mjs <runner> <app> [--test-app <exe>] [run.mjs options, e.g. --only a,b]
//
// Runners are listed in tests/e2e/runners.json (not in the repository; see
// runners.example.json): a name, the SSH host, its OS, a folder there to
// work in, and optionally `path`: folders put first on PATH for the tests
// (Windows), for tools installed only for the user. <app> is what to test, on this computer:
//
// - Windows: an NSIS installer (*-setup.exe), and with --test-app the app
//   built with the `e2e` feature (vodscribe.exe): the installer is run for
//   the user, silently, the test build put over the vodscribe.exe it
//   installed, and that driven through the app's own test hook (a release
//   doesn't let WebDriver in). The runner needs Node on PATH.
//   The tests run as a scheduled task, so the app has the user's desktop
//   (over SSH it would have none); someone must be signed in.
// - macOS: a .app built with the `e2e` feature (its webview has no
//   WebDriver), driven through the app's own test hook; the runner needs
//   Node (~/.local/node/bin, or on PATH). Someone must be signed in, with no
//   screensaver: a hidden window gets no animation frames, and the player's
//   time stands still.
//
// The runner's app data is emptied before each scenario: runners are for
// testing only. E2E_NETWORK=1 here is passed on (scenarios that reach the sites),
// and on the Mac, E2E_SCREEN (lib/screen.mjs).
// run.mjs's options pass through as they are: --shard splits the suite
// between runners; --parallel is Linux's only, so 1 here (the app's data
// folder on Windows and macOS can't be moved per worker).

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

import { macNotClear, macScreen, windowsNotClear } from "./lib/runnerScreen.mjs";

const here = path.dirname(fileURLToPath(import.meta.url));
const repo = path.join(here, "..", "..");
const [name, app, ...rest] = process.argv.slice(2);
if (!name || !app) {
  console.error("usage: node tests/e2e/runners.mjs <runner> <app> [--test-app <exe>] [run.mjs options]");
  process.exit(2);
}
const testAt = rest.indexOf("--test-app");
const testApp = testAt >= 0 ? rest.splice(testAt, 2)[1] : null;
const listed = path.join(here, "runners.json");
if (!existsSync(listed)) throw new Error(`no ${listed}: copy runners.example.json and fill it in`);
const runner = JSON.parse(readFileSync(listed, "utf8")).find((r) => r.name === name);
if (!runner) throw new Error(`no runner ${name} in ${listed}`);
const out = path.join(here, "out", name);
rmSync(out, { recursive: true, force: true });
mkdirSync(out, { recursive: true });

// One connection for every command, kept open a while: OpenSSH 9.8+ servers
// refuse an address for minutes after many connections from it
// (PerSourcePenalties), which polling with a new one each time set off.
const shared = [
  "-o",
  "BatchMode=yes",
  "-o",
  "ControlMaster=auto",
  "-o",
  `ControlPath=${path.join(tmpdir(), `vodscribe-e2e-${name}-%C`)}`,
  "-o",
  "ControlPersist=120",
];
const ssh = (command, opts = {}) =>
  execFileSync("ssh", [...shared, runner.host, command], { encoding: "utf8", ...opts });
const scp = (...args) => execFileSync("scp", ["-q", ...shared, ...args], { stdio: "inherit" });
const log = (what) => console.log(`[${name}] ${what}`);
// The lines of run.mjs's output shown here as it goes: progress, results and what it found of the computer.
const SHOWN = /^\[\d+ s\]|^e2e:|^preflight:|^  (ok|FAIL|flaky|wall)|FAILED/;

/** Copies the harness (tests/e2e and what it imports, as committed plus what's new) to the runner's folder. */
function sendHarness(mkdir) {
  ssh(mkdir);
  // And what of the app the harness imports (media.ts: what an unplayable video says).
  const files = execFileSync(
    "git",
    ["ls-files", "-co", "--exclude-standard", "tests/e2e", "apps/desktop/src/lib/media.ts"],
    {
      cwd: repo,
      encoding: "utf8",
    },
  )
    .split("\n")
    .filter((f) => f && !f.startsWith("tests/e2e/out/"));
  const tar = spawnSync("tar", ["-cf", "-", ...files], { cwd: repo, maxBuffer: 1 << 30 });
  if (tar.status !== 0) throw new Error(`tar: ${tar.stderr}`);
  const unpack = runner.os === "windows" ? `tar -xf - -C "${runner.dir}"` : `tar -xf - -C '${runner.dir}'`;
  execFileSync("ssh", [...shared, runner.host, unpack], { input: tar.stdout });
}

/** The Windows runner's screen, run in the scheduled task (over SSH there's
 *  no desktop to see) before the suite, and again after a failed one with
 *  "after": a screenshot to out\desktop-<when>.png, the windows up to
 *  out\desktop-<when>.txt, and exit 3 if a window is in front that isn't the
 *  desktop, the taskbar or the task's own console, with what it is as JSON
 *  (lib/runnerScreen.mjs windowsNotClear says it). */
const WINDOWS_DESKTOP = String.raw`param([string]$When = 'before')
$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Windows.Forms, System.Drawing
Add-Type @'
using System;
using System.Text;
using System.Runtime.InteropServices;
public static class Desk {
  [DllImport("user32.dll")] public static extern IntPtr GetForegroundWindow();
  [DllImport("kernel32.dll")] public static extern IntPtr GetConsoleWindow();
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetWindowText(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll", CharSet = CharSet.Unicode)] public static extern int GetClassName(IntPtr h, StringBuilder s, int n);
  [DllImport("user32.dll")] public static extern uint GetWindowThreadProcessId(IntPtr h, out uint pid);
  [StructLayout(LayoutKind.Sequential)] public struct Box { public int Left, Top, Right, Bottom; }
  [DllImport("user32.dll")] public static extern bool GetWindowRect(IntPtr h, out Box r);
  public static string Text(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
  public static string Class(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
}
'@
$screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
[System.Drawing.Graphics]::FromImage($bitmap).CopyFromScreen($screen.Left, $screen.Top, 0, 0, $bitmap.Size)
$bitmap.Save("$PSScriptRoot\out\desktop-$When.png", [System.Drawing.Imaging.ImageFormat]::Png)
$front = [Desk]::GetForegroundWindow()
$id = 0; [void][Desk]::GetWindowThreadProcessId($front, [ref]$id)
$process = Get-Process -Id $id -ErrorAction SilentlyContinue
$owner = $process.ProcessName
$title = [Desk]::Text($front); $class = [Desk]::Class($front)
# Windows' own: its program in the Windows folder (not known when its path can't be read).
$system = [bool]($process.Path -and $process.Path.StartsWith($env:windir, [StringComparison]::OrdinalIgnoreCase))
$box = New-Object Desk+Box; [void][Desk]::GetWindowRect($front, [ref]$box)
$bounds = [System.Windows.Forms.Screen]::FromHandle($front).Bounds
$full = $box.Left -le $bounds.Left -and $box.Top -le $bounds.Top -and $box.Right -ge $bounds.Right -and $box.Bottom -ge $bounds.Bottom
$up = Get-Process | Where-Object { $_.MainWindowTitle } | ForEach-Object { "$($_.ProcessName)  $($_.MainWindowTitle)" }
@("in front: $owner  $class  $title  (Windows' own: $system, full screen: $full)") + $up | Set-Content "$PSScriptRoot\out\desktop-$When.txt"
# Nothing in front, the desktop, the taskbar, or the console this task runs in.
$own = $front -eq [Desk]::GetConsoleWindow() -or $title -like '*run.cmd*'
if ($front -eq [IntPtr]::Zero -or $own -or $class -in 'Progman', 'WorkerW', 'Shell_TrayWnd') { exit 0 }
@{ owner = $owner; title = $title; class = $class; system = $system; fullScreen = $full } | ConvertTo-Json -Compress
exit 3
`;

async function windows() {
  const dir = runner.dir.replaceAll("/", "\\");
  // PowerShell reads the script from stdin: no quoting through cmd.exe.
  const ps = (script) => ssh("powershell -NoProfile -NonInteractive -Command -", { input: `${script}\n` }).trim();
  // The scheduled task runs in a signed-in session, and only then.
  if (!ps(`(quser 2>$null) -match 'Active'; exit 0`))
    throw new Error(`no one is signed in on ${name}: the tests need someone's desktop (sign in, then run this again)`);
  // A run still going there (one this left when stopped, or one started by
  // hand) would have its app killed by the install below and its out folder
  // taken for this one's, and this would wait on its exit.txt: refused, not
  // overlapped. Its task runs as long as the tests (run.vbs waits), and its
  // run.cmd holds out\run.log open.
  const going = ps(
    [
      "$why = @()",
      `if ((Get-ScheduledTask -TaskName vodscribe-e2e -ErrorAction SilentlyContinue).State -eq 'Running') { $why += 'its task vodscribe-e2e is running' }`,
      `$log = '${dir}\\out\\run.log'`,
      `if (Test-Path $log) { try { [IO.File]::Open($log, 'Open', 'ReadWrite', 'None').Close() } catch { $why += 'its out\\run.log is in use' } }`,
      "$why -join ', and '",
    ].join("\n"),
  );
  if (going)
    throw new Error(
      `a test run is still going on ${name} (${going}): let it end, or end it there (its node, then ` +
        `schtasks /end /tn vodscribe-e2e), then run this again`,
    );
  // What the tests need there, found the way the task will look for it:
  // Node alone, since the app is driven through its own test hook (bridge),
  // not through tauri-driver.
  const pathed = (runner.path ?? []).map((p) => `$env:PATH = '${p.replaceAll("/", "\\")};' + $env:PATH;`).join(" ");
  if (!ps(`${pathed} (Get-Command node -ErrorAction SilentlyContinue).Source; exit 0`))
    throw new Error(`${name} has no node on PATH (or in its "path" in runners.json)`);
  sendHarness(`if not exist "${dir}" mkdir "${dir}"`);
  log("harness sent");

  // Installed as a person would, silently: the same place each time.
  scp(app, `${runner.host}:${runner.dir}/setup.exe`);
  // A copy left running would keep its files in use (and there's often none: fine).
  ps(`Get-Process vodscribe -ErrorAction SilentlyContinue | Stop-Process -Force; exit 0`);
  ssh(`"${dir}\\setup.exe" /S`);
  const exe = ps(
    `(Get-ChildItem "$env:LOCALAPPDATA\\VODscribe" -Filter vodscribe.exe -ErrorAction SilentlyContinue | Select-Object -First 1).FullName`,
  );
  if (!exe) throw new Error("the installer put no vodscribe.exe in %LOCALAPPDATA%\\VODscribe");
  log(`installed ${exe}`);

  // A release doesn't let WebDriver in (lib.rs webview_args): the test build,
  // over the one installed, with everything else as installed.
  if (!testApp) throw new Error("Windows needs --test-app: vodscribe.exe built with the e2e feature");
  scp(testApp, `${runner.host}:${runner.dir}/vodscribe-e2e.exe`);
  ps(`Copy-Item -Force '${dir}\\vodscribe-e2e.exe' '${exe}'`);
  log("the test build put over it");

  // A scheduled task runs in the signed-in user's session, with a desktop.
  const args = rest.map((a) => `"${a}"`).join(" ");
  const extra = (runner.path ?? []).map((p) => p.replaceAll("/", "\\"));
  const cmd = [
    "@echo off",
    ...(extra.length ? [`set PATH=${extra.join(";")};%PATH%`] : []),
    "set E2E_RESET_DATA=1",
    "set VODSCRIBE_NO_UPDATE_CHECK=1",
    ...(process.env.E2E_NETWORK === "1" ? ["set E2E_NETWORK=1"] : []),
    `set E2E_OUT=${dir}\\out`,
    `cd /d "${dir}"`,
    `if exist out rmdir /s /q out`,
    "mkdir out",
    // A window over the desktop fails the screen-sampling scenarios: seen
    // first. The check failing for itself (not 3) doesn't stop the tests.
    "powershell -NoProfile -ExecutionPolicy Bypass -File desktop.ps1 > out\\desktop.log 2>&1",
    "if errorlevel 3 (type out\\desktop.log > out\\run.log & echo 3 > out\\exit.txt & exit /b 3)",
    `node tests\\e2e\\run.mjs "${exe}" --driver bridge ${args} > out\\run.log 2>&1`,
    "set CODE=%ERRORLEVEL%",
    // One of Windows' own offers (an upgrade screen, Copilot) can come up
    // during the suite and fail what follows: a failed run looks again.
    'if not "%CODE%"=="0" powershell -NoProfile -ExecutionPolicy Bypass -File desktop.ps1 after > out\\desktop-after.log 2>&1',
    "echo %CODE% > out\\exit.txt",
  ].join("\r\n");
  const local = path.join(out, "run.cmd");
  writeFileSync(local, cmd);
  scp(local, `${runner.host}:${runner.dir}/run.cmd`);
  const desktop = path.join(out, "desktop.ps1");
  writeFileSync(desktop, WINDOWS_DESKTOP);
  scp(desktop, `${runner.host}:${runner.dir}/desktop.ps1`);
  // Started through a script host with no window of its own: run.cmd's
  // console would otherwise open on the desktop, and on a computer whose
  // console is Windows Terminal it opens large and in front, over the app's
  // picture (the pixel checks then see the console, not the app).
  const hidden = path.join(out, "run.vbs");
  // It waits for run.cmd, so the task is running for as long as the tests are.
  writeFileSync(hidden, `CreateObject("WScript.Shell").Run """${dir}\\run.cmd""", 0, True\r\n`);
  scp(hidden, `${runner.host}:${runner.dir}/run.vbs`);
  // The task in the signed-in user's session, with no trigger: started only
  // here. (`schtasks /create /sc once /st 23:59` also started it at 23:59 that
  // day, a second run over the first, whose exit.txt ended the wait.)
  const registered = ps(
    [
      "$ErrorActionPreference = 'Stop'",
      `$action = New-ScheduledTaskAction -Execute 'wscript.exe' -Argument '"${dir}\\run.vbs"'`,
      "$user = New-ScheduledTaskPrincipal -UserId ([Security.Principal.WindowsIdentity]::GetCurrent().Name) -LogonType Interactive",
      "Register-ScheduledTask -TaskName vodscribe-e2e -Action $action -Principal $user -Force | Out-Null; 'registered'",
    ].join("\n"),
  );
  if (!registered.endsWith("registered")) throw new Error(`couldn't make the scheduled task on ${name}: ${registered}`);
  // The last run's results gone first, its exit.txt above all: the wait below
  // ends on one.
  const left = ps(
    `Remove-Item -LiteralPath '${dir}\\out' -Recurse -Force -ErrorAction SilentlyContinue; Test-Path -LiteralPath '${dir}\\out'`,
  );
  if (left === "True") throw new Error(`couldn't clear ${runner.dir}/out on ${name}: a file in it is in use`);
  ssh(`schtasks /run /tn vodscribe-e2e`, { stdio: "ignore" });
  log("running");
  const begun = Date.now();
  let shown = 0;
  for (let lost = 0; ;) {
    await sleep(15_000);
    // The tests go on in the task if the connection drops: keep asking.
    let text;
    try {
      text = ps(`if (Test-Path '${dir}\\out\\run.log') { Get-Content -Raw '${dir}\\out\\run.log' }`);
      lost = 0;
    } catch (e) {
      if (++lost > 40) throw e;
      if (lost === 1) log(`lost the connection (${e.message.split("\n").at(-2) ?? e.message}); still waiting`);
      continue;
    }
    const lines = text.split(/\r?\n/).filter((l) => SHOWN.test(l));
    for (const l of lines.slice(shown)) console.log(`  ${l}`);
    shown = lines.length;
    if (ps(`Test-Path '${dir}\\out\\exit.txt'`) === "True") break;
    if (!text && Date.now() - begun > 60_000)
      throw new Error("the scheduled task hasn't started the tests in a minute");
  }
  scp("-r", `${runner.host}:${runner.dir}/out/*`, out);
  // Nothing left to start on its own.
  ps(`Unregister-ScheduledTask -TaskName vodscribe-e2e -Confirm:$false -ErrorAction SilentlyContinue; exit 0`);
  // What's in front, as desktop.ps1 found it before the suite and after a
  // failed one, or what went wrong with the check itself.
  for (const [file, when] of [
    ["desktop.log", "desktop"],
    ["desktop-after.log", "desktop after the failed run"],
  ]) {
    const checked = path.join(out, file);
    const said = existsSync(checked) ? readFileSync(checked, "utf8").trim() : "";
    const front = said.split(/\r?\n/).find((l) => l.startsWith("{"));
    if (said) log(`${when}: ${front ? windowsNotClear(JSON.parse(front)) : said}`);
  }
  return Number(readFileSync(path.join(out, "exit.txt"), "utf8").trim());
}

/** What's on the Mac's screen, read by JavaScript for Automation from the
 *  window server (not System Events: scripting that over SSH puts up a
 *  permission prompt of its own): the frontmost app, then each window on
 *  screen as owner, layer and title, tab-separated. */
const MAC_WINDOWS = `ObjC.import("AppKit"); ObjC.import("CoreGraphics");
const front = $.NSWorkspace.sharedWorkspace.frontmostApplication;
const ref = $.CGWindowListCopyWindowInfo($.kCGWindowListOptionOnScreenOnly | $.kCGWindowListExcludeDesktopElements, 0);
const windows = ObjC.deepUnwrap(ObjC.castRefToObject(ref)) || [];
[front ? front.localizedName.js : "", ...windows.filter((w) => w.kCGWindowAlpha > 0)
  .map((w) => [w.kCGWindowOwnerName, w.kCGWindowLayer, w.kCGWindowName || ""].join("\\t"))].join("\\n");`;
/** The runner's screen before the suite, kept as desktop-before.png: a
 *  window left over it (a system alert, a prompt, another app) sits over
 *  what the scenarios sample from the screen, and fails them while the app
 *  is fine. Throws, listing what's there, if any app but Finder is in front
 *  or has a window up (lib/runnerScreen.mjs macScreen). */
async function macDesktop() {
  const shot = `${runner.dir}/desktop-before.png`;
  const look = () => {
    // The display woken first, so the picture is of the desktop, not a dark screen.
    ssh(`caffeinate -u -t 1; screencapture -x '${shot}'`);
    scp(`${runner.host}:${shot}`, path.join(out, "desktop-before.png"));
    const [front, ...windows] = ssh("osascript -l JavaScript -", { input: MAC_WINDOWS }).trim().split("\n");
    const listed = windows.map((w) => w.split("\t"));
    writeFileSync(
      path.join(out, "desktop-before.txt"),
      `frontmost: ${front}\n${listed.map((w) => w.join("  ")).join("\n")}\n`,
    );
    return { front, ...macScreen(front, listed) };
  };
  let seen = look();
  if (seen.wake) {
    // Just after a start, nothing activated yet: Finder brought to the front
    // through Launch Services (System Events would ask for permission over SSH).
    log("the login window is in front with nothing up (just started?): activating Finder");
    ssh("open -a Finder");
    await sleep(2000);
    seen = look();
  }
  if (!seen.clear) throw new Error(macNotClear(name, seen.front, seen.foreign, path.join(out, "desktop-before.png")));
  log("desktop clear");
}

async function mac() {
  sendHarness(`mkdir -p '${runner.dir}'`);
  log("harness sent");
  await macDesktop();
  const target = `${runner.dir}/${path.basename(app)}`;
  ssh(`rm -rf '${target}'`);
  scp("-r", app, `${runner.host}:${runner.dir}/`);
  log(`copied ${path.basename(app)}`);
  const args = rest.map((a) => `'${a}'`).join(" ");
  const run = spawnSync(
    "ssh",
    [
      ...shared,
      runner.host,
      `export PATH="$HOME/.local/node/bin:$PATH" E2E_RESET_DATA=1 VODSCRIBE_NO_UPDATE_CHECK=1 E2E_OUT='${runner.dir}/out'` +
        `${process.env.E2E_NETWORK === "1" ? " E2E_NETWORK=1" : ""}` +
        `${process.env.E2E_SCREEN ? ` E2E_SCREEN='${process.env.E2E_SCREEN}'` : ""}; ` +
        `caffeinate -u -t 1; cd '${runner.dir}' && rm -rf out && node tests/e2e/run.mjs '${target}' ${args}`,
    ],
    { stdio: ["ignore", "pipe", "inherit"], encoding: "utf8", maxBuffer: 1 << 28 },
  );
  for (const l of run.stdout.split("\n").filter((l) => SHOWN.test(l))) console.log(`  ${l}`);
  spawnSync("scp", ["-q", ...shared, "-r", `${runner.host}:${runner.dir}/out/*`, out], { stdio: "inherit" });
  return run.status;
}

const code = runner.os === "windows" ? await windows() : runner.os === "macos" ? await mac() : null;
if (code === null) throw new Error(`runner ${name}: os ${runner.os} isn't one this can drive (windows, macos)`);
log(`results in ${out}`);
process.exit(code);
