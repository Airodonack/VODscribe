<script lang="ts">
  import { flip } from "svelte/animate";
  import type { EmoteProvider } from "./api";
  import Icon from "./Icon.svelte";
  import { settings } from "./settings.svelte";

  // The emote providers chat's word emotes come from (crates/core
  // `word_emotes`), in one row: click one to turn it on or off, drag it (or
  // Alt+←/→) to change which comes first when two have an emote of the same
  // name. All start off: each one turned on is a service contacted.

  const ABOUT: Record<string, { name: string; host: string }> = {
    "7tv": { name: "7TV", host: "7tv.io" },
    bttv: { name: "BetterTTV", host: "betterttv.net" },
    ffz: { name: "FrankerFaceZ", host: "frankerfacez.com" },
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
  // The row rearranges live as the pointer passes the other buttons' middles.
  const THRESHOLD = 4;
  let buttons: Record<string, HTMLElement | null> = {};
  let drag = $state<{ id: string; x: number; moved: boolean } | null>(null);
  // The click after a drag isn't a toggle.
  let dragged = false;

  function onDown(e: PointerEvent, id: string) {
    if (e.button !== 0) return;
    drag = { id, x: e.clientX, moved: false };
    (e.currentTarget as HTMLElement).setPointerCapture(e.pointerId);
  }

  function onMove(e: PointerEvent) {
    if (!drag) return;
    if (!drag.moved && Math.abs(e.clientX - drag.x) < THRESHOLD) return;
    drag.moved = true;
    const others = order.filter((p) => p.id !== drag!.id);
    // Where it goes: after every other button whose middle the pointer has passed.
    const at = others.filter((p) => {
      const box = buttons[p.id]?.getBoundingClientRect();
      return box && e.clientX > box.left + box.width / 2;
    }).length;
    const current = order.findIndex((p) => p.id === drag!.id);
    if (current !== at) {
      const next = [...others];
      next.splice(at, 0, order[current]);
      order = next;
    }
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

  let anyOn = $derived(order.some((p) => p.on));
</script>

<div class="row" role="group" aria-label="Chat emote providers, first one wins">
  {#each order as p (p.id)}
    {@const about = ABOUT[p.id]}
    <button
      bind:this={buttons[p.id]}
      animate:flip={{ duration: 150 }}
      class="provider"
      class:on={p.on}
      class:dragging={drag?.id === p.id && drag.moved}
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
      <span class="check" aria-hidden="true"
        >{#if p.on}<Icon name="check" size={11} />{/if}</span
      >
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
    display: flex;
    gap: 6px;
  }

  .provider {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    padding: 4px 9px 4px 6px;
    font-size: 12px;
    white-space: nowrap;
    cursor: grab;
    touch-action: none;
    user-select: none;
  }

  .provider.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .provider.dragging {
    cursor: grabbing;
    box-shadow: var(--shadow);
  }

  .check {
    display: inline-flex;
    align-items: center;
    justify-content: center;
    width: 13px;
    height: 13px;
    border: 1px solid var(--border);
    border-radius: 3px;
    background: var(--surface);
    color: var(--accent);
  }

  .on .check {
    border-color: var(--accent);
  }

  .hint {
    margin: 6px 0 0;
    font-size: 12px;
  }
</style>
