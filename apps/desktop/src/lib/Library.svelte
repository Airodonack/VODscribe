<script lang="ts">
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, tick } from "svelte";
  import { flip } from "svelte/animate";
  import { api, type Vod } from "./api";
  import Icon from "./Icon.svelte";
  import { AUTHOR, SITE, SUPPORT } from "./links";
  import Logo from "./Logo.svelte";
  import { Reorder } from "./reorder.svelte";
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

  /**
   * Adds what was dropped or picked. Links go through the same check as a pasted
   * link (yt-dlp's probe); files, only accepted from a real file drop or the file
   * picker (`allowFiles`), are probed with ffmpeg. Adds queue up, and the errors
   * of everything added together are shown together once the queue is done.
   */
  let queue: Promise<void> = Promise.resolve();
  let queued = 0;
  let queuedErrors: string[] = [];
  function addItems(items: string[], allowFiles: boolean) {
    const links: string[] = [];
    const files: string[] = [];
    const errors: string[] = [];
    for (const item of items.map((i) => i.trim()).filter(Boolean)) {
      const link = item.match(/https?:\/\/\S+/i)?.[0];
      if (link) links.push(link);
      else if (allowFiles) files.push(item);
      else errors.push(`“${item.length > 80 ? item.slice(0, 80) + "…" : item}” isn't a link.`);
    }
    if (!links.length && !files.length && !errors.length) return;
    if (queued++ === 0) queuedErrors = [];
    queuedErrors.push(...errors);
    adding = true;
    error = null;
    queue = queue
      .then(async () => {
        for (const link of links) await api.addVod(link).catch((e) => queuedErrors.push(String(e)));
        if (files.length) {
          await api.addFiles(files).then(
            (r) => queuedErrors.push(...r.errors),
            (e) => queuedErrors.push(String(e)),
          );
        }
      })
      .finally(() => {
        if (--queued > 0) return;
        adding = false;
        error = queuedErrors.length ? queuedErrors.join("\n") : null;
      });
  }

  const MEDIA = [
    "mp4",
    "mkv",
    "webm",
    "mov",
    "avi",
    "m4v",
    "ts",
    "flv",
    "mp3",
    "m4a",
    "wav",
    "flac",
    "ogg",
    "opus",
    "aac",
  ];
  async function pickFiles() {
    const picked = await open({
      multiple: true,
      directory: false,
      filters: [{ name: "Video and audio", extensions: MEDIA }],
    }).catch(() => null);
    if (picked) addItems(Array.isArray(picked) ? picked : [picked], true);
  }

  // Dropping onto the window. Files (and, on Linux, links) arrive through the
  // webview's drop event with their paths; links on other platforms arrive as
  // an ordinary HTML drop of text.
  let dropping = $state(false);
  function onDragOver(e: DragEvent) {
    const types = e.dataTransfer?.types ?? [];
    if (!types.includes("text/uri-list") && !types.includes("text/plain")) return;
    e.preventDefault();
    dropping = true;
  }
  function onDrop(e: DragEvent) {
    const data = e.dataTransfer;
    if (!data) return;
    const text = data.getData("text/uri-list") || data.getData("text/plain");
    if (!text) return;
    e.preventDefault();
    dropping = false;
    // uri-list lines starting with # are comments.
    // Text drops only ever add links; file paths come from real file drops.
    addItems(
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
        if (payload.type === "enter" || payload.type === "over") dropping = true;
        else if (payload.type === "leave") dropping = false;
        else {
          dropping = false;
          addItems(payload.paths, true);
        }
      })
      .then(
        (u) => (unmounted ? u() : (stop = u)),
        () => {},
      );
    return () => {
      unmounted = true;
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

<svelte:window ondragover={onDragOver} ondrop={onDrop} ondragleave={(e) => !e.relatedTarget && (dropping = false)} />

{#if dropping}
  <div class="drop">Drop video files or VOD links to add them</div>
{/if}

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
  <p class="more muted">
    or <button class="link" onclick={pickFiles} disabled={adding}>add video files…</button> or drop files and links anywhere
  </p>
  {#if error}
    <p class="error-text add-error">{error}</p>
  {/if}

  <div class="list" bind:this={reorder.list}>
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
          onerror={(e) => (error = e)}
        />
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
