<script lang="ts">
  // Settings › Controls: every action of the key map (keymap.ts) with its
  // keys, each one removable, more added by pressing them, one action or all
  // put back as they were. A change is saved at once and shows wherever the
  // key is named (the legends under the players, the buttons' titles).
  import { tick } from "svelte";
  import { ask } from "./ask";
  import Icon from "./Icon.svelte";
  import { ACTIONS, bindingOf, label, type Action, type ActionId, type Scope } from "./keymap";
  import { keys } from "./keys.svelte";
  import { os } from "./platform";

  // The scopes as the user knows them, in the table's order.
  const GROUPS: { scope: Scope; heading: string }[] = [
    { scope: "app", heading: "Everywhere" },
    { scope: "player", heading: "Playing" },
    { scope: "edit", heading: "In an edit" },
    { scope: "mark", heading: "With a cut or mark selected" },
  ];

  let filter = $state("");
  const groups = $derived.by(() => {
    const words = filter.trim().toLowerCase();
    const shown = (a: (typeof ACTIONS)[number]) =>
      !words ||
      a.label.toLowerCase().includes(words) ||
      keys.bindingsOf(a.id).some((b) => label(b, os).toLowerCase().includes(words));
    return GROUPS.map((g) => ({ ...g, actions: ACTIONS.filter((a) => a.scope === g.scope && shown(a)) })).filter(
      (g) => g.actions.length,
    );
  });
  const anyChanged = $derived(ACTIONS.some((a) => keys.changed(a.id)));

  // The action waiting for its new key, and a key pressed for one that
  // another action live with it already has: asked about before it's taken.
  let recording = $state<ActionId | null>(null);
  let clash = $state<{ id: ActionId; binding: string; other: Action } | null>(null);

  function startRecording(id: ActionId) {
    clash = null;
    recording = id;
  }

  // Back to the row's "+" once it's done, so the next key isn't the window's.
  async function stopRecording(id: ActionId) {
    recording = null;
    await tick();
    document.querySelector<HTMLElement>(`[data-add="${id}"]`)?.focus();
  }

  // Keys that name no key a binding could be: a dead key of a layout, or one
  // an input method has.
  const NO_KEY = new Set(["Dead", "Unidentified", "Process"]);

  // The press while a row waits: it's the new key, and nothing else of the
  // app's sees it (Space mustn't scroll nor F fill the screen; Escape mustn't
  // close Settings). Escape cancels; Tab moves on, as always; a modifier on
  // its own waits for the key it's held with.
  function record(e: KeyboardEvent, id: ActionId) {
    if (e.key === "Tab") return;
    e.preventDefault();
    e.stopPropagation();
    if (e.repeat) return;
    if (e.key === "Escape") return void stopRecording(id);
    const binding = bindingOf(e);
    if (binding === null || NO_KEY.has(e.key)) return;
    void stopRecording(id);
    const mine = keys.bindingsOf(id);
    if (mine.includes(binding)) return;
    const other = keys.conflict(binding, id);
    if (other) clash = { id, binding, other };
    else void keys.set({ [id]: [...mine, binding] });
  }

  // Replace: the key goes from the other action to this one.
  function replace() {
    if (!clash) return;
    const { id, binding, other } = clash;
    clash = null;
    void keys.set({
      [id]: [...keys.bindingsOf(id), binding],
      [other.id]: keys.bindingsOf(other.id as ActionId).filter((b) => b !== binding),
    });
  }

  const remove = (id: ActionId, binding: string) =>
    keys.set({ [id]: keys.bindingsOf(id).filter((b) => b !== binding) });

  async function resetAll() {
    const ok = await ask("Put every keyboard control back as it was when VODscribe was new?", "Reset", "Keep");
    if (ok) await keys.resetAll();
  }

  const focus = (el: HTMLElement) => el.focus();
  const alt = os === "mac" ? "Option" : "Alt";
</script>

