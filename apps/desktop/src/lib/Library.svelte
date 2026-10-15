<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, tick } from "svelte";
  import { flip } from "svelte/animate";
  import { api, type Vod } from "./api";
  import { os } from "./platform";
  import { DEMO, demoOff } from "./demo";
  import Icon from "./Icon.svelte";
  import { AUTHOR, SITE, SUPPORT, releaseNotes } from "./links";
  import Logo from "./Logo.svelte";
  import { Adder } from "./adder.svelte";
  import Spinner from "./Spinner.svelte";
  import { Reorder } from "./reorder.svelte";
  import UpdateBanner from "./UpdateBanner.svelte";
  import VodRow from "./VodRow.svelte";

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
  // Adding links and files, one after another (adder.svelte.ts). Its error is
  // also where a row's failed action shows, under the form.
  const adder = new Adder();

  async function add(event: SubmitEvent) {
    event.preventDefault();
    if (await adder.link(url)) url = "";
  }

  async function pickFiles() {
    const extensions = await api.mediaExtensions().catch(() => []);
    const picked = await open({
      multiple: true,
      directory: false,
      // No list (it failed to load) means no filter: an empty one would hide every file.
      filters: extensions.length ? [{ name: "Video and audio", extensions }] : [],
    }).catch(() => null);
    if (picked) adder.items(Array.isArray(picked) ? picked : [picked], true);
  }

  // Dropping onto the window. Files arrive through the webview's drop event with
  // their paths, and on Linux dropped links do too. Windows and macOS don't pass
  // link drops to the app at all, so there links have to be pasted.
  const linksDrop = os === "linux";
  // A link dragged over the page (DOM events) or files over the window (the
  // webview's own events); either shows the drop overlay.
  let linkOver = $state(false);
  let filesOver = $state(false);
  let dropping = $derived(linkOver || filesOver);
  // dragover repeats every few hundred milliseconds while a drag is over the
  // page; when it stops, the drag left or ended somewhere the page doesn't hear
  // of (outside the window), so the overlay goes.
  let linkGone: ReturnType<typeof setTimeout> | undefined;
  // While anything is dragged in, embedded players (separate pages in iframes)
  // ignore the pointer, so a drop lands here rather than navigating one of them.
  // That lasts until the link is dropped or leaves the window, not just while
  // dragover repeats: over a player, it may not reach the page in time. The
  // pointer moving again means the drag is over.
  let linkDragged = $state(false);
  $effect(() => {
    document.body.classList.toggle("dropping", linkDragged || filesOver);
  });
  // A drag that started inside the app (transcript text, a thumbnail) isn't a drop to add.
  let draggingInside = false;
  // Taken from dragenter on too: a quick drag can be dropped before any
  // dragover, and a drop no handler has claimed opens the link in the window.
  function onDragOver(e: DragEvent) {
    // The demo's page is a website: nothing can be dropped on it.
    if (DEMO) return;
    const types = e.dataTransfer?.types ?? [];
    if (draggingInside || (!types.includes("text/uri-list") && !types.includes("text/plain"))) return;
    e.preventDefault();
    linkOver = linkDragged = true;
    clearTimeout(linkGone);
    linkGone = setTimeout(() => (linkOver = false), 400);
  }
  function onDragLeave(e: DragEvent) {
    // dragleave fires for every element crossed; only leaving the window counts.
    if (e.clientX <= 0 || e.clientY <= 0 || e.clientX >= innerWidth || e.clientY >= innerHeight) {
      linkOver = linkDragged = false;
    }
  }
  function onDrop(e: DragEvent) {
    if (DEMO) return;
    linkOver = linkDragged = false;
    clearTimeout(linkGone);
    const data = e.dataTransfer;
    if (!data || draggingInside) return;
    // Whatever was dropped, the webview doesn't open it in place of the app.
    e.preventDefault();
    const text = data.getData("text/uri-list") || data.getData("text/plain");
    if (!text) return;
    // uri-list lines starting with # are comments.
    // Text drops only ever add links; file paths come from real file drops.
    adder.items(
      text.split(/\r?\n/).filter((l) => !l.startsWith("#")),
      false,
    );
  }

  let version = $state<string | null>(null);
  onMount(() => {
    getVersion().then(
      (v) => (version = v || null),
      () => {},
    );
    let stop: (() => void) | undefined;
    let unmounted = false;
    getCurrentWebview()
      .onDragDropEvent(({ payload }) => {
        if (payload.type === "enter") {
          // macOS reports every drag, even ones with nothing the app can use.
          filesOver = payload.paths.length > 0 && !draggingInside;
        } else if (payload.type === "leave") filesOver = false;
        else if (payload.type === "drop") {
          filesOver = false;
          if (!draggingInside) adder.items(payload.paths, true);
        }
      })
      .then(
        (u) => (unmounted ? u() : (stop = u)),
        () => {},
      );
    return () => {
      unmounted = true;
      clearTimeout(linkGone);
      document.body.classList.remove("dropping");
      stop?.();
    };
  });

  const toggle = (vod: Vod) => onselect(vod.id === selectedId ? null : vod.id);

  // The whole row selects, except its action buttons (and the main button,
  // which handles itself so it also works from the keyboard).
  function onRowClick(e: MouseEvent, vod: Vod) {
    if ((e.target as HTMLElement).closest("button")) return;
    toggle(vod);
  }

  // Dragging a row (anywhere but its action buttons) or Alt+↑/↓ reorders the library.
  const reorder = new Reorder(
    () => vods.map((v) => v.id),
    (ids) => onreorder(ids),
  );
  let byId = $derived(new Map(vods.map((v) => [v.id, v])));
  let shown = $derived(reorder.order.map((id) => byId.get(id)).filter((v) => v !== undefined));

  function onRowKey(e: KeyboardEvent, vod: Vod) {
    if (reorder.key(e, vod.id)) tick().then(() => reorder.rows[vod.id]?.querySelector<HTMLElement>(".main")?.focus());
  }
