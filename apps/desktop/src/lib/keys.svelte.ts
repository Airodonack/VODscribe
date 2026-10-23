// The keyboard controls in force: this OS's defaults from the action table
// (keymap.ts), with the user's own bindings over them. The handlers ask
// `is(e, id)`; legends and titles show `labelOf(id)` or `bindingsOf(id)`,
// so a rebound key shows wherever it's named.

import { buildMap, bindingOf, conflict as conflictIn, label, type Action, type ActionId, type KeyLike } from "./keymap";
import { os } from "./platform";

class Keys {
  /** The user's bindings, by action id: the whole list for each action
   *  changed (an empty one: no key). None yet: Settings will keep them. */
  overrides = $state.raw<Record<string, string[]>>({});
  #map = $derived(buildMap(os, this.overrides));

  /** `id`'s bindings, the one shown first. */
  bindingsOf(id: ActionId): string[] {
    return this.#map.get(id) ?? [];
  }

  /** Whether the key press `e` fires action `id`. */
  is(e: KeyLike, id: ActionId): boolean {
    const pressed = bindingOf(e);
    return pressed !== null && this.bindingsOf(id).includes(pressed);
  }

  /** `id`'s first key as this OS writes it ("⌘F", "Ctrl+F"), or "" if it has none. */
  labelOf(id: ActionId): string {
    const first = this.bindingsOf(id)[0];
    return first === undefined ? "" : label(first, os);
  }

  /** The action live with `id` that already has `binding`, or null. */
  conflict(binding: string, id: ActionId): Action | null {
    return conflictIn(this.#map, binding, id, os);
  }
}

export const keys = new Keys();

export const is = (e: KeyLike, id: ActionId) => keys.is(e, id);
export const labelOf = (id: ActionId) => keys.labelOf(id);
export const bindingsOf = (id: ActionId) => keys.bindingsOf(id);
/** `id`'s key for a control's title: " (⌘F)", or nothing if it has none. */
export const keyHint = (id: ActionId) => (keys.labelOf(id) ? ` (${keys.labelOf(id)})` : "");
