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
// testing only. E2E_NETWORK=1 here is passed on (scenarios that reach the sites).

import { execFileSync, spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, rmSync, writeFileSync } from "node:fs";
import path from "node:path";
import { tmpdir } from "node:os";
import { setTimeout as sleep } from "node:timers/promises";
import { fileURLToPath } from "node:url";

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

/** The Windows runner's screen before the suite, run in the scheduled task
 *  (over SSH there's no desktop to see): a screenshot to out\desktop-before.png,
 *  the windows up to out\desktop-before.txt, and exit 3 if a window is in
 *  front that isn't the desktop, the taskbar or the task's own console. */
const WINDOWS_DESKTOP = String.raw`$ErrorActionPreference = 'Stop'
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
  public static string Text(IntPtr h) { var s = new StringBuilder(512); GetWindowText(h, s, 512); return s.ToString(); }
  public static string Class(IntPtr h) { var s = new StringBuilder(256); GetClassName(h, s, 256); return s.ToString(); }
}
'@
$screen = [System.Windows.Forms.SystemInformation]::VirtualScreen
$bitmap = New-Object System.Drawing.Bitmap $screen.Width, $screen.Height
[System.Drawing.Graphics]::FromImage($bitmap).CopyFromScreen($screen.Left, $screen.Top, 0, 0, $bitmap.Size)
$bitmap.Save("$PSScriptRoot\out\desktop-before.png", [System.Drawing.Imaging.ImageFormat]::Png)
$front = [Desk]::GetForegroundWindow()
$id = 0; [void][Desk]::GetWindowThreadProcessId($front, [ref]$id)
$owner = (Get-Process -Id $id -ErrorAction SilentlyContinue).ProcessName
$title = [Desk]::Text($front); $class = [Desk]::Class($front)
$up = Get-Process | Where-Object { $_.MainWindowTitle } | ForEach-Object { "$($_.ProcessName)  $($_.MainWindowTitle)" }
@("in front: $owner  $class  $title") + $up | Set-Content "$PSScriptRoot\out\desktop-before.txt"
# Nothing in front, the desktop, the taskbar, or the console this task runs in.
$own = $front -eq [Desk]::GetConsoleWindow() -or $title -like '*run.cmd*'
if ($front -eq [IntPtr]::Zero -or $own -or $class -in 'Progman', 'WorkerW', 'Shell_TrayWnd') { exit 0 }
Write-Output "the screen isn't clear: $owner ($title) is in front"
exit 3
`;

async function windows() {
  const dir = runner.dir.replaceAll("/", "\\");
  // PowerShell reads the script from stdin: no quoting through cmd.exe.
  const ps = (script) => ssh("powershell -NoProfile -NonInteractive -Command -", { input: `${script}\n` }).trim();
  // The scheduled task runs in a signed-in session, and only then.
  if (!ps(`(quser 2>$null) -match 'Active'; exit 0`))
    throw new Error(`no one is signed in on ${name}: the tests need someone's desktop (sign in, then run this again)`);
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
    "echo %ERRORLEVEL% > out\\exit.txt",
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
  writeFileSync(hidden, `CreateObject("WScript.Shell").Run """${dir}\\run.cmd""", 0, False\r\n`);
  scp(hidden, `${runner.host}:${runner.dir}/run.vbs`);
  ssh(`schtasks /create /tn vodscribe-e2e /sc once /st 23:59 /it /f /tr "wscript.exe ${dir}\\run.vbs"`, {
    stdio: "ignore",
  });
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
    const lines = text.split(/\r?\n/).filter((l) => /^\[\d+ s\]|^e2e:|^  (ok|FAIL)|FAILED/.test(l));
    for (const l of lines.slice(shown)) console.log(`  ${l}`);
    shown = lines.length;
    if (ps(`Test-Path '${dir}\\out\\exit.txt'`) === "True") break;
    if (!text && Date.now() - begun > 60_000)
      throw new Error("the scheduled task hasn't started the tests in a minute");
  }
  scp("-r", `${runner.host}:${runner.dir}/out/*`, out);
  const checked = path.join(out, "desktop.log");
  if (existsSync(checked) && readFileSync(checked, "utf8").trim())
    log(`desktop: ${readFileSync(checked, "utf8").trim()}`);
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
/** Window layers from the Dock's up are the system's own (the Dock, the
 *  menu bar, Notification Center, the pointer); an app's windows and its
 *  dialogs sit below. */
const MAC_DOCK_LAYER = 20;

/** The runner's screen before the suite, kept as desktop-before.png: a
 *  window left over it (a system alert, a prompt, another app) sits over
 *  what the scenarios sample from the screen, and fails them while the app
 *  is fine. Throws, listing what's there, if any app but Finder is in front
 *  or has a window up. */
function macDesktop() {
  const shot = `${runner.dir}/desktop-before.png`;
  // The display woken first, so the picture is of the desktop, not a dark screen.
  ssh(`caffeinate -u -t 1; screencapture -x '${shot}'`);
  scp(`${runner.host}:${shot}`, path.join(out, "desktop-before.png"));
  const [front, ...windows] = ssh("osascript -l JavaScript -", { input: MAC_WINDOWS }).trim().split("\n");
  const listed = windows.map((w) => w.split("\t"));
  const foreign = listed.filter(([owner, layer]) => owner !== "Finder" && Number(layer) < MAC_DOCK_LAYER);
  writeFileSync(
    path.join(out, "desktop-before.txt"),
    `frontmost: ${front}\n${listed.map((w) => w.join("  ")).join("\n")}\n`,
  );
  if (front !== "Finder" || foreign.length) {
    const named = foreign.map(([owner, , title]) => (title ? `${owner} (${title})` : owner));
    throw new Error(
      `${name}'s screen isn't clear: ${front} is in front${named.length ? `, and up: ${named.join(", ")}` : ""}` +
        ` (see ${path.join(out, "desktop-before.png")}); close it there, then run this again`,
    );
  }
  log("desktop clear");
}

function mac() {
  sendHarness(`mkdir -p '${runner.dir}'`);
  log("harness sent");
  macDesktop();
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
        `${process.env.E2E_NETWORK === "1" ? " E2E_NETWORK=1" : ""}; ` +
        `caffeinate -u -t 1; cd '${runner.dir}' && rm -rf out && node tests/e2e/run.mjs '${target}' ${args}`,
    ],
    { stdio: ["ignore", "pipe", "inherit"], encoding: "utf8", maxBuffer: 1 << 28 },
  );
  for (const l of run.stdout.split("\n").filter((l) => /^\[\d+ s\]|^e2e:|^  (ok|FAIL)|FAILED/.test(l)))
    console.log(`  ${l}`);
  spawnSync("scp", ["-q", ...shared, "-r", `${runner.host}:${runner.dir}/out/*`, out], { stdio: "inherit" });
  return run.status;
}

const code = runner.os === "windows" ? await windows() : runner.os === "macos" ? mac() : null;
if (code === null) throw new Error(`runner ${name}: os ${runner.os} isn't one this can drive (windows, macos)`);
log(`results in ${out}`);
process.exit(code);
