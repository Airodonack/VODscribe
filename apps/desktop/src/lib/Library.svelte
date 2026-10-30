<script module lang="ts">
  import type { Edit, LibraryItem, Vod as VodOf } from "./api";

  /** A row of the library's list: a VOD or an edit, in one order. */
  export type LibraryRow = { kind: "vod"; vod: VodOf } | { kind: "edit"; edit: Edit };

  /** A row's key for reordering (reorder.svelte.ts keys rows by number): a
   *  VOD's id, or an edit's id made negative. */
  export const rowKey = (row: LibraryRow) => (row.kind === "vod" ? row.vod.id : -row.edit.id);
  const itemOf = (key: number): LibraryItem => (key > 0 ? { kind: "vod", id: key } : { kind: "edit", id: -key });
</script>

<script lang="ts">
  import { backDropped, backLeft, backOver, draggingOut, isOutgoing } from "./dragout";
  import { getVersion } from "@tauri-apps/api/app";
  import { getCurrentWebview } from "@tauri-apps/api/webview";
  import { open } from "@tauri-apps/plugin-dialog";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { onMount, tick, untrack } from "svelte";
  import { flip } from "svelte/animate";
  import { api, type Put, type Vod } from "./api";
  import { os } from "./platform";
  import { DEMO, demoOff } from "./demo";
  import Icon from "./Icon.svelte";
  import { AUTHOR, SITE, releaseNotes } from "./links";
  import Logo from "./Logo.svelte";
  import { Adder } from "./adder.svelte";
  import { annotated } from "./annotated.svelte";
  import Spinner from "./Spinner.svelte";
  import { Reorder } from "./reorder.svelte";
  import UpdateBanner from "./UpdateBanner.svelte";
  import FormatHint from "./FormatHint.svelte";
  import VodRow from "./VodRow.svelte";
  import EditRow from "./EditRow.svelte";
  import { clips } from "./clips.svelte";
  import { edits } from "./edits.svelte";
  import { carryEntries, chosenEntries, chosenToTranscribe, removeChosenEntries, transcribeChosen } from "./bulk";
  import AddToEdit from "./AddToEdit.svelte";
  import Menu, { type MenuItem } from "./Menu.svelte";
  import { actionsFor } from "./actions";
  import { offers } from "./offers.svelte";
  import { pressToDrag, type Pressed } from "./pressToDrag";
  import { readAhead } from "./readAhead";
  import { restOver } from "./resting";
  import { keyHint } from "./keys.svelte";
  import { count, dragBox, howOf, library, overlaps, type Entry } from "./selection.svelte";

  let {
    rows,
    selectedId,
    onselect,
    onreorder,
    onopenedit,
  }: {
    rows: LibraryRow[];
    selectedId: number | null;
    onselect: (id: number | null) => void;
    onreorder: (items: LibraryItem[]) => void;
    onopenedit: (id: number) => void;
  } = $props();

  // A row's menu, opened by a right click on it (or the menu key): what can
  // be done with its VOD or edit (actions.ts), and for a VOD from a site,
  // asking for its annotations. Add to edit… there asks which edit, beside the row.
  let menuOpen = $state(false);
  let menuFor = $state<{ row: LibraryRow; el: HTMLElement } | null>(null);
  let addingRow = $state(false);
  function openMenu(e: MouseEvent, row: LibraryRow) {
    // The web demo keeps its VODs as they are.
    if (DEMO) return;
    e.preventDefault();
    menuFor = { row, el: e.currentTarget as HTMLElement };
    menuOpen = true;
  }
  const rowPut = (row: LibraryRow): Put =>
    row.kind === "vod" ? { kind: "vod", id: row.vod.id } : { kind: "clip", id: row.edit.id };
  const menuItems = $derived.by((): MenuItem[] => {
    const row = menuFor?.row;
    if (!row) return [];
    const addToEdit = () => (addingRow = true);
    if (row.kind === "edit") return actionsFor({ kind: "edit", edit: row.edit }, { addToEdit });
    const vod = row.vod;
    const items: MenuItem[] = actionsFor(
      { kind: "vod", vod },
      { addToEdit, onremove: (id) => id === selectedId && onselect(null) },
    );
    // Annotations are published for VODs from sites (a file's id is this computer's own).
    if (vod.platform !== "local")
      items.splice(
        items.length - 1,
        0,
        { label: "Check for annotations", icon: "gift", run: () => void offers.check(vod) },
        { label: "Check all VODs for annotations", icon: "gift", run: () => void offers.checkLibrary() },
      );
    return items;
  });

  let url = $state("");
  // Adding links and files, one after another (adder.svelte.ts). Its error is
  // also where a row's failed action shows, under the form.
  // What's added is opened, paused, to watch, clip or transcribe.
  const adder = new Adder((id) => onselect(id));

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
    // A clip of ours, dragged out and back: not a link to add (but not opened in the window either).
    if (draggingOut()) return e.preventDefault();
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
    if (draggingOut()) return e.preventDefault();
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
        // A clip of ours being dragged out, back over the window: it can go
        // where a carry inside could (dragout.ts). Positions come in device pixels.
        if (draggingOut()) {
          const at = "position" in payload ? payload.position : null;
          const [x, y] = at ? [at.x / devicePixelRatio, at.y / devicePixelRatio] : [0, 0];
          if (payload.type === "leave") backLeft();
          else if (payload.type === "drop") backDropped(x, y);
          else backOver(x, y);
          return;
        }
        if (payload.type === "enter") {
          // macOS reports every drag, even ones with nothing the app can use.
          // A clip being dragged out of the app isn't one to add.
          filesOver = payload.paths.length > 0 && !draggingInside && !draggingOut() && !isOutgoing(payload.paths);
        } else if (payload.type === "leave") filesOver = false;
        else if (payload.type === "drop") {
          filesOver = false;
          if (!draggingInside && !draggingOut() && !isOutgoing(payload.paths)) adder.items(payload.paths, true);
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

  // Dragging a row (anywhere but its action buttons) or Alt+↑/↓ reorders the
  // library, VODs and edits together.
  const reorder = new Reorder(
    () => rows.map(rowKey),
    (keys) => onreorder(keys.map(itemOf)),
  );
  let byKey = $derived(new Map(rows.map((r) => [rowKey(r), r])));
  let shown = $derived(reorder.order.map((key) => byKey.get(key)).filter((r) => r !== undefined));

  // Rows picked as in a file manager (selection.svelte.ts, pick.ts): a plain
  // click opens a row and picks it alone; Ctrl-click (⌘ on a Mac) adds one
  // to it or takes it out, Shift-click picks the run from the last clicked,
  // ↑ and ↓ open the row before or after (with Shift, picking the run to
  // it), or a box is dragged from empty space below the rows. The open row
  // is picked when nothing else is; with more picked, what's picked shows
  // above the list, with Remove…, and the open row stays the one lit at its edge.
  const entryOf = (row: LibraryRow): Entry =>
    row.kind === "vod" ? { kind: "vod", id: row.vod.id } : { kind: "edit", id: row.edit.id };
  const isOpen = (row: LibraryRow) => (row.kind === "vod" ? row.vod.id === selectedId : edits.openId === row.edit.id);
  const openEntry = $derived<Entry | null>(
    selectedId !== null
      ? { kind: "vod", id: selectedId }
      : edits.openId !== null
        ? { kind: "edit", id: edits.openId }
        : null,
  );
  library.follow(() => openEntry);
  // Opened another way (the keys, an add, a link): picked alone, as if clicked.
  let wasOpen: Entry | null = null;
  $effect(() => {
    const now = openEntry;
    untrack(() => library.opened(wasOpen, now));
    wasOpen = now;
  });
  function onRowClick(e: MouseEvent, row: LibraryRow) {
    if ((e.target as HTMLElement).closest(".actions")) return;
    const how = howOf(e);
    if (how === "plain") {
      // The open row clicked with others picked: it's picked alone and stays
      // open (else its click would close a VOD). Alone, its click closes it.
      if (isOpen(row) && library.picking) {
        e.preventDefault();
        e.stopPropagation();
      }
      library.click(entryOf(row), how, shown.map(entryOf));
      return;
    }
    e.preventDefault();
    e.stopPropagation();
    library.click(entryOf(row), how, shown.map(entryOf));
  }
  /** Opens `row`, as a plain click on it does (an open VOD stays open). */
  function openRow(row: LibraryRow) {
    if (row.kind === "vod") onselect(row.vod.id);
    else onopenedit(row.edit.id);
  }
  /** The rows a box drawn over the list touches, in their order. */
  const boxed = (box: DOMRect) =>
    shown
      .filter((r) => {
        const el = reorder.rows[rowKey(r)];
        return el && overlaps(box, el.getBoundingClientRect());
      })
      .map(entryOf);
  // A row picked that's gone another way (its own Remove, an edit deleted
  // from its view) goes from the set too: else it would still count in the
  // numbers shown, and go along in a drag or Add to edit… as a VOD that's gone.
  $effect(() => {
    const here = new Set(rows.map(rowKey));
    library.keep((e) => here.has(e.kind === "vod" ? e.id : -e.id));
  });
  const vodsPicked = $derived(rows.filter((r) => r.kind === "vod" && library.has(entryOf(r))).length);
  const editsPicked = $derived(rows.filter((r) => r.kind === "edit" && library.has(entryOf(r))).length);
  const removePicked = () => removeChosenEntries(rows, (id) => id === selectedId && onselect(null));

  // "Add to edit…" for what's picked: the edits, then where in one.
  let adding = $state(false);
  let addButton = $state<HTMLElement>();

  // A picked row dragged carries what's picked (into an edit: bulk.ts
  // `carryEntries`) rather than reordering the list; the click that ends it
  // doesn't open the row (pressToDrag).
  let pressing = false;
  function onRowDown(e: PointerEvent, row: LibraryRow, key: number) {
    // A clip carried over the list is dropped, not a row dragged.
    if (clips.carryingAny || (e.target as HTMLElement).closest(".actions")) return false;
    pressing = e.button === 0 && library.picking && library.has(entryOf(row));
    if (!pressing) reorder.start(e, key);
    return pressing;
  }
  function onRowDrag(e: PointerEvent, key: number, { x, y }: Pressed) {
    const from = reorder.rows[key]?.getBoundingClientRect() ?? new DOMRect(x, y);
    carryEntries(from, e.clientX, e.clientY);
  }

  // A VOD's row the pointer rests on is read ahead (readAhead.ts), so a
  // click on it opens it with its transcript, marks and chat at hand; never
  // the one open, nor while something's carried or dragged (a button held
  // is no rest: resting.ts `restOver`).
  function restedOn(row: LibraryRow) {
    if (row.kind !== "vod" || row.vod.id === selectedId) return;
    if (pressing || clips.carryingAny || draggingOut()) return;
    void readAhead(row.vod).catch(() => {});
  }
  // And once one is open and has settled, the VODs either side of it, the next to be opened with ↑ and ↓.
  const AROUND_AFTER = 1500;
  $effect(() => {
    const id = selectedId;
    if (id === null) return;
    const timer = setTimeout(() => {
      const at = shown.findIndex((r) => r.kind === "vod" && r.vod.id === id);
      if (at < 0) return;
      for (const row of [shown[at - 1], shown[at + 1]])
        if (row?.kind === "vod") void readAhead(row.vod).catch(() => {});
    }, AROUND_AFTER);
    return () => clearTimeout(timer);
  });

  // Transcribe on a VOD picked with others that can be: hovered, they all
  // light up; clicked, they're all asked for, in the order picked.
  const toTranscribe = $derived(chosenToTranscribe(rows));
  let transcribeLit = $state(false);
  function together(vod: Vod) {
    if (toTranscribe.length < 2 || !toTranscribe.some((v) => v.id === vod.id)) return undefined;
    return {
      count: toTranscribe.length,
      lit: (on: boolean) => (transcribeLit = on),
      run: () => {
        transcribeLit = false;
        adder.error = null;
        void transcribeChosen(toTranscribe, (message) => (adder.error = message));
      },
    };
  }

  function onRowKey(e: KeyboardEvent, key: number) {
    if (reorder.key(e, key)) return void tick().then(() => focusRow(key));
    // ↑ and ↓ (no Alt: that reorders): the row before or after opened and
    // picked alone; with Shift, the run from the last clicked picked, the
    // open row staying open.
    const by = e.key === "ArrowUp" ? -1 : e.key === "ArrowDown" ? 1 : 0;
    if (!by || e.altKey || e.ctrlKey || e.metaKey) return;
    const from = byKey.get(key);
    if (!from) return;
    e.preventDefault();
    const order = shown.map(entryOf);
    const to = library.step(entryOf(from), by, e.shiftKey, order);
    const row = to && shown[order.findIndex((o) => o.kind === to.kind && o.id === to.id)];
    if (!row) return;
    if (!e.shiftKey && !isOpen(row)) openRow(row);
    focusRow(rowKey(row));
  }
  const focusRow = (key: number) => reorder.rows[key]?.querySelector<HTMLElement>(".main")?.focus();
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

<aside class="library-pane">
  <!-- Sibling elements, since buttons can't nest: the site link fills the
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
    <span class="credit">Made by {AUTHOR}</span>
  </div>

  <UpdateBanner />
  <FormatHint />

  <form onsubmit={add}>
    <input
      type="url"
      placeholder="Paste a VOD link…"
      bind:value={url}
      disabled={adder.adding}
      readonly={DEMO}
      aria-label="VOD link"
      autocorrect="off"
      autocapitalize="off"
      spellcheck="false"
      {...demoOff(
        "The web demo can't transcribe. In the app, paste a Kick, Twitch, YouTube or other video link here and it's transcribed on your computer.",
      )}
    />
    <button
      class="primary"
      disabled={adder.adding || (!url.trim() && !DEMO)}
      {...demoOff(
        "The web demo can't transcribe. In the app, paste a Kick, Twitch, YouTube or other video link here and it's transcribed on your computer.",
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

  {#if library.picking && vodsPicked + editsPicked}
    <!-- What's picked in the list, and what can be done with it. -->
    <div class="picked" role="status">
      <span class="count"
        >{count([
          [vodsPicked, "VOD", "VODs"],
          [editsPicked, "edit", "edits"],
        ])}</span
      >
      <button
        class="link"
        bind:this={addButton}
        onclick={() => (adding = !adding)}
        title="Put them in an edit, in the order picked (each VOD whole): on its timeline or its clip bar"
        >Add to edit…</button
      >
      <button class="link" onclick={removePicked} title="Remove them, after asking{keyHint('library.remove')}"
        >Remove…</button
      >
      <button
        class="clear"
        onclick={() => library.clear()}
        title="Let go of them (Esc)"
        aria-label="Clear the selection"
      >
        <Icon name="x" size={13} />
      </button>
    </div>
  {/if}

  <div
    class="list library-list"
    role="listbox"
    aria-label="Library"
    aria-multiselectable="true"
    bind:this={reorder.list}
    use:dragBox={{ set: library, hits: boxed, onempty: () => library.clear() }}
  >
    <!-- While a clip is carried: the place to start an edit with it (dragout.ts). -->
    {#if clips.carryingAny}
      <div class="new-edit" class:target={edits.dropTarget === "new"} data-new-edit>
        <Icon name="plus" size={15} /> Make new edit
      </div>
    {/if}
    <!-- Links and files being added: each stands in until its VOD arrives (new ones go on top). -->
    {#each adder.pending as p (p.key)}
      <div class="vod pending" role="status" aria-label="Adding {p.label}">
        <span class="standin-thumb"><Spinner size={16} /></span>
        <span class="standin-text">
          <span class="standin-title">{p.label}</span>
          <span class="muted standin-meta">{p.started ? "Adding…" : "Waiting…"}</span>
        </span>
        <button
          class="row-act cancel"
          onclick={() => adder.cancel(p.key)}
          title="Cancel"
          aria-label="Cancel adding {p.label}"
        >
          <Icon name="x" size={15} />
        </button>
      </div>
    {/each}
    {#each shown as row (rowKey(row))}
      {@const key = rowKey(row)}
      {@const picked = library.has(entryOf(row))}
      {@const n = library.number(entryOf(row))}
      <!-- The row's own button has the focus and the keys (VodRow, EditRow). -->
      <!-- svelte-ignore a11y_click_events_have_key_events, a11y_interactive_supports_focus -->
      <div
        class="vod"
        class:edit={row.kind === "edit"}
        class:selected={row.kind === "vod" ? row.vod.id === selectedId : edits.openId === row.edit.id}
        class:chosen={picked && library.picking}
        class:lit={transcribeLit && row.kind === "vod" && toTranscribe.some((v) => v.id === row.vod.id)}
        class:lifted={reorder.lifted(key)}
        role="option"
        aria-selected={picked}
        data-key={key}
        style:transform={reorder.transform(key)}
        bind:this={reorder.rows[key]}
        animate:flip={{ duration: reorder.flipDuration(key) }}
        onclickcapture={(e) => {
          reorder.clickCapture(e);
          if (!e.defaultPrevented) onRowClick(e, row);
        }}
        onclick={(e) => {
          if ((e.target as HTMLElement).closest("button")) return;
          if (row.kind === "vod") toggle(row.vod);
          else onopenedit(row.edit.id);
        }}
        use:pressToDrag={{
          press: (e) => onRowDown(e, row, key),
          start: (e, at) => onRowDrag(e, key, at),
          end: () => (pressing = false),
        }}
        use:restOver={() => restedOn(row)}
        oncontextmenu={(e) => openMenu(e, row)}
      >
        {#if n !== null}<span class="order row-order">{n}</span>{/if}
        {#if row.kind === "vod"}
          {@const vod = row.vod}
          <VodRow
            {vod}
            reorderable={rows.length > 1}
            ontoggle={() => toggle(vod)}
            together={together(vod)}
            onkey={(e) => onRowKey(e, key)}
            onremove={() => selectedId === vod.id && onselect(null)}
            onerror={(e) => (adder.error = e)}
          />
        {:else}
          <EditRow
            edit={row.edit}
            reorderable={rows.length > 1}
            onopen={() => onopenedit(row.edit.id)}
            onkey={(e) => onRowKey(e, key)}
          />
        {/if}
      </div>
    {:else}
      {#if adder.pending.length === 0}
        <p class="muted empty">No VODs yet. Paste a link above to add one.</p>
      {/if}
    {/each}
  </div>
</aside>

<AddToEdit bind:open={adding} anchor={addButton} items={chosenEntries} />

<Menu
  bind:open={menuOpen}
  anchor={menuFor?.el}
  label={menuFor?.row.kind === "edit" ? "Edit menu" : "VOD menu"}
  items={menuItems}
  width={250}
/>
<AddToEdit bind:open={addingRow} anchor={menuFor?.el} items={() => (menuFor ? [rowPut(menuFor.row)] : [])} />

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

  /* While a clip is carried: start an edit with it, at the top of the list. */
  .new-edit {
    position: sticky;
    top: 0;
    z-index: 3;
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 6px;
    margin: 8px 12px;
    padding: 12px;
    border: 1px dashed var(--accent);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    color: var(--accent);
    font-size: 13px;
    font-weight: 600;
  }

  .new-edit.target {
    border-style: solid;
    background: color-mix(in srgb, var(--accent) 14%, var(--surface));
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

  .vod.pending .cancel {
    margin-left: auto;
    align-self: center;
    flex: none;
  }

  .vod.selected {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 var(--accent);
  }

  /* Picked with others (Ctrl- or Shift-click, or a box): outlined, the open
     one still lit at its edge. */
  .vod.chosen {
    background: color-mix(in srgb, var(--accent) 10%, var(--surface));
    box-shadow: inset 0 0 0 2px var(--accent);
  }

  /* Transcribe on one of them is hovered: all it would transcribe. */
  .vod.lit {
    background: color-mix(in srgb, var(--accent) 18%, var(--surface));
    box-shadow: inset 0 0 0 2px var(--accent);
  }

  .vod.chosen.selected {
    box-shadow:
      inset 3px 0 var(--accent),
      inset 0 0 0 2px var(--accent);
  }

  /* Its place in the order, at the thumbnail's corner. */
  .row-order {
    position: absolute;
    top: 6px;
    left: 6px;
    z-index: 2;
  }

  /* What's picked, above the list. */
  .picked {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 6px 8px 6px 12px;
    border-bottom: 1px solid var(--border);
    background: var(--accent-soft);
    font-size: 12px;
  }

  .picked .count {
    font-weight: 600;
  }

  .picked .link {
    color: var(--accent);
    font-size: 12px;
  }

  .picked .clear {
    margin-left: auto;
    width: 22px;
    height: 22px;
    padding: 0;
    border: none;
    border-radius: 50%;
    background: none;
    color: var(--muted);
    display: inline-flex;
    align-items: center;
    justify-content: center;
  }

  .picked .clear:hover {
    background: var(--surface-3);
    color: var(--text);
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
