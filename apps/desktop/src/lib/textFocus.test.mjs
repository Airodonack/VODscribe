// A press on the timeline (or the player, the transcript, the clip bar, the
// library's list) and Escape let go of a text field (textFocus.ts), the
// page's elements faked: each is where it sits, the selectors it's inside.
// node --test apps/desktop/src/lib/textFocus.test.mjs
import assert from "node:assert/strict";
import { test } from "node:test";

import { AREAS, escapeLetsGoOfText, isTextField, pressLetsGoOfText } from "./textFocus.ts";

/**
 * An element inside each of `within` (selectors as the page's: AREAS,
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

// Source: textFocus.ts header; 0.7.1 Fixes ("Search lets go of the keys.").
test("a press on the timeline lets go of the search box, and the keys are the timeline's", () => {
  const search = element([AREAS, ".toolbar", "input"], "search");
  const timeline = element([AREAS]);
  assert.equal(pressLetsGoOfText(timeline, search), true);
  assert.equal(search.focused, false);
});

// Source: textFocus.ts `AREAS`.
test("a press on the library's list, the clip bar or the transcript lets go of a title being written", () => {
  // Each is marked as one of the places a press lets go of text (AREAS).
  const title = element([AREAS, "input"], "text");
  assert.equal(pressLetsGoOfText(element([AREAS]), title), true);
  assert.equal(title.focused, false);
});

// Source: textFocus.ts `KEEPS`.
test("a press in the toolbar, on another field, in a dialog or a menu keeps the field's focus", () => {
  for (const where of [
    [AREAS, ".toolbar"],
    [AREAS, "input"],
    [AREAS, "[role=dialog]"],
    [AREAS, "[role=menu]"],
    [AREAS, ".grouping"],
    // Outside the main side and the list: the library's own buttons, the side pane.
    [],
  ]) {
    const search = element([AREAS, ".toolbar", "input"], "search");
    assert.equal(pressLetsGoOfText(element(where), search), false, where.join());
    assert.equal(search.focused, true);
  }
  // Nothing to let go of: a button, a checkbox, nothing focused.
  const box = element([AREAS, "input"], "checkbox");
  assert.equal(pressLetsGoOfText(element([AREAS]), box), false);
  assert.equal(box.focused, true);
  assert.equal(pressLetsGoOfText(element([AREAS]), null), false);
});

// Source: textFocus.ts `escapeLetsGoOfText`.
test("Escape in a field lets go of it, unless the field took it", () => {
  const search = element([AREAS, ".toolbar", "input"], "search");
  assert.equal(escapeLetsGoOfText({ key: "a", target: search, defaultPrevented: false }), false);
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: search, defaultPrevented: true }), false);
  assert.equal(search.focused, true);
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: search, defaultPrevented: false }), true);
  assert.equal(search.focused, false);
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: element([AREAS]), defaultPrevented: false }), false);
});

// Source for the tests below: textFocus.ts's header ("a press on the player,
// the timeline, the transcript, the clip bar or the library's list lets go of
// a text field being written in ... and so does Escape in it") and its
// `isTextField`, `pressLetsGoOfText` and `escapeLetsGoOfText` doc comments;
// 0.7.1 Fixes ("Search lets go of the keys.").

test("a text input, a textarea and an editable element are text fields; a button, a checkbox and a slider are not", () => {
  assert.equal(isTextField(element(["input"], "text")), true);
  assert.equal(isTextField(element(["input"], "search")), true);
  assert.equal(isTextField(element(["textarea"])), true);
  assert.equal(isTextField(element(["[contenteditable]"])), true);
  for (const type of ["button", "checkbox", "color", "file", "radio", "range", "reset", "submit"])
    assert.equal(isTextField(element(["input"], type)), false, type);
  assert.equal(isTextField(element([])), false);
  assert.equal(isTextField(null), false);
});

test("Escape in a checkbox or a plain element keeps the focus", () => {
  const box = element(["input"], "checkbox");
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: box, defaultPrevented: false }), false);
  assert.equal(box.focused, true);
  assert.equal(escapeLetsGoOfText({ key: "Escape", target: null, defaultPrevented: false }), false);
});

test("a press with no target lets go of nothing, and a field focused stays so", () => {
  const search = element(["input"], "search");
  assert.equal(pressLetsGoOfText(null, search), false);
  assert.equal(search.focused, true);
});

test("a press in the search's own chips, an editable title or a menu in the main side keeps the focus", () => {
  for (const kept of [".grouping", "[contenteditable]", "[role=menu]", "textarea", "select"]) {
    const field = element(["input"], "text");
    assert.equal(pressLetsGoOfText(element([AREAS, kept]), field), false, kept);
    assert.equal(field.focused, true);
  }
});

test("the areas are the elements the page marks as letting go of text", () => {
  assert.equal(AREAS, "[data-press-blurs-text]");
});
