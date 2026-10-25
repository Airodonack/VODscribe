<script lang="ts">
  import type { Layer, Vod } from "./api";
  import Emote from "./Emote.svelte";
  import Icon from "./Icon.svelte";
  import { EVENTS, eventColor, eventName, eventRgb, type EventKind } from "./events";
  import { MAX_VISIBLE, layers } from "./layers.svelte";
  import Popover from "./Popover.svelte";
  import { cuts } from "./cuts.svelte";
  import { isImported } from "./markSources";

  // The Timeline button above the minimap, and its panel for what's drawn on
  // it: whose chapters and highlights (the user's own, those imported), and
  // the layers, saved chat searches, each a list of terms shown as pills.
  let {
    vod,
    emotes,
    counts,
    eventCounts,
  }: {
    vod: Vod;
    /** Emote images in this VOD's chat, by lowercase name, to show terms as their emote. */
    emotes: Map<string, string>;
    /** How many of this VOD's messages each shown layer matched, by layer id. */
    counts: Map<string, number>;
    /** How many of each event this VOD's chat has. */
    eventCounts: Map<EventKind, number>;
  } = $props();

  let open = $state(false);
  let anchor = $state<HTMLElement>();
  let mine = $derived(layers.forVod(vod));
  let elsewhere = $derived(layers.list.length - mine.length);
  let shown = $derived(layers.shownFor(vod));

  // This VOD's chapters and highlights, the user's and those imported (once its marks are in).
  const named = $derived(
    cuts.isOf({ kind: "vod", id: vod.id })
      ? cuts.all.filter((m) => m.kind === "chapter" || m.kind === "highlight")
      : [],
  );
  const MARKS = [
    { key: "user", label: "Your marks", icon: "star" },
    { key: "imported", label: "Imported marks", icon: "gift" },
  ] as const;
  const markCount = (key: "user" | "imported") => named.filter((m) => isImported(m) === (key === "imported")).length;

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
    <!-- The events: the platform's color when any are on, gray when none are. -->
    <span
      class="dot"
      class:off={layers.events.length === 0}
      style:background={layers.events.length > 0 ? eventColor(vod.platform) : null}
    ></span>
    {#each shown as layer (layer.id)}<span class="dot" style:background={layer.color}></span>{/each}
  </button>
  <Popover bind:open {anchor} label="Timeline layers" width={352}>
    <!-- Whose chapters and highlights: on the timeline, in the pane and in the transcript. -->
    <section class="marks" aria-label="Marks">
      {#each MARKS as m (m.key)}
        {@const on = layers.marks[m.key]}
        <button
          class="event"
          class:on
          style:--event="var(--accent)"
          onclick={() => cuts.setSources({ [m.key]: !on })}
          aria-pressed={on}
          title={on ? "Hide these chapters and highlights" : "Show these chapters and highlights"}
        >
          <Icon name={m.icon} size={13} />
          {m.label}
          <span class="count">{markCount(m.key)}</span>
        </button>
      {/each}
    </section>
    <p class="intro muted">
      Events and words to look for in chat, drawn on the timeline with their messages highlighted. Up to {MAX_VISIBLE}
      layers show at once, besides the events.
    </p>
    {#if layers.error}<p class="error-text">{layers.error}</p>{/if}
    <!-- Editable once the saved layers are in, so an edit can't be lost to them or overwrite them. -->
    {#if !layers.loaded}
      {#if !layers.error}<p class="muted">Loading your layers…</p>{/if}
    {:else}
      <section class="events" aria-label="Events">
        {#each EVENTS as event (event.kind)}
          {@const on = layers.events.includes(event.kind)}
          <button
            class="event"
            class:on
            style:--event={eventColor(vod.platform)}
            style:--event-rgb={eventRgb(vod.platform)}
            onclick={() => layers.toggleEvent(event.kind)}
            aria-pressed={on}
            title={on ? "Hide from the timeline" : "Show on the timeline"}
          >
            <Icon name={event.icon} size={13} />
            {eventName(event.kind, vod.platform)}
            <span class="count">{eventCounts.get(event.kind) ?? 0}</span>
          </button>
        {/each}
      </section>
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
              autocorrect="off"
              autocapitalize="off"
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
              autocorrect="off"
              autocapitalize="off"
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
    {/if}
  </Popover>
</div>

<style>
  .anchor {
    position: relative;
    /* Keeps its size in the header; the status beside it is what gets cut short. */
    flex: none;
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

  .dot.off {
    background: color-mix(in srgb, var(--muted) 55%, transparent);
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
  /* The events: one row of toggles in the platform's color, each kind at its own strength. */
  .events,
  .marks {
    display: flex;
    flex-wrap: wrap;
    gap: 5px;
    padding: 8px 0 10px;
    border-top: 1px solid var(--border);
  }

  /* Whose marks: first in the panel, above what's about chat. */
  .marks {
    padding-top: 0;
    border-top: none;
  }

  .event {
    /* Each as wide as its name ("Super Chats" is long); the row wraps if they don't fit. */
    flex: 1 1 auto;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    gap: 4px;
    min-width: 0;
    padding: 4px 5px;
    font-size: 12px;
    font-weight: 600;
    white-space: nowrap;
    color: var(--muted);
  }

  .event.on {
    color: var(--text);
    border-color: var(--event);
    background: color-mix(in srgb, var(--event) 22%, transparent);
  }

  .event :global(svg) {
    color: var(--event);
  }

  .event .count {
    font-weight: 400;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
  }

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
