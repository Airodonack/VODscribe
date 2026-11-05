// A screen smaller than the test computer's, for the scenarios that measure
// layout: E2E_SCREEN names one, and the harness then sizes the app's page as
// that screen would. Unset, the page is what the window and the display give.
//
// hosted-mac: CI's hosted macOS runner, a 1024 by 768 screen. Its 1200 by 800
// window opens with a page 613 px tall (the menu bar, the title bar and the
// Dock take the rest), and after a resize the system keeps the window on the
// screen with a page 581 px tall at most; the width is as asked (1600 px
// wide when a scenario asks for it). Layout checks that passed here on a
// 900 px display failed there (2026-10, annotations' pinned headings).

/** Each screen's page heights: when the app opens, and after a resize. */
const SCREENS = {
  "hosted-mac": { opened: 613, held: 581 },
};

/** The screen E2E_SCREEN names ({ name, opened, held }), or null when it's unset. */
export function shortScreen() {
  const name = process.env.E2E_SCREEN;
  if (!name) return null;
  const screen = SCREENS[name];
  if (!screen) throw new Error(`E2E_SCREEN=${name}: no such screen (there are: ${Object.keys(SCREENS).join(", ")})`);
  return { name, ...screen };
}

// The scenario's resizes so far, and whether it was told of a hold: a
// restart's new app is on the same screen, so this lasts the scenario.
let resizes = 0;
let told = false;

/** A new scenario: its first resize is the first again. */
export function newScenario() {
  resizes = 0;
  told = false;
}

/** The page height a resize to `width` by `height` gets on the short screen
 *  (unchanged without one), logging a hold once per scenario. */
export function heightFor(width, height, log) {
  const screen = shortScreen();
  if (!screen) return height;
  const most = resizes++ === 0 ? screen.opened : screen.held;
  if (height <= most) return height;
  if (most === screen.held && !told) {
    told = true;
    log(`held to ${width}x${most} (${screen.name})`);
  }
  return most;
}
