<script lang="ts">
  import { flushSync } from "svelte";
  import { flip } from "svelte/animate";
  import type { EmoteProvider } from "./api";
  import Icon from "./Icon.svelte";
  import bttvLogo from "./logos/bttv.png";
  import ffzLogo from "./logos/ffz.png";
  import { settings } from "./settings.svelte";

  // The emote providers chat's word emotes come from (crates/core
  // `word_emotes`), in one row: click one to turn it on or off, drag it (or
  // Alt+←/→) to change which comes first when two have an emote of the same
  // name. All start off: each one turned on is a service contacted. Each shows
  // its provider's logo (logos/README.md), grayed out while off.

  const ABOUT: Record<string, { name: string; host: string; logo: string | null }> = {
    // 7TV's logo is drawn below, in the text's color, as 7TV draws it.
    "7tv": { name: "7TV", host: "7tv.io", logo: null },
    bttv: { name: "BetterTTV", host: "betterttv.net", logo: bttvLogo },
    ffz: { name: "FrankerFaceZ", host: "frankerfacez.com", logo: ffzLogo },
  };

  let order = $state<EmoteProvider[]>([]);
  $effect(() => {
    void settings.load();
  });
  // Follows the saved order, except while a drag is rearranging it.
  $effect(() => {
    const saved = settings.value?.emote_providers;
    if (saved && !drag) order = saved.map((p) => ({ ...p }));
  });

  const save = () => settings.update({ emote_providers: order.map((p) => ({ ...p })) });

  function toggle(id: string) {
    order = order.map((p) => (p.id === id ? { ...p, on: !p.on } : p));
    void save();
  }

  function move(id: string, by: number) {
    const from = order.findIndex((p) => p.id === id);
    const to = from + by;
    if (from < 0 || to < 0 || to >= order.length) return;
    const next = [...order];
    next.splice(to, 0, ...next.splice(from, 1));
    order = next;
    void save();
  }

  // Dragging: a press that moves a few pixels is a drag rather than a click.
  // The dragged button follows the pointer; the others make way for it as it
  // passes their middles, and it settles into its place when let go.
  const THRESHOLD = 4;
  let row = $state<HTMLElement>();
  let buttons: Record<string, HTMLElement | null> = {};
  interface Drag {
    id: string;
    /** Where the press was, and how far into the button. */
    x: number;
    grab: number;
    moved: boolean;
    /** How far the button is drawn from its place in the row. */
    dx: number;
  }
  let drag = $state<Drag | null>(null);
  // The click after a drag isn't a toggle.
  let dragged = false;

  function onDown(e: PointerEvent, id: string) {
    if (e.button !== 0) return;
    // A drag that ended without a click (let go off the button) mustn't eat this press's.
    dragged = false;
    const box = (e.currentTarget as HTMLElement).getBoundingClientRect();
    drag = { id, x: e.clientX, grab: e.clientX - box.left, moved: false, dx: 0 };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    if (!drag || !row) return;
    if (!drag.moved && Math.abs(e.clientX - drag.x) < THRESHOLD) return;
    drag.moved = true;
    const id = drag.id;
    const others = order.filter((p) => p.id !== id);
    // Its place: after every other button whose middle the pointer has passed.
    const at = others.filter((p) => {
      const el = buttons[p.id];
      return el && e.clientX - row!.getBoundingClientRect().left > el.offsetLeft + el.offsetWidth / 2;
    }).length;
    const current = order.findIndex((p) => p.id === id);
    if (current !== at) {
      const next = [...others];
      next.splice(at, 0, order[current]);
      flushSync(() => (order = next));
    }
    // Drawn where the pointer holds it, from its (new) place in the row.
    const el = buttons[id];
    if (el) drag.dx = e.clientX - row.getBoundingClientRect().left - drag.grab - el.offsetLeft;
  }

  function onUp() {
    if (!drag) return;
    dragged = drag.moved;
    drag = null;
    if (dragged) void save();
  }

  function onClick(id: string) {
    if (dragged) {
      dragged = false;
      return;
    }
    toggle(id);
  }

  function onKey(e: KeyboardEvent, id: string) {
    if (!e.altKey || (e.key !== "ArrowLeft" && e.key !== "ArrowRight")) return;
    e.preventDefault();
    move(id, e.key === "ArrowLeft" ? -1 : 1);
    // The button is re-placed; keep the focus on it.
    queueMicrotask(() => buttons[id]?.focus());
  }

  /** Slides the buttons making way; the one being dragged follows the pointer instead. */
  function makeWay(node: Element, places: { from: DOMRect; to: DOMRect }, held: { held: boolean }) {
    return held.held ? { duration: 0 } : flip(node, places, { duration: 150 });
  }

  let anyOn = $derived(order.some((p) => p.on));
