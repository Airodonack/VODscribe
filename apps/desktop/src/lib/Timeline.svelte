<script lang="ts">
  import type { Layer, Vod } from "./api";
  import Emote from "./Emote.svelte";
  import Icon from "./Icon.svelte";
  import { MAX_VISIBLE, layers } from "./layers.svelte";

  // The Timeline button above the minimap, and its panel for the layers drawn
  // on it: saved chat searches, each a list of terms shown as pills.
  let {
    vod,
    emotes,
    counts,
  }: {
    vod: Vod;
    /** Emote images in this VOD's chat, by lowercase name, to show terms as their emote. */
    emotes: Map<string, string>;
    /** How many of this VOD's messages each shown layer matched, by layer id. */
    counts: Map<string, number>;
  } = $props();

  let open = $state(false);
  let anchor = $state<HTMLElement>();
  let mine = $derived(layers.forVod(vod));
  let elsewhere = $derived(layers.list.length - mine.length);
  let shown = $derived(layers.shownFor(vod));

  // The panel is placed against the window, not inside the scrolling
  // transcript area, which would cut it off: below the button when there's
  // room, otherwise above it (over the player), and scrolling within that.
  const ROOM = 280;
  let place = $state<{ top?: number; bottom?: number; right: number; maxHeight: number } | null>(null);
  function fit() {
    if (!open || !anchor) return;
    const box = anchor.getBoundingClientRect();
    const right = window.innerWidth - box.right;
    const below = window.innerHeight - box.bottom - 16;
    const above = box.top - 16;
    // Too little room either side (a short window): as tall as the window, over the button.
    const least = 160;
    place =
      below >= ROOM || (below >= above && below >= least)
        ? { top: box.bottom + 8, right, maxHeight: below - 8 }
        : above >= least
          ? { bottom: window.innerHeight - box.top + 8, right, maxHeight: above - 8 }
          : { top: 8, right, maxHeight: window.innerHeight - 16 };
  }
  $effect(() => {
    if (open) fit();
  });

  // By the click's path when it happened: removing a pill takes its button out
  // of the page before this runs, and that's still a click inside.
  function onWindowClick(e: MouseEvent) {
    if (open && anchor && !e.composedPath().includes(anchor)) open = false;
  }

  function addTerm(layer: Layer, input: HTMLInputElement) {
    const text = input.value.trim().replace(/,$/, "").trim();
    input.value = "";
    if (!text || layer.terms.some((t) => t.text.toLowerCase() === text.toLowerCase())) return;
    layer.terms.push({ text, emote: emotes.get(text.toLowerCase()) ?? null });
    layers.save();
  }

  function onTermKey(e: KeyboardEvent, layer: Layer) {
    const input = e.currentTarget as HTMLInputElement;
    if (e.key === "Enter" || e.key === ",") {
      e.preventDefault();
      addTerm(layer, input);
    } else if (e.key === "Backspace" && input.value === "" && layer.terms.length > 0) {
      layer.terms.pop();
      layers.save();
    }
  }

  function toggleShown(layer: Layer) {
    layer.visible = !layer.visible;
    layers.save();
  }

  function toggleChannel(layer: Layer) {
    layer.channel = layer.channel ? null : { platform: vod.platform, channel: vod.channel ?? "" };
    layers.save();
  }
</script>

<svelte:window
  onclick={onWindowClick}
  onkeydown={(e) => e.key === "Escape" && (open = false)}
  onresize={fit}
  onscrollcapture={fit}
/>