<section>
  <span class="eyebrow">Controls</span>
  <input type="search" placeholder="Filter controls" aria-label="Filter controls" bind:value={filter} />
  {#each groups as g (g.scope)}
    <div class="group" role="group" aria-label={g.heading}>
      <p class="label">{g.heading}</p>
      {#each g.actions as a (a.id)}
        <div class="control" data-action={a.id}>
          <span class="what">{a.label}</span>
          <span class="bindings">
            {#each keys.bindingsOf(a.id) as b (b)}
              <span class="chip"
                ><kbd>{label(b, os)}</kbd><button
                  class="remove"
                  aria-label="Remove {label(b, os)} from {a.label}"
                  title="Remove"
                  onclick={() => remove(a.id, b)}><Icon name="x" size={11} /></button
                ></span
              >
            {/each}
            {#if recording === a.id}
              <span
                class="recorder"
                role="button"
                tabindex="0"
                aria-label="Press a key for {a.label}, or Escape to cancel"
                use:focus
                onkeydown={(e) => record(e, a.id)}
                onblur={() => recording === a.id && (recording = null)}
                onclick={() => (recording = null)}>Press a key…</span
              >
            {:else}
              <button
                class="add"
                data-add={a.id}
                aria-label="Add a key to {a.label}"
                title="Add a key"
                onclick={() => startRecording(a.id)}><Icon name="plus" size={12} /></button
              >
            {/if}
          </span>
          <span class="reset">
            {#if keys.changed(a.id)}
              <button class="link" aria-label="Reset {a.label}" onclick={() => keys.reset(a.id)}>Reset</button>
            {/if}
          </span>
        </div>
        {#if clash?.id === a.id}
          <div class="clash" role="alert">
            <span><kbd>{label(clash.binding, os)}</kbd> Used by {clash.other.label}.</span>
            <button onclick={replace}>Replace</button>
            <button onclick={() => (clash = null)}>Cancel</button>
          </div>
        {/if}
      {/each}
    </div>
  {:else}
    <p class="muted hint">No control matches “{filter.trim()}”.</p>
  {/each}
  <button class="link" onclick={resetAll} disabled={!anyChanged}>Reset all controls…</button>
  <p class="muted hint note">
    Always the same: Esc cancels, deselects, leaves full screen and closes; Tab moves between buttons; keys typed in a
    text box are its own; {alt}+↑/↓ moves a row of the library, and Enter or F2 renames a clip; with the mouse, {alt}+drag
    on the timeline or Clip drags a clip out, and the mouse's back and forward buttons go back and forward.
  </p>
</section>

<style>
  section {
    display: grid;
    grid-template-columns: minmax(0, 1fr);
    gap: 8px;
    padding: 14px 16px;
    justify-items: start;
  }

  input[type="search"] {
    width: 100%;
  }

  .group {
    display: grid;
    width: 100%;
    margin-top: 6px;
  }

  .label {
    margin: 0 0 2px;
    font-size: 13px;
    font-weight: 600;
  }

  /* One action a line: what it does, its keys, Reset when they're its own. */
  .control {
    display: grid;
    grid-template-columns: minmax(0, 1fr) auto 44px;
    align-items: center;
    gap: 8px;
    min-height: 30px;
    padding: 2px 0;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
  }

  .bindings {
    display: flex;
    flex-wrap: wrap;
    justify-content: flex-end;
    align-items: center;
    gap: 4px;
  }

  .chip {
    display: inline-flex;
    align-items: center;
  }

  .chip kbd {
    border-top-right-radius: 0;
    border-bottom-right-radius: 0;
  }

  /* The ×, joined to its key cap. */
  .chip .remove {
    display: inline-grid;
    place-items: center;
    width: 18px;
    align-self: stretch;
    padding: 0;
    border: 1px solid var(--border);
    border-left: none;
    border-bottom-width: 2px;
    border-radius: 0 4px 4px 0;
    background: var(--surface);
    color: var(--muted);
  }

  .chip .remove:hover {
    color: var(--danger);
  }

  .add {
    display: inline-grid;
    place-items: center;
    width: 22px;
    height: 22px;
    padding: 0;
    color: var(--muted);
  }

  .recorder {
    padding: 1px 8px;
    border: 1px dashed var(--accent);
    border-radius: 4px;
    background: var(--accent-soft);
    color: var(--text);
    cursor: default;
  }

  .recorder:focus {
    outline: none;
  }

  .reset {
    text-align: right;
  }

  .clash {
    display: flex;
    flex-wrap: wrap;
    align-items: center;
    justify-content: flex-end;
    gap: 8px;
    padding: 6px 0;
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    color: var(--warn);
  }

  .clash button {
    padding: 2px 10px;
    font-size: 12px;
  }

  .hint {
    margin: 0;
    font-size: 12px;
  }

  .note {
    margin-top: 4px;
  }
</style>
