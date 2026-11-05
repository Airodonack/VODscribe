// Text focus and the timeline's keys take turns (App.svelte asks here): a
// press on the player, the timeline, the transcript, the clip bar or the
// library's list lets go of a text field being written in (the search box,
// a title), and so does Escape in it, so the next Ctrl+C, Ctrl+V, Delete or
// letter key is the selection's, not the field's. The browser does this by
// itself only where a press has its default: the timeline and the clip bar
// take theirs (a drag), so the field kept the focus and the keys. A key
// typed in a field never reaches the timeline (App.svelte `typing`).
// Pure: node --test apps/desktop/src/lib/textFocus.test.mjs.

/** What a press or a key needs of an element: `closest`, as the DOM's. */
type Node = { closest(selector: string): unknown };
/** The element with the focus: one that can let go of it. */
type Focused = Node & { blur(): void; type?: string };

/** Where a press lets go of a text field: the main side (the player, the
 *  timeline, the transcript, the clip bar) and the library's list. */
const AREAS = ".main, .library-list";
/** What keeps it: the toolbar (the search box, its filters), the search's
 *  own chips, another field, a dialog or a menu. */
const KEEPS = ".toolbar, .grouping, input, textarea, select, [contenteditable], [role=dialog], [role=menu]";
/** Inputs with no text to write. */
const TEXTLESS = new Set(["button", "checkbox", "color", "file", "radio", "range", "reset", "submit"]);

/** Whether `el` is a field text is written in. */
export function isTextField(el: Focused | null): el is Focused {
  return !!el?.closest("input, textarea, [contenteditable]") && !TEXTLESS.has(el.type ?? "");
}

/** A press on `target` (a pointerdown, seen first): `focused` (the page's
 *  active element) lets go of the focus if it's a text field and the press
 *  is on the player, the timeline, the transcript, the clip bar or the
 *  library's list. True if it did. */
export function pressLetsGoOfText(target: Node | null, focused: Focused | null): boolean {
  if (!target || !isTextField(focused) || !target.closest(AREAS) || target.closest(KEEPS)) return false;
  focused.blur();
  return true;
}

/** Escape in a text field (`target`, the key's): the field lets go of the
 *  focus. True if it did. */
export function escapeLetsGoOfText(e: { key: string; target: unknown; defaultPrevented: boolean }): boolean {
  const field = e.target as Focused | null;
  if (e.key !== "Escape" || e.defaultPrevented || !isTextField(field)) return false;
  field.blur();
  return true;
}