</script>

<div class="row" bind:this={row} role="group" aria-label="Chat emote providers, first one wins">
  {#each order as p (p.id)}
    {@const about = ABOUT[p.id]}
    {@const held = drag?.id === p.id && drag.moved}
    <button
      bind:this={buttons[p.id]}
      animate:makeWay={{ held }}
      class="provider"
      class:on={p.on}
      class:held
      style:transform={held ? `translateX(${drag?.dx ?? 0}px)` : null}
      aria-pressed={p.on}
      title={p.on
        ? `On: VODscribe asks ${about?.host} for each Kick or Twitch channel's emotes. Click to turn off; drag to reorder.`
        : `Off. Turning it on lets VODscribe contact ${about?.host} for each Kick or Twitch channel's emotes and their images.`}
      onpointerdown={(e) => onDown(e, p.id)}
      onpointermove={onMove}
      onpointerup={onUp}
      onpointercancel={onUp}
      onclick={() => onClick(p.id)}
      onkeydown={(e) => onKey(e, p.id)}
    >
      {#if about?.logo}
        <img class="logo" src={about.logo} alt="" draggable="false" />
      {:else if p.id === "7tv"}
        <svg class="logo" viewBox="8 24 109 80" aria-hidden="true"
          ><path
            d="M90.986 45.953L95.9196 37.3498L98.584 32.801L93.65 24.1978V24H67.3036L77.1712 41.2064L79.9344 45.953H90.986Z"
          /><path
            d="M36.6158 103.703L66.2184 52.0842L69.8692 45.7554L60.0016 28.549L57.2388 24.099H15.598L10.6642 32.7022L8 37.251L12.9338 45.8542V46.052H44.5098L19.841 89.068L16.3874 95.1992L21.3212 103.802V104H36.6158"
          /><path
            d="M77.862 103.703H92.9592L112.694 69.29L116.148 63.357L111.214 54.7538V54.556H96.0184L86.1508 71.7624L85.46 73.048L75.5924 55.8415L74.902 54.556L65.0344 71.7624L62.2716 76.5088L77.0728 102.318L77.862 103.703Z"
          /></svg
        >
      {/if}
      {about?.name ?? p.id}
    </button>
  {/each}
</div>
<p class="muted hint">
  {#if anyOn}
    <Icon name="external" size={12} /> VODscribe contacts the providers turned on for each Kick or Twitch channel's emotes,
    once a day. When two have an emote of the same name, the one further left wins. Drag to reorder.
  {:else}
    Off. Turning a provider on lets VODscribe contact it for the emotes Kick and Twitch chat types as words, and show
    them as images.
  {/if}
</p>
{#if settings.error}<p class="error-text hint">{settings.error}</p>{/if}

<style>
  .row {
    /* The buttons' places are measured from here while dragging. */
    position: relative;
    display: flex;
    gap: 6px;
  }

  .provider {
    position: relative;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 9px 4px 6px;
    font-size: 12px;
    white-space: nowrap;
    cursor: grab;
    touch-action: none;
    user-select: none;
    color: var(--muted);
    /* Let go, a dragged button glides into its place. */
    transition: transform 150ms ease;
  }

  .provider.on {
    color: var(--text);
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  /* Being dragged: above the others, lifted, and right under the pointer. */
  .provider.held {
    z-index: 1;
    cursor: grabbing;
    box-shadow: var(--shadow);
    transition: none;
  }

  .logo {
    width: 14px;
    height: 14px;
    flex: none;
    object-fit: contain;
    fill: currentColor;
  }

  /* Off: the logo grayed out, so on and off read at a glance. */
  .provider:not(.on) .logo {
    filter: grayscale(1);
    opacity: 0.55;
  }

  .hint {
    margin: 6px 0 0;
    font-size: 12px;
  }
</style>
