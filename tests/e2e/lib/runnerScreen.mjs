// What runners.mjs makes of a runner's screen before the suite: whether it's
// clear, and if not, what to tell the person at it. A window over the
// desktop sits over what the scenarios sample from the screen and fails
// them while the app is fine, so the run doesn't start until it's closed.

/** Window layers from the Dock's up are the Mac's own (the Dock, the menu
 *  bar, Notification Center, the pointer); an app's windows and its dialogs
 *  sit below. */
export const MAC_DOCK_LAYER = 20;

/**
 * The Mac's screen judged, from its frontmost app and its windows on screen
 * (each [owner, layer, title]): `clear` when Finder is in front and no other
 * app has a window up; `foreign`, the windows in the way; `wake` when
 * nothing is in the way but the login window is the frontmost app with no
 * window of its own, as just after a start before any app was activated:
 * activating Finder clears that, where closing a window doesn't.
 */
export function macScreen(front, windows) {
  const foreign = windows.filter(([owner, layer]) => owner !== "Finder" && Number(layer) < MAC_DOCK_LAYER);
  const wake = front === "loginwindow" && !foreign.length && !windows.some(([owner]) => owner === "loginwindow");
  return { clear: front === "Finder" && !foreign.length, wake, foreign };
}

/** What `name`'s Mac screen (`macScreen`'s `foreign`) has in the way, said for the person at it. */
export function macNotClear(name, front, foreign, shot) {
  const named = foreign.map(([owner, , title]) => (title ? `${owner} (${title})` : owner));
  return (
    `${name}'s screen isn't clear: ${front} is in front${named.length ? `, and up: ${named.join(", ")}` : ""}` +
    ` (see ${shot}); close it there, then run this again`
  );
}

/** Processes whose windows are Windows' own prompts and offers: a Microsoft
 *  account's sign-in (WWAHost), the "finish setting up" and upgrade screens,
 *  update notices, Copilot. */
const WINDOWS_PROMPTS = new Set(
  ["WWAHost", "UserOOBEBroker", "MoNotificationUx", "MusNotificationUx", "Copilot", "copilotapp"].map((p) =>
    p.toLowerCase(),
  ),
);

/**
 * What to say of the window in front on a Windows runner, as desktop.ps1
 * found it: { owner (its process), title, class, system (the process is
 * Windows' own, under its folder), fullScreen }. One of Windows' prompts
 * (a known one, or any of Windows' own covering the screen but File
 * Explorer) is named as such, so it's closed rather than taken for the app.
 */
export function windowsNotClear({ owner, title, system, fullScreen }) {
  const named = title ? `${owner} (${title})` : owner;
  const prompt =
    WINDOWS_PROMPTS.has(String(owner).toLowerCase()) ||
    (system && fullScreen && String(owner).toLowerCase() !== "explorer");
  if (!prompt) return `the screen isn't clear: ${named} is in front`;
  return (
    `the screen isn't clear: ${named} is in front, a prompt of Windows' own${fullScreen ? " over the whole screen" : ""}` +
    ` (an offer to sign in, upgrade or try something), not the app: answer or close it on the runner's screen, then run this again`
  );
}
