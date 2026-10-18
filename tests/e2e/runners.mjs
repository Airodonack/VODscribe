// Runs the end-to-end tests on other computers ("runners"), reached over SSH,
// and brings the results back to tests/e2e/out/<runner>/.
//
//   node tests/e2e/runners.mjs <runner> <app> [run.mjs options, e.g. --only a,b]
//
// Runners are listed in tests/e2e/runners.json (not in the repository; see
// runners.example.json): a name, the SSH host, its OS, a folder there to
// work in, and optionally `path`: folders put first on PATH for the tests
// (Windows), for tools installed only for the user. <app> is what to test, on this computer:
//
// - Windows: an NSIS installer (*-setup.exe). It's installed for the user,
//   silently, and driven over WebDriver; the runner needs Node and
//   tauri-driver on PATH. The msedgedriver matching its WebView2 is fetched.
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
  console.error("usage: node tests/e2e/runners.mjs <runner> <app> [run.mjs options]");
  process.exit(2);
}
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

/** Copies the harness (tests/e2e, as committed plus what's new) to the runner's folder. */
function sendHarness(mkdir) {
  ssh(mkdir);
  const files = execFileSync("git", ["ls-files", "-co", "--exclude-standard", "tests/e2e"], {
    cwd: repo,
    encoding: "utf8",
  })
    .split("\n")
    .filter((f) => f && !f.startsWith("tests/e2e/out/"));
  const tar = spawnSync("tar", ["-cf", "-", ...files], { cwd: repo, maxBuffer: 1 << 30 });
  if (tar.status !== 0) throw new Error(`tar: ${tar.stderr}`);
  const unpack = runner.os === "windows" ? `tar -xf - -C "${runner.dir}"` : `tar -xf - -C '${runner.dir}'`;
  execFileSync("ssh", [...shared, runner.host, unpack], { input: tar.stdout });
}

async function windows() {
  const dir = runner.dir.replaceAll("/", "\\");
  // PowerShell reads the script from stdin: no quoting through cmd.exe.
  const ps = (script) => ssh("powershell -NoProfile -NonInteractive -Command -", { input: `${script}\n` }).trim();
  // The scheduled task runs in a signed-in session, and only then.
  if (!ps(`(quser 2>$null) -match 'Active'; exit 0`))
    throw new Error(`no one is signed in on ${name}: the tests need someone's desktop (sign in, then run this again)`);
  // What the tests need there, found the way the task will look for it.
  const pathed = (runner.path ?? []).map((p) => `$env:PATH = '${p.replaceAll("/", "\\")};' + $env:PATH;`).join(" ");
  for (const tool of ["node", "tauri-driver"]) {
    if (!ps(`${pathed} (Get-Command ${tool} -ErrorAction SilentlyContinue).Source; exit 0`))
      throw new Error(`${name} has no ${tool} on PATH (or in its "path" in runners.json)`);
  }
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

  // WebView2 is driven by the msedgedriver of its exact version.
  const version = ps(
    `$k = 'HKLM:\\SOFTWARE\\WOW6432Node\\Microsoft\\EdgeUpdate\\Clients\\{F3017226-FE2A-4295-8BDF-00C3A9A7E4C5}'; (Get-ItemProperty $k).pv`,
  );
  const driver = `${dir}\\edgedriver-${version}\\msedgedriver.exe`;
  if (ps(`Test-Path '${driver}'`) !== "True") {
    ps(
      `Invoke-WebRequest 'https://msedgedriver.microsoft.com/${version}/edgedriver_win64.zip' -OutFile '${dir}\\edgedriver.zip'; Expand-Archive -Force '${dir}\\edgedriver.zip' -DestinationPath '${dir}\\edgedriver-${version}'`,
    );
  }
  log(`WebView2 ${version}`);

  // A scheduled task runs in the signed-in user's session, with a desktop.
  const args = rest.map((a) => `"${a}"`).join(" ");
  const extra = (runner.path ?? []).map((p) => p.replaceAll("/", "\\"));
  const cmd = [
    "@echo off",
    ...(extra.length ? [`set PATH=${extra.join(";")};%PATH%`] : []),
    "set E2E_RESET_DATA=1",
    "set VODSCRIBE_NO_UPDATE_CHECK=1",
    ...(process.env.E2E_NETWORK === "1" ? ["set E2E_NETWORK=1"] : []),
    `set NATIVE_DRIVER=${driver}`,
    `set E2E_OUT=${dir}\\out`,
    `cd /d "${dir}"`,
    `if exist out rmdir /s /q out`,
    "mkdir out",
    `node tests\\e2e\\run.mjs "${exe}" ${args} > out\\run.log 2>&1`,
    "echo %ERRORLEVEL% > out\\exit.txt",
  ].join("\r\n");
  const local = path.join(out, "run.cmd");
  writeFileSync(local, cmd);
  scp(local, `${runner.host}:${runner.dir}/run.cmd`);
  ssh(`schtasks /create /tn vodscribe-e2e /sc once /st 23:59 /it /f /tr "${dir}\\run.cmd"`, { stdio: "ignore" });
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
  return Number(readFileSync(path.join(out, "exit.txt"), "utf8").trim());
}

function mac() {
  sendHarness(`mkdir -p '${runner.dir}'`);
  log("harness sent");
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
