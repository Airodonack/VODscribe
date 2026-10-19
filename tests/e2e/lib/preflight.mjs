// What a run needs of the computer it runs on, looked at before any scenario
// (run.mjs), so a gap is named once rather than found as a scenario's
// timeout: another VODscribe running (the single-instance plugin hands each
// test app to it, and every scenario fails "exited before it listened"), no
// sound output (WebKit's <audio> never starts without one), a locked screen
// (a hidden window gets no animation frames, and the player's time stands).

import { execFileSync } from "node:child_process";

const run = (cmd, args) => {
  try {
    return execFileSync(cmd, args, { encoding: "utf8", stdio: ["ignore", "pipe", "ignore"], timeout: 15_000 });
  } catch {
    return null;
  }
};

/** Other VODscribe processes running, as "name (pid)". On Linux the
 *  handoff goes over the D-Bus session, which the runs keep to themselves
 *  (dbus-run-session), so another there is no matter. */
function othersRunning() {
  if (process.platform === "linux") return [];
  if (process.platform === "win32") {
    const out = run("tasklist", ["/FI", "IMAGENAME eq vodscribe.exe", "/FO", "CSV", "/NH"]) ?? "";
    return out
      .split("\n")
      .filter((l) => l.toLowerCase().startsWith('"vodscribe.exe"'))
      .map((l) => `vodscribe.exe (${l.split(",")[1]?.replaceAll('"', "")})`);
  }
  const out = run("pgrep", ["-lx", "VODscribe"]) ?? "";
  return out
    .trim()
    .split("\n")
    .filter(Boolean)
    .map((l) => {
      const [pid, name] = l.split(" ");
      return `${name} (${pid})`;
    });
}

/** Whether there's a sound output to play to; null where it can't be told. */
function soundOutput() {
  if (process.platform === "darwin") {
    const out = run("system_profiler", ["SPAudioDataType"]);
    return out === null ? null : /Output Channels:|Default Output Device: Yes/.test(out);
  }
  if (process.platform === "win32") {
    const out = run("powershell", [
      "-NoProfile",
      "-Command",
      // An output endpoint that works (speakers, headphones), not just a sound card.
      "@(Get-PnpDevice -Class AudioEndpoint -Status OK | Where-Object FriendlyName -notmatch 'Microphone|Line In').Count",
    ]);
    return out === null ? null : Number(out.trim()) > 0;
  }
  // Linux: a sound server or ALSA device, if one's set up (the scripts give a null sink).
  return null;
}

/** Whether the screen is locked; null where it can't be told. */
function screenLocked() {
  if (process.platform === "darwin") {
    const out = run("ioreg", ["-n", "Root", "-d1", "-a"]);
    return out === null ? null : /<key>CGSSessionScreenIsLocked<\/key>\s*<true\/>/.test(out);
  }
  if (process.platform === "win32") {
    // The lock screen is LogonUI running in the user's session.
    const out = run("tasklist", ["/FI", "IMAGENAME eq LogonUI.exe", "/FO", "CSV", "/NH"]);
    return out === null ? null : out.toLowerCase().includes("logonui.exe");
  }
  return null;
}

/**
 * Looks the computer over: throws if the run can't work at all (another
 * VODscribe running), and returns warnings for what some scenarios need.
 */
export function preflight() {
  const others = othersRunning();
  if (others.length)
    throw new Error(
      `another VODscribe is running (${others.join(", ")}): it would take each test app's place. Quit it first.`,
    );
  const warnings = [];
  if (soundOutput() === false) warnings.push("no sound output: scenarios that play sound will time out");
  if (screenLocked() === true) warnings.push("the screen is locked: the player's time won't move");
  return warnings;
}
