// A press on the timeline (or the player, the transcript, the clip bar, the
// library's list) and Escape let go of a text field (textFocus.ts), the
// page's elements faked: each is where it sits, the selectors it's inside.
// node --test apps/desktop/src/lib/textFocus.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

import { escapeLetsGoOfText, pressLetsGoOfText } from "./textFocus.ts";

/**
 * An element inside each of `within` (selectors as the page's: ".main",
 * "input", "[role=dialog]"), focused until it lets go.
 * @param {string[]} within @param {string} [type]
 */
function element(within, type) {
  const el = {
    type,
    focused: true,
    /** @param {string} selector */
    closest: (selector) => (selector.split(",").some((s) => within.includes(s.trim())) ? el : null),
    blur: () => void (el.focused = false),
  };
  return el;
}

test("a press on the timeline lets go of the search box, and the keys are the timeline's", () => {
  const search = element([".main", ".toolbar", "input"], "search");
  const timeline = element([".main"]);
  assert.equal(pressLetsGoOfText(timeline, search), true);
  assert.equal(search.focused, false);
});

test("a press on the library's list, the clip bar or the transcript lets go of a title being written", () => {
  for (const where of [[".library-list"], [".main"]]) {
    const title = element([".main", "input"], "text");
    assert.equal(pressLetsGoOfText(element(where), title), true, where.join());
    assert.equal(title.focused, false);
  }
});

test("a press in the toolbar, on another field, in a dialog or a menu keeps the field's focus", () => {
  for (const where of [
    [".main", ".toolbar"],
    [".main", "input"],
    [".main", "[role=dialog]"],
    [".main", "[role=menu]"],
    [".main", ".grouping"],
    // Outside the main side and the list: the library's own buttons, the side pane.
    [],
  ]) {
    const search = element([".main", ".toolbar", "input"], "search");
    assert.equal(pressLetsGoOfText(element(where), search), false, where.join());
    assert.equal(search.focused, true);
  }
  // Nothing to let go of: a button, a checkbox, nothing focused.
  const box = element([".main", "input"], "checkbox");
  assert.equal(pressLetsGoOfText(element([".main"]), box), false);
  assert.equal(box.focused, true);
  assert.equal(pressLetsGoOfText(element([".main"]), null), false);
});

test("Escape in a field lets go of it, unless the field took it", () => {
  const search = element([".main", ".toolbar", "input"], "search");
  assert.equal(escapeLetsGoOfText({ key: "a", target: search, defaultPrevented: false }), false);
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: search, defaultPrevented: true }), false);
  assert.equal(search.focused, true);
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: search, defaultPrevented: false }), true);
  assert.equal(search.focused, false);
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: element([".main"]), defaultPrevented: false }), false);
});
