<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, tick } from "svelte";
  import { flip } from "svelte/animate";
  import { api, type Vod } from "./api";
  import Icon from "./Icon.svelte";
  import { AUTHOR, SITE, SUPPORT } from "./links";
  import Logo from "./Logo.svelte";
  import Thumb from "./Thumb.svelte";
  import { formatTime, platformName } from "./platform";

  let {
    vods,
    selectedId,
    onselect,
    onreorder,
  }: {
    vods: Vod[];
    selectedId: number | null;
    onselect: (id: number | null) => void;
    onreorder: (ids: number[]) => void;
  } = $props();

  let url = $state("");
  let adding = $state(false);
  // The last failed action (adding a link, or a row's button), shown under the form.
  let error = $state<string | null>(null);

  async function add(event: SubmitEvent) {
    event.preventDefault();
    adding = true;
    error = null;
    try {
      await api.addVod(url);
      url = "";
    } catch (e) {
      error = String(e);
    } finally {
      adding = false;
    }
  }

  /** Runs a row action, showing its error if it fails. */
  function act(action: Promise<unknown>) {
    error = null;
    action.catch((e) => (error = String(e)));
  }

  const statusLabel: Record<Vod["status"], string> = {
    queued: "Waiting to download",
    downloading: "Downloading",
    ready: "Waiting to transcribe",
    transcribing: "Transcribing",
    paused: "Paused",
    done: "",
    error: "Failed",
  };

  function retranscribe(vod: Vod) {
    if (confirm(`Re-transcribe “${vod.title}”? Its current transcript is replaced; the audio may need to download again.`)) {
      act(api.retranscribeVod(vod.id));
    }
  }

  function cancel(vod: Vod) {
    if (confirm(`Cancel “${vod.title}”? Its partial download and transcript will be discarded.`)) {
      act(api.cancelVod(vod.id));
    }
  }

  function remove(vod: Vod) {
    if (confirm(`Remove “${vod.title}” and its transcript?`)) {
      if (selectedId === vod.id) onselect(null);
      act(api.deleteVod(vod.id));
    }
  }


  let version = $state<string | null>(null);
  onMount(() => {
    getVersion().then((v) => (version = v || null), () => {});
  });

  const toggle = (vod: Vod) => onselect(vod.id === selectedId ? null : vod.id);

  // The whole row selects, except its action buttons (and the main button,
  // which handles itself so it also works from the keyboard).
  function onRowClick(e: MouseEvent, vod: Vod) {
    if ((e.target as HTMLElement).closest("button")) return;
    toggle(vod);
  }

  // Dragging a row (anywhere but its action buttons) reorders the library.
  // The list reorders live as the row passes its neighbours' midpoints; the
  // dragged row follows the pointer. Layout positions come from offsetTop,
  // which ignores the transforms of rows that are still animating.
  let list: HTMLElement;
  const rows: Record<number, HTMLElement> = {};
  let drag = $state<{ id: number; order: number[]; moved: boolean; dy: number } | null>(null);
  let byId = $derived(new Map(vods.map((v) => [v.id, v])));
  let shown = $derived(drag?.moved ? drag.order.map((id) => byId.get(id)).filter((v) => v !== undefined) : vods);
  let suppressClick = false;

  /** `order` minus VODs removed since the drag began, plus any added (on top, as in the library). */
  function reconcile(order: number[]) {
    const kept = order.filter((id) => byId.has(id));
    return [...vods.map((v) => v.id).filter((id) => !kept.includes(id)), ...kept];
  }

  function startDrag(e: PointerEvent, vod: Vod) {
    if (e.button !== 0 || vods.length < 2 || (e.target as HTMLElement).closest(".actions")) return;
    const startY = e.clientY;
    const grab = e.clientY - rows[vod.id].getBoundingClientRect().top;
    let pointerY = startY;
    let frame = 0;
    drag = { id: vod.id, order: vods.map((v) => v.id), moved: false, dy: 0 };

    // Where the pointer is, in the list's content coordinates.
    const contentY = () => pointerY - list.getBoundingClientRect().top + list.scrollTop;

    function step() {
      if (!drag) return;
      // VODs can be added or removed mid-drag: new ones go on top, as they do in the library.
      const order = reconcile(drag.order);
      if (order.length !== drag.order.length || order.some((id, i) => id !== drag!.order[i])) drag.order = order;
      const self = rows[drag.id];
      if (!byId.has(drag.id) || !self) return end();

      const box = list.getBoundingClientRect();
      const edge = 36;
      if (pointerY < box.top + edge) list.scrollTop -= Math.ceil((box.top + edge - pointerY) / 4);
      else if (pointerY > box.bottom - edge) list.scrollTop += Math.ceil((pointerY - box.bottom + edge) / 4);

      const y = contentY();
      const i = drag.order.indexOf(drag.id);
      // Rows that haven't rendered yet have no midpoint, so they're never swapped with.
      const mid = (id: number) => (rows[id] ? rows[id].offsetTop + rows[id].offsetHeight / 2 : NaN);
      const top = y - grab;
      const bottom = top + self.offsetHeight;
      const o = drag.order;
      if (i > 0 && top < mid(o[i - 1])) {
        [o[i - 1], o[i]] = [o[i], o[i - 1]];
      } else if (i < o.length - 1 && bottom > mid(o[i + 1])) {
        [o[i + 1], o[i]] = [o[i], o[i + 1]];
      }
      tick().then(() => {
        const el = drag && rows[drag.id];
        if (drag && el) drag.dy = y - grab - el.offsetTop;
      });
      frame = requestAnimationFrame(step);
    }

    function move(e: PointerEvent) {
      pointerY = e.clientY;
      if (drag && !drag.moved && Math.abs(pointerY - startY) > 4) {
        drag.moved = true;
        document.body.classList.add("dragging");
        frame = requestAnimationFrame(step);
      }
    }

    function end() {
      cancelAnimationFrame(frame);
      window.removeEventListener("pointermove", move);
      window.removeEventListener("pointerup", end);
      window.removeEventListener("pointercancel", end);
      document.body.classList.remove("dragging");
      if (drag?.moved) {
        // The click that ends a drag shouldn't select the row.
        suppressClick = true;
        setTimeout(() => (suppressClick = false));
        const order = reconcile(drag.order);
        if (order.some((id, i) => id !== vods[i]?.id)) onreorder(order);
      }
      drag = null;
    }

    window.addEventListener("pointermove", move);
    window.addEventListener("pointerup", end);
    window.addEventListener("pointercancel", end);
  }

  // Alt+↑/↓ on a row moves it, for the keyboard.
  function onRowKey(e: KeyboardEvent, vod: Vod) {
    if (!e.altKey || (e.key !== "ArrowUp" && e.key !== "ArrowDown")) return;
    e.preventDefault();
    const order = vods.map((v) => v.id);
    const i = order.indexOf(vod.id);
    const j = e.key === "ArrowUp" ? i - 1 : i + 1;
    if (j < 0 || j >= order.length) return;
    [order[i], order[j]] = [order[j], order[i]];
    onreorder(order);
    tick().then(() => rows[vod.id]?.querySelector<HTMLElement>(".main")?.focus());
  }
