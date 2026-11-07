// A press that closed a menu (Popover.svelte's `closeOn="press"`) does
// nothing else: it and the rest of its click are kept from the page, as in a
// file manager, where a click on a row closes the row's menu without opening
// or picking the row. It ends with the click, or once the release has come
// without one, or when the release won't come: the press cancelled, or a new
// press begun (on a Mac, a Control-click's release is lost after its
// contextmenu), which then goes on as usual.

const REST = ["mousedown", "pointerup", "mouseup", "click"];
/** What says the press's release isn't coming. */
const OVER = ["pointerdown", "pointercancel"];

/** Keeps press `down`, and the rest of its click, from the page (`doc`). */
export function swallowClick(down: Event, doc: EventTarget = document) {
  const end = () => {
    REST.forEach((type) => doc.removeEventListener(type, stop, true));
    OVER.forEach((type) => doc.removeEventListener(type, over, true));
  };
  function stop(e: Event) {
    e.preventDefault();
    e.stopImmediatePropagation();
    if (e.type === "click") end();
    // Let go away from where it was pressed, there's no click: it ends here.
    else if (e.type === "pointerup" || e.type === "mouseup") setTimeout(end);
  }
  function over(e: Event) {
    if (e !== down) end();
  }
  REST.forEach((type) => doc.addEventListener(type, stop, true));
  OVER.forEach((type) => doc.addEventListener(type, over, true));
  stop(down);
}
