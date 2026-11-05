// The runners' screen checks (runnerScreen.mjs): what's in the way, and
// what the person at the runner is told.

import assert from "node:assert/strict";
import { test } from "node:test";

import { macNotClear, macScreen, windowsNotClear } from "./runnerScreen.mjs";

// The system's own, always up (a Mac just after a start).
const SYSTEM = [
  ["Window Server", "2147483630", "Cursor"],
  ["Control Center", "25", "Clock"],
  ["Window Server", "24", "Menubar"],
  ["Notification Center", "23", "Notification Center"],
  ["Dock", "20", "Dock"],
];

test("Finder in front with only the system's windows and its own is clear", () => {
  assert.deepEqual(macScreen("Finder", [...SYSTEM, ["Finder", "0", "Downloads"]]), {
    clear: true,
    wake: false,
    foreign: [],
  });
});

test("the login window in front with no window of its own: Finder is to be activated", () => {
  assert.deepEqual(macScreen("loginwindow", SYSTEM), { clear: false, wake: true, foreign: [] });
});

test("the login window with a window up (a lock or sign-in screen) isn't woken past", () => {
  const { clear, wake } = macScreen("loginwindow", [...SYSTEM, ["loginwindow", "2002", ""]]);
  assert.deepEqual([clear, wake], [false, false]);
});

test("another app's window is in the way, whoever is in front, and named", () => {
  const dialog = ["Software Update", "0", "Software Update"];
  for (const front of ["Finder", "loginwindow", "Safari"]) {
    const { clear, wake, foreign } = macScreen(front, [...SYSTEM, dialog]);
    assert.deepEqual([clear, wake, foreign], [false, false, [dialog]]);
  }
  assert.equal(
    macNotClear("the-mac", "Finder", [dialog], "/out/desktop-before.png"),
    "the-mac's screen isn't clear: Finder is in front, and up: Software Update (Software Update)" +
      " (see /out/desktop-before.png); close it there, then run this again",
  );
  assert.equal(
    macNotClear("the-mac", "Safari", [], "/s.png"),
    "the-mac's screen isn't clear: Safari is in front (see /s.png); close it there, then run this again",
  );
});

test("Windows' own prompts are named as such: a known one, or one of Windows' over the whole screen", () => {
  for (const front of [
    // Seen on a runner.
    { owner: "WWAHost", title: "Microsoft account", system: true, fullScreen: false },
    { owner: "copilotapp", title: "Copilot", system: false, fullScreen: true },
    // An upgrade screen by a process of Windows' not listed.
    { owner: "SomeUpgradeUx", title: "", system: true, fullScreen: true },
  ])
    assert.match(windowsNotClear(front), /is in front, a prompt of Windows' own.*not the app/, front.owner);
  assert.equal(
    windowsNotClear({ owner: "WWAHost", title: "Microsoft account", system: true, fullScreen: true }),
    "the screen isn't clear: WWAHost (Microsoft account) is in front, a prompt of Windows' own over the whole screen" +
      " (an offer to sign in, upgrade or try something), not the app: answer or close it on the runner's screen, then run this again",
  );
});

test("an app's window, or File Explorer's, is only said to be in front", () => {
  assert.equal(
    windowsNotClear({ owner: "explorer", title: "drop-here - File Explorer", system: true, fullScreen: true }),
    "the screen isn't clear: explorer (drop-here - File Explorer) is in front",
  );
  assert.equal(
    windowsNotClear({ owner: "CapCut", title: "CapCut", system: false, fullScreen: true }),
    "the screen isn't clear: CapCut (CapCut) is in front",
  );
  assert.equal(
    windowsNotClear({ owner: "notepad", title: "", system: true, fullScreen: false }),
    "the screen isn't clear: notepad is in front",
  );
});