</script>

<svelte:window
  ondragstart={() => (draggingInside = true)}
  ondragend={() => (draggingInside = false)}
  ondragenter={onDragOver}
  ondragover={onDragOver}
  ondrop={onDrop}
  ondragleave={onDragLeave}
  onpointermove={() => (linkDragged = false)}
/>

{#if dropping}
  <div class="drop">{linksDrop ? "Drop video files or VOD links to add them" : "Drop video files to add them"}</div>
{/if}

<aside>
  <!-- Sibling buttons, since buttons can't nest: the site link fills the
       header, the credit sits on top of it under the name, and the version
       number at the right, linking to what's new in it. -->
  <div class="brand">
    <button class="site" onclick={() => openUrl(SITE)} title="vodscribe.com">
      <Logo size={28} tile />
      <span class="name">VODscribe</span>
      <span class="external"><Icon name="external" size={12} /></span>
    </button>
    {#if version}
      <button
        class="version"
        onclick={() => openUrl(releaseNotes(version!))}
        title="What's new in {version.split(/[_+-]/)[0]}">v{version}</button
      >
    {/if}
    <button class="credit" onclick={() => openUrl(SUPPORT)} title="Support VODscribe on Ko-fi">
      Made by {AUTHOR}
    </button>
  </div>

  <UpdateBanner />

  <form onsubmit={add}>
    <input
      type="url"
      placeholder="Paste a VOD link…"
      bind:value={url}
      disabled={adder.adding}
      readonly={DEMO}
      aria-label="VOD link"
      {...demoOff(
        "The web demo can't transcribe. In the app, paste a Kick, Twitch or YouTube link here and it's transcribed on your computer.",
      )}
    />
    <button
      class="primary"
      disabled={adder.adding || (!url.trim() && !DEMO)}
      {...demoOff(
        "The web demo can't transcribe. In the app, paste a Kick, Twitch or YouTube link here and it's transcribed on your computer.",
      )}>{adder.adding ? "Adding…" : "Add"}</button
    >
  </form>
  <p class="more muted">
    or <button
      class="link"
      onclick={pickFiles}
      disabled={adder.adding}
      {...demoOff("The web demo can't transcribe. In the app, you can add video files from your computer too.")}
      >add video files…</button
    >
    {#if !DEMO}or drop {linksDrop ? "files and links" : "video files"} anywhere{/if}
  </p>
  {#if adder.error}
    <p class="error-text add-error">{adder.error}</p>
  {/if}

  <div class="list" bind:this={reorder.list}>
    <!-- Links and files being added: each stands in until its VOD arrives (new ones go on top). -->
    {#each adder.pending as p (p.key)}
      <div class="vod pending" role="status" aria-label="Adding {p.label}">
        <span class="standin-thumb"><Spinner size={16} /></span>
        <span class="standin-text">
          <span class="standin-title">{p.label}</span>
          <span class="muted standin-meta">Adding…</span>
        </span>
        <button class="cancel" onclick={() => adder.cancel(p.key)} title="Cancel" aria-label="Cancel adding {p.label}">
          <Icon name="x" size={15} />
        </button>
      </div>
    {/each}
    {#each shown as vod (vod.id)}
      <!-- svelte-ignore a11y_no_static_element_interactions, a11y_click_events_have_key_events -->
      <div
        class="vod"
        class:selected={vod.id === selectedId}
        class:lifted={reorder.lifted(vod.id)}
        style:transform={reorder.transform(vod.id)}
        bind:this={reorder.rows[vod.id]}
        animate:flip={{ duration: reorder.flipDuration(vod.id) }}
        onclickcapture={reorder.clickCapture}
        onclick={(e) => onRowClick(e, vod)}
        onpointerdown={(e) => {
          if (!(e.target as HTMLElement).closest(".actions")) reorder.start(e, vod.id);
        }}
      >
        <VodRow
          {vod}
          reorderable={vods.length > 1}
          ontoggle={() => toggle(vod)}
          onkey={(e) => onRowKey(e, vod)}
          onremove={() => selectedId === vod.id && onselect(null)}
          onerror={(e) => (adder.error = e)}
        />
      </div>
    {:else}
      {#if adder.pending.length === 0}
        <p class="muted empty">No VODs yet. Paste a link above to transcribe one.</p>
      {/if}
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

  .external {
    display: inline-flex;
    margin-left: auto;
  }

  /* Over the site link, left of its icon: what's new in this version. */
  .version {
    position: absolute;
    right: 34px;
    top: 50%;
    transform: translateY(-50%);
    padding: 0 2px;
    border: none;
    background: none;
    color: var(--muted);
    font-size: 12px;
    font-weight: 400;
    font-variant-numeric: tabular-nums;
  }

  .version:hover:not(:disabled) {
    color: var(--accent);
    text-decoration: underline;
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
    color: var(--accent);
    text-decoration: underline;
  }

  form {
    display: flex;
    gap: 6px;
    padding: 10px 12px 4px;
  }

  .more {
    margin: 0;
    padding: 0 12px 9px;
    font-size: 12px;
    border-bottom: 1px solid var(--border);
  }

  .more .link {
    color: var(--accent);
  }

  .drop {
    position: fixed;
    inset: 8px;
    z-index: 50;
    display: flex;
    align-items: center;
    justify-content: center;
    border: 2px dashed var(--accent);
    border-radius: 12px;
    background: color-mix(in srgb, var(--accent-soft) 85%, transparent);
    color: var(--text);
    font-size: 15px;
    font-weight: 600;
    pointer-events: none;
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
    /* Rows' offsetTop is measured from here; see Reorder. */
    position: relative;
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

  /* A stand-in for something being added: a row's shape, a spinner for its thumbnail. */
  .vod.pending {
    display: flex;
    gap: 10px;
    cursor: default;
  }

  .vod.pending:hover {
    background: var(--surface);
  }

  .standin-thumb {
    flex: none;
    width: 80px;
    aspect-ratio: 16 / 9;
    border-radius: var(--radius-sm);
    background: var(--surface-3);
    color: var(--muted);
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .standin-text {
    display: flex;
    flex-direction: column;
    gap: 2px;
    min-width: 0;
  }

  .standin-title {
    font-weight: 600;
    font-size: 13px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .standin-meta {
    font-size: 12px;
  }

  /* Like a row's own action buttons (VodRow.svelte). */
  .vod.pending .cancel {
    margin-left: auto;
    align-self: center;
    flex: none;
    width: 26px;
    height: 24px;
    padding: 0;
    border: none;
    background: none;
    color: var(--muted);
    border-radius: var(--radius-sm);
  }

  .vod.pending .cancel:hover {
    color: var(--text);
    background: var(--surface-2);
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
    box-shadow:
      inset 3px 0 var(--accent),
      var(--shadow);
  }

  :global(body.dragging),
  :global(body.dragging *) {
    cursor: grabbing !important;
  }

  .empty {
    padding: 16px 12px;
  }
</style>