<div class="anchor" bind:this={anchor}>
  <button
    class="timeline"
    class:on={open}
    onclick={() => {
      open = !open;
      if (open) void layers.load();
    }}
    aria-expanded={open}
    title="Layers on the timeline: chat words and emotes to look for"
  >
    <Icon name="layers" size={14} /> Timeline
    {#each shown as layer (layer.id)}<span class="dot" style:background={layer.color}></span>{/each}
  </button>
  {#if open}
    <div
      class="popover"
      role="dialog"
      aria-label="Timeline layers"
      style:top={place?.top !== undefined ? `${place.top}px` : null}
      style:bottom={place?.bottom !== undefined ? `${place.bottom}px` : null}
      style:right="{place?.right ?? 0}px"
      style:max-height="{place?.maxHeight ?? 0}px"
    >
      <p class="intro muted">
        Words and emotes to look for in chat. Up to {MAX_VISIBLE} show on the timeline at once, and their messages are highlighted.
      </p>
      {#if layers.error}<p class="error-text">{layers.error}</p>{/if}
      {#each mine as layer (layer.id)}
        {@const isShown = shown.includes(layer)}
        <section class="layer" class:hidden={!isShown}>
          <div class="top">
            <label class="swatch" style:background={layer.color} title="Change the color">
              <input
                type="color"
                value={layer.color}
                oninput={(e) => {
                  layer.color = e.currentTarget.value;
                  layers.save();
                }}
                aria-label="{layer.name}'s color"
              />
            </label>
            <input
              class="name"
              bind:value={layer.name}
              oninput={() => layers.save()}
              aria-label="Layer name"
              spellcheck="false"
            />
            {#if isShown && counts.has(layer.id)}<span class="count muted">{counts.get(layer.id)}</span>{/if}
            <button
              class="icon"
              onclick={() => toggleShown(layer)}
              disabled={!layer.visible && !layers.canShowMore(vod)}
              aria-pressed={layer.visible}
              title={layer.visible
                ? "Hide from the timeline"
                : layers.canShowMore(vod)
                  ? "Show on the timeline"
                  : `${MAX_VISIBLE} layers show at once; hide one first`}
            >
              <Icon name={layer.visible ? "eye" : "eye-off"} size={15} />
            </button>
            <button class="icon" onclick={() => layers.remove(layer.id)} title="Delete this layer">
              <Icon name="trash" size={14} />
            </button>
          </div>
          <div class="terms">
            {#each layer.terms as term, i (term.text)}
              {@const image = term.emote ?? emotes.get(term.text.toLowerCase())}
              <span class="pill" title={term.text}>
                {#if image}<Emote name={term.text} url={image} />{:else}{term.text}{/if}
                <button
                  class="remove"
                  aria-label="Remove {term.text}"
                  onclick={() => {
                    layer.terms.splice(i, 1);
                    layers.save();
                  }}><Icon name="x" size={11} /></button
                >
              </span>
            {/each}
            <input
              class="add-term"
              placeholder={layer.terms.length ? "Add…" : "Add a word or emote…"}
              onkeydown={(e) => onTermKey(e, layer)}
              onblur={(e) => addTerm(layer, e.currentTarget)}
              aria-label="Add a term to {layer.name}"
              spellcheck="false"
            />
          </div>
          {#if vod.channel}
            <button class="scope" onclick={() => toggleChannel(layer)}>
              {#if layer.channel}
                <Icon name="lock" size={11} /> Only {layer.channel.channel}'s VODs
              {:else}
                All channels
              {/if}
              <span class="muted">· make it {layer.channel ? "every channel's" : `${vod.channel}'s only`}</span>
            </button>
          {/if}
        </section>
      {/each}
      <button class="add" onclick={() => layers.add(vod)}><Icon name="plus" size={13} /> Add a layer</button>
      {#if elsewhere > 0}
        <p class="muted elsewhere">{elsewhere} more for other channels.</p>
      {/if}
    </div>
  {/if}
</div>

<style>
  .anchor {
    position: relative;
  }

  .timeline {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    font-size: 12px;
    padding: 3px 8px;
  }

  .timeline.on {
    border-color: var(--accent);
  }

  .dot {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .popover {
    position: fixed;
    z-index: 10;
    width: 340px;
    overflow-y: auto;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
  }

  .intro {
    margin: 0 0 10px;
    font-size: 12px;
  }

  .layer {
    padding: 8px 0 10px;
    border-top: 1px solid var(--border);
  }

  .layer.hidden .swatch,
  .layer.hidden .terms {
    opacity: 0.55;
  }

  .top {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  /* The layer's color, opening the system color picker. */
  .swatch {
    position: relative;
    flex: none;
    width: 14px;
    height: 14px;
    border-radius: 4px;
    cursor: pointer;
    box-shadow: inset 0 0 0 1px color-mix(in srgb, var(--text) 15%, transparent);
  }

  .swatch:hover {
    outline: 2px solid var(--border);
    outline-offset: 1px;
  }

  .swatch input {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    opacity: 0;
    cursor: pointer;
    border: none;
    padding: 0;
  }

  .name {
    flex: 1;
    min-width: 0;
    padding: 2px 4px;
    border: 1px solid transparent;
    background: none;
    font-weight: 600;
    font-size: 13px;
  }

  .name:hover,
  .name:focus {
    border-color: var(--border);
  }

  .count {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .terms {
    display: flex;
    flex-wrap: wrap;
    gap: 4px;
    margin: 6px 0 0 16px;
  }

  .pill {
    display: inline-flex;
    align-items: center;
    gap: 2px;
    height: 24px;
    padding: 0 2px 0 8px;
    border-radius: 12px;
    background: var(--surface-2);
    font-size: 12px;
  }

  .remove {
    display: inline-flex;
    border: none;
    background: none;
    padding: 3px;
    border-radius: 50%;
    color: var(--muted);
  }

  .remove:hover {
    color: var(--danger);
  }

  .add-term {
    flex: 1;
    min-width: 90px;
    height: 24px;
    padding: 0 6px;
    font-size: 12px;
    border: 1px dashed var(--border);
    background: none;
  }

  .scope {
    margin: 6px 0 0 14px;
    border: none;
    background: none;
    padding: 2px;
    font-size: 11px;
    display: inline-flex;
    align-items: center;
    gap: 4px;
  }

  .scope:hover {
    text-decoration: underline;
  }

  .add {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    margin-top: 6px;
    font-size: 12px;
  }

  .elsewhere {
    margin: 8px 0 0;
    font-size: 12px;
  }
</style>
