// The keyboard controls in force: this OS's defaults from the action table
// (keymap.ts), with the user's own bindings over them, kept in the settings
// (`key_bindings`, loaded once at the start as the theme is). The handlers
// ask `is(e, id)`; legends and titles show `labelOf(id)` or `bindingsOf(id)`,
// so a rebound key shows wherever it's named. Settings › Controls changes
// them with `set`, `reset` and `resetAll`.

import { settings } from "./appSettings.svelte";
import {
  ACTIONS,
  buildMap,
  canonical,
  conflicts as conflictsIn,
  defaultsOf,
  label,
  matches,
  type Action,
  type ActionId,
  type KeyLike,
} from "./keymap";
import { os } from "./platform";

/** `id`'s default bindings on this OS, as the map writes them. */
const defaults = (id: ActionId) => {
  const action = ACTIONS.find((a) => a.id === id);
  return action ? defaultsOf(action, os).map((b) => canonical(b) ?? b) : [];
};
const same = (a: readonly string[], b: readonly string[]) => a.length === b.length && a.every((x, i) => x === b[i]);

class Keys {
  /** The user's bindings, by action id: the whole list for each action
   *  changed (an empty one: no key). Ids the table doesn't have (from a
   *  later version, say) are kept, and saved back as they came. */
  overrides = $derived<Record<string, string[]>>(settings.value?.key_bindings ?? {});
  #map = $derived(buildMap(os, this.overrides));

  /** `id`'s bindings, the one shown first. */
  bindingsOf(id: ActionId): string[] {
    return this.#map.get(id) ?? [];
  }

  /** Whether the key press `e` fires action `id`. */
  is(e: KeyLike, id: ActionId): boolean {
    return matches(e, this.bindingsOf(id));
  }

  /** `id`'s first key as this OS writes it ("⌘F", "Ctrl+F"), or "" if it has none. */
  labelOf(id: ActionId): string {
    const first = this.bindingsOf(id)[0];
    return first === undefined ? "" : label(first, os);
  }

  /** The actions live with `id` that already have `binding`; empty if none. */
  conflicts(binding: string, id: ActionId): Action[] {
    return conflictsIn(this.#map, binding, id, os);
  }

  /** Whether `id`'s bindings are other than its defaults. */
  changed(id: ActionId): boolean {
    return !same(this.bindingsOf(id), defaults(id));
  }

  /**
   * Gives each action in `lists` those bindings and saves them all. The
   * overrides go to the settings whole, worked out over what they are when
   * the save's turn comes, so two quick changes can't drop one; a list that's
   * the defaults again is no override.
   */
  set(lists: Partial<Record<ActionId, string[]>>): Promise<void> {
    return settings.update((now) => {
      const next = { ...now.key_bindings };
      for (const [id, list] of Object.entries(lists) as [ActionId, string[]][]) {
        if (same(list, defaults(id))) delete next[id];
        else next[id] = list;
      }
      return { key_bindings: next };
    });
  }

  /** `id`'s default bindings back. */
  reset(id: ActionId): Promise<void> {
    return this.set({ [id]: defaults(id) });
  }

  /** Every action's default bindings back (the other settings stay). */
  resetAll(): Promise<void> {
    return settings.update({ key_bindings: {} });
  }
}

export const keys = new Keys();

export const is = (e: KeyLike, id: ActionId) => keys.is(e, id);
export const labelOf = (id: ActionId) => keys.labelOf(id);
export const bindingsOf = (id: ActionId) => keys.bindingsOf(id);
/** `id`'s key for a control's title: " (⌘F)", or nothing if it has none. */
export const keyHint = (id: ActionId) => (keys.labelOf(id) ? ` (${keys.labelOf(id)})` : "");