</script>

<aside>
  <!-- Two sibling buttons, since buttons can't nest: the site link fills the
       header and the credit sits on top of it, under the name. -->
  <div class="brand">
    <button class="site" onclick={() => openUrl(SITE)} title="vodscribe.com">
      <Logo size={28} tile />
      <span class="name">VODscribe</span>
      {#if version}<span class="version muted">v{version}</span>{/if}
      <Icon name="external" size={12} />
    </button>
    <button class="credit" onclick={() => openUrl(SUPPORT)} title="Support VODscribe on Ko-fi">
      Made by {AUTHOR}
    </button>
  </div>

  <form onsubmit={add}>
    <input type="url" placeholder="Paste a VOD link…" bind:value={url} disabled={adding} aria-label="VOD link" />
    <button class="primary" disabled={adding || !url.trim()}>{adding ? "Adding…" : "Add"}</button>
  </form>
  {#if error}
    <p class="error-text add-error">{error}</p>
  {/if}

  <div class="list" bind:this={list}>
    {#each shown as vod (vod.id)}
      {@const active = vod.status === "downloading" || vod.status === "transcribing"}
      {@const showBar = active || (vod.status === "paused" && vod.progress > 0)}
      {@const platform = platformName[vod.platform] ?? vod.platform}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div
        class="vod"
        class:selected={vod.id === selectedId}
        class:lifted={drag?.moved && drag.id === vod.id}
        style:transform={drag?.moved && drag.id === vod.id ? `translateY(${drag.dy}px)` : null}
        bind:this={rows[vod.id]}
        animate:flip={{ duration: drag?.id === vod.id ? 0 : 150 }}
        onclickcapture={(e) => {
          if (suppressClick) {
            e.stopPropagation();
            e.preventDefault();
          }
        }}
        onclick={(e) => onRowClick(e, vod)}
        onpointerdown={(e) => startDrag(e, vod)}
      >
        <button
          class="main"
          onclick={() => toggle(vod)}
          onkeydown={(e) => onRowKey(e, vod)}
          title={vods.length > 1 ? "Drag to reorder (Alt+↑/↓)" : undefined}
        >
          <span class="thumb {vod.platform}">
            <!-- The platform tile shows until (and unless) the thumbnail covers it. -->
            <Icon name="play" size={14} />
            {#if vod.thumb}<Thumb name={vod.thumb} />{/if}
            {#if vod.duration}<span class="duration">{formatTime(vod.duration)}</span>{/if}
          </span>
          <span class="text">
            <span class="title">{vod.title}</span>
            <span class="meta muted">
              <span class="platform {vod.platform}">{platform}</span>
              {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
            </span>
          </span>
        </button>

        {#if vod.status !== "done" || showBar || vod.error || vod.outdated || vod.note}
          <div class="status-area">
            {#if vod.status !== "done"}
              <span class="status" class:active class:error-text={vod.status === "error"}>
                {#if active}<span class="pulse" aria-hidden="true"></span>{/if}
                {#if vod.status === "error"}<Icon name="error" size={13} />{/if}
                {statusLabel[vod.status]}{#if showBar}<span class="pct">{Math.round(vod.progress * 100)}%</span>{/if}
              </span>
            {/if}
            {#if showBar}
              <span class="bar" class:paused={vod.status === "paused"}><span style:width="{vod.progress * 100}%"></span></span>
            {/if}
            {#if vod.status === "error" && vod.error}
              <span class="note error-text" title={vod.error}>{vod.error}</span>
            {:else if vod.outdated}
              <span class="note warn-text" title="This transcript was made with a previous speech model.">
                <Icon name="warning" size={13} />
                {vod.status === "done" ? "Older model; may be missing speech" : "Older model; resuming starts over"}
              </span>
            {:else if vod.note}
              <span class="note muted" title={vod.note}>{vod.note}</span>
            {/if}
          </div>
        {/if}

        <div class="actions">
          {#if vod.status === "error"}
            <button class="act" onclick={() => act(api.retryVod(vod.id))} title="Retry" aria-label="Retry">
              <Icon name="redo" size={15} />
            </button>
          {:else if vod.status === "paused"}
            <!-- Resuming an outdated transcript starts over, like Re-transcribe. -->
            <button
              class="act"
              class:warn={vod.outdated}
              onclick={() => act(api.resumeVod(vod.id))}
              title={vod.outdated ? "Resume (starts over with the current model)" : "Resume"}
              aria-label="Resume"
            >
              <Icon name="play" size={15} />
            </button>
            <button class="act" onclick={() => cancel(vod)} title="Cancel" aria-label="Cancel">
              <Icon name="x" size={15} />
            </button>
          {:else if vod.status !== "done"}
            <button class="act" onclick={() => act(api.pauseVod(vod.id))} title="Pause" aria-label="Pause">
              <Icon name="pause" size={15} />
            </button>
            <button class="act" onclick={() => cancel(vod)} title="Cancel" aria-label="Cancel">
              <Icon name="x" size={15} />
            </button>
          {:else}
            <button
              class="act"
              class:warn={vod.outdated}
              onclick={() => retranscribe(vod)}
              title={vod.outdated ? "Re-transcribe with the current model" : "Re-transcribe"}
              aria-label="Re-transcribe"
            >
              <Icon name="redo" size={15} />
            </button>
          {/if}
          <button class="act" onclick={() => remove(vod)} title="Remove" aria-label="Remove">
            <Icon name="trash" size={15} />
          </button>
        </div>
      </div>
    {:else}
      <p class="muted empty">No VODs yet. Paste a link above to transcribe one.</p>
    {/each}
  </div>
</aside>

<style>
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }

  .brand {
    position: relative;
    flex: none;
    height: 53px;
    border-bottom: 1px solid var(--border);
  }

  .site {
    display: flex;
    align-items: center;
    justify-content: flex-start;
    gap: 6px;
    width: 100%;
    height: 100%;
    padding: 0 14px;
    border: none;
    border-radius: 0;
    background: var(--surface);
    color: var(--muted);
  }

  .site:hover:not(:disabled) {
    background: var(--bg);
    color: var(--text);
  }

  /* The name sits in the top half of the header, the credit below it. */
  .name {
    margin: 0 0 15px 4px;
    color: var(--text);
    font-weight: 700;
    font-size: 15px;
    letter-spacing: -0.01em;
  }

  .version {
    margin-left: auto;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: inherit;
  }

  .credit {
    position: absolute;
    left: 50px;
    bottom: 7px;
    padding: 0 2px;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 11px;
    font-weight: 400;
    line-height: 16px;
  }

  .credit:hover:not(:disabled) {
    background: none;
    color: var(--accent);
    text-decoration: underline;
  }


  form {
    display: flex;
    gap: 6px;
    padding: 10px 12px;
    border-bottom: 1px solid var(--border);
  }

  .add-error {
    margin: 0;
    padding: 8px 12px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
    white-space: pre-wrap;
  }

  .list {
    overflow-y: auto;
    flex: 1;
  }

  .vod {
    position: relative;
    cursor: pointer;
    user-select: none;
    -webkit-user-select: none;
    background: var(--surface);
    padding: 10px 12px 9px;
    border-bottom: 1px solid var(--border);
    transition: background 0.1s;
  }

  .vod:hover {
    background: var(--bg);
  }

  .vod.selected {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 var(--accent);
  }

  /* Opaque so the rows it passes over don't show through. */
  .vod.selected.lifted {
    background: color-mix(in srgb, var(--accent) 12%, var(--surface));
  }

  .vod.lifted {
    z-index: 1;
    cursor: grabbing;
    transition: none;
    box-shadow: var(--shadow);
  }

  .vod.selected.lifted {
    box-shadow: inset 3px 0 var(--accent), var(--shadow);
  }

  :global(body.dragging),
  :global(body.dragging *) {
    cursor: grabbing !important;
  }

  .main {
    display: flex;
    gap: 10px;
    width: 100%;
    text-align: left;
    border: none;
    background: none;
    padding: 0;
    white-space: normal;
    align-items: flex-start;
  }

  .main:hover:not(:disabled) {
    background: none;
  }

  .thumb {
    position: relative;
    flex: none;
    width: 80px;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius-sm);
    background: var(--surface-3);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
    overflow: hidden;
  }

  .thumb.youtube {
    background: color-mix(in srgb, var(--youtube) 18%, var(--surface-3));
  }

  .thumb.twitch {
    background: color-mix(in srgb, var(--twitch) 22%, var(--surface-3));
  }

  .thumb.kick {
    background: color-mix(in srgb, var(--kick) 20%, var(--surface-3));
  }


  .duration {
    position: absolute;
    right: 3px;
    bottom: 3px;
    padding: 0 4px;
    border-radius: 3px;
    background: rgb(0 0 0 / 0.75);
    color: #fff;
    font-family: var(--mono);
    font-size: 10px;
    line-height: 16px;
    font-variant-numeric: tabular-nums;
  }

  .text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
    flex: 1;
  }

  .title {
    font-weight: 600;
    font-size: 13px;
    line-height: 1.35;
    display: -webkit-box;
    -webkit-line-clamp: 2;
    line-clamp: 2;
    -webkit-box-orient: vertical;
    overflow: hidden;
  }

  .meta {
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .sep {
    margin: 0 4px;
  }

  .status-area {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-top: 7px;
    font-size: 12px;
  }

  .status {
    display: flex;
    align-items: center;
    gap: 6px;
  }

  .pct {
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  .pulse {
    width: 7px;
    height: 7px;
    border-radius: 50%;
    background: var(--accent);
    animation: pulse 1.6s ease-in-out infinite;
  }

  @keyframes pulse {
    50% {
      opacity: 0.35;
    }
  }

  .note {
    display: flex;
    align-items: center;
    gap: 5px;
    min-width: 0;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .note :global(svg) {
    flex: none;
  }

  .bar {
    display: block;
    height: 3px;
    border-radius: 2px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .bar.paused > span {
    background: var(--muted);
  }

  .bar > span {
    display: block;
    height: 100%;
    background: var(--accent);
    transition: width 0.25s;
  }

  .actions {
    display: flex;
    gap: 2px;
    justify-content: flex-end;
    margin-top: 4px;
    margin-right: -6px;
  }

  .act {
    width: 26px;
    height: 24px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    border-radius: var(--radius-sm);
    opacity: 0.75;
  }

  .vod:hover .act,
  .vod.selected .act {
    opacity: 1;
  }

  .act:hover:not(:disabled) {
    background: var(--surface-3);
    color: var(--text);
  }

  .act.warn {
    color: var(--warn);
  }

  .empty {
    padding: 16px 12px;
  }
</style>
