<script lang="ts">
  // A VOD's annotations (or an edit's) as a list in the window's right
  // column, or in a tab beside its transcript's on a narrow window
  // (App.svelte): its chapters (each with
  // its emoji when the file gave one), each with the highlights that start in
  // it nested under it; the chapter playing, and any highlight playing, lit
  // and kept in view. (Who's who is in the transcript, over their lines.) A chapter
  // shows its highlights until it's collapsed, then how many it holds; a
  // highlight shows its kind's emoji, title, start and length (and the discs
  // of who's in it, at the end of that line) until it's expanded, then its
  // kind's word too, why and cast by name (each by their own short name, a
  // link where the streamer playing them is: their channel, or their own
  // stream at the highlight's start, castLink.svelte.ts). A click on a
  // row plays from its start; the chevron beside it opens or closes it. The
  // rows take the keyboard as one list (↑/↓, Home and End between them, Enter
  // to play, → and ← to open and close, ← from a highlight to its chapter).
  // A VOD's highlight goes into an edit as a group named for it, one band as
  // a clip's pill does: dragged onto an edit's row in the library, "Make new
  // edit" or the clip bar (dragout.ts `carry`), or from the keyboard, A on
  // its row (or its menu, a right click or the menu key) asking which edit
  // (AddToEdit.svelte). A row pointed at lights its slice or glyph on the
  // timeline, and one pointed at there lights its row (markHover.svelte.ts).
  // The website's VOD page shows it in the same place,
  // with its review controls under each row (`review`) and the VOD's own
  // (`reviewVod`) at the top; the app has none of those, and the website
  // nowhere to put a highlight. Its data comes as the other displays' does:
  // the marks open (cuts.svelte.ts) and the VOD's annotations
  // (annotated.svelte.ts).
  import { tick, type Snippet } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import type { Put, Vod } from "./api";
  import AddToEdit from "./AddToEdit.svelte";
  import { castLink } from "./castLink.svelte";
  import CastDisc from "./CastDisc.svelte";
  import Popover from "./Popover.svelte";
  import { DEMO } from "./demo";
  import { DRAG_SLOP, carry, stretchAt } from "./dragout";
  import { putOf } from "./edits.svelte";
  import { castIndex, speakerColor } from "./annotated";
  import { speakerName } from "./castNames";
  import { annotated } from "./annotated.svelte";
  import { highlightGlyph } from "./annotations";
  import {
    chapterRows,
    currentChapter,
    currentHighlights,
    groupKey,
    hasDetail,
    highlightRows,
    lengthLabel,
    nestRows,
    shownRows,
    sideStep,
    stepRow,
    type HighlightRow,
    type PaneRow,
  } from "./annotationRows";
  import { cuts } from "./cuts.svelte";
  import { markHover } from "./markHover.svelte";
  import { formatTime, playsFromFile } from "./platform";

  let {
    vod,
    edit = null,
    full = false,
    onplay,
    review,
    reviewVod,
  }: {
    /** The VOD whose annotations these are, or none for an edit's. */
    vod: Vod | null | undefined;
    /** The edit whose chapters and highlights these are (on its timeline), if not a VOD's. */
    edit?: number | null;
    /** A column of the window's own, its head as tall as the toolbar beside it. */
    full?: boolean;
    /** Plays the VOD (or the edit) from `time`. */
    onplay: (time: number) => void;
    /** What goes under each chapter and highlight row (the website's review controls). */
    review?: Snippet<[PaneRow]>;
    /** What goes above the lists, for the VOD as a whole. */
    reviewVod?: Snippet;
  } = $props();

  $effect(() => annotated.load(vod?.id));
  const cast = $derived(vod ? (annotated.of(vod.id)?.cast ?? []) : []);
  // The marks open are this VOD's once its player has opened them (or the edit's, open).
  const mine = $derived(
    vod ? cuts.isOf({ kind: "vod", id: vod.id }) : edit !== null && cuts.isOf({ kind: "edit", id: edit }),
  );
  const chapters = $derived(mine ? chapterRows(cuts.shown, vod?.duration) : []);
  const highlights = $derived(mine ? highlightRows(cuts.shown) : []);
  const groups = $derived(nestRows(chapters, highlights));
  const current = $derived(mine ? currentChapter(chapters, cuts.now) : -1);
  // The highlights playing, as a string first: it changes only when they do, not with every move of the playhead.
  const playingIds = $derived(mine ? currentHighlights(highlights, cuts.now).join(" ") : "");
  const playing = $derived(new Set(playingIds ? playingIds.split(" ").map(Number) : []));

  // Chapters are open until collapsed (by groupKey), highlights closed until expanded (by id).
  const collapsed = new SvelteSet<number>();
  const expanded = new SvelteSet<number>();
  const rows = $derived(shownRows(groups, collapsed));

  const isOpen = (row: PaneRow) => (row.kind === "chapter" ? !collapsed.has(row.id) : expanded.has(row.id));
  const opens = (row: PaneRow) =>
    row.kind === "chapter" ? !!groups.find((g) => g.chapter?.id === row.id)?.highlights.length : hasDetail(row);

  function toggle(row: PaneRow) {
    const set = row.kind === "chapter" ? collapsed : expanded;
    if (set.has(row.id)) set.delete(row.id);
    else set.add(row.id);
  }

  function toggleGroup(key: number) {
    if (collapsed.has(key)) collapsed.delete(key);
    else collapsed.add(key);
  }

  /** Who is in a highlight, in the cast's colours (null for a name not in it), with their disc's label (CastDisc)
   *  and the short name their chip shows: the cast's own name for them (the character, not the streamer playing
   *  them, who is in the chip's link), or the name as the highlight wrote it for someone not in the cast. */
  const people = (row: HighlightRow) =>
    row.cast.map((name) => {
      const member = cast[castIndex(cast, name)];
      return {
        name,
        color: speakerColor(cast, name),
        member,
        label: speakerName(member, name),
        short: member?.name ?? name,
      };
    });

  let scroller = $state<HTMLElement>();
  const chapterEls: HTMLElement[] = $state([]);

  // The chapter playing stays in view as it changes, or the highlight playing
  // in it if one is (and shown): scrolled within the pane only (never the
  // page around it), and only as far as it takes.
  $effect(() => {
    const chapter = chapterEls[current];
    const box = scroller;
    void playingIds;
    if (!box) return;
    void tick().then(() => {
      const el = box.querySelector<HTMLElement>(".pane-row.highlight.lit") ?? chapter;
      if (!el) return;
      // The rows' offsets are from the list (position: relative).
      const top = el.offsetTop;
      const head = box.querySelector<HTMLElement>(".head")?.offsetHeight ?? 0;
      if (top - head < box.scrollTop) box.scrollTop = top - head;
      else if (top + el.offsetHeight > box.scrollTop + box.clientHeight)
        box.scrollTop = top + el.offsetHeight - box.clientHeight;
    });
  });

  const rowId = (row: PaneRow) => `${row.kind}-${row.id}`;

  // The row pointed at lights its mark on the timeline; one pointed at there
  // has the hover look here. Not for a finger.
  const pointer = {};
  $effect(() => () => markHover.point(null, pointer));
  function onRowEnter(e: PointerEvent, row: PaneRow) {
    if (e.pointerType !== "touch") markHover.point(row.id, pointer);
  }
  const onRowLeave = () => markHover.point(null, pointer);

  async function focusRow(row: PaneRow) {
    await tick();
    scroller?.querySelector<HTMLElement>(`button.row-main[data-row="${rowId(row)}"]`)?.focus();
  }

  /** What of the VOD a highlight goes into an edit as: its own stretch, or
   *  for one with no end (a mark of the user's), the stretch between cuts it's in. */
  function stretchOf(row: HighlightRow): Put | null {
    if (!vod || !mine || DEMO) return null;
    const end = row.end ?? (vod.duration ? stretchAt(row.start, vod.duration)?.end : undefined);
    if (end === undefined || !(end > row.start)) return null;
    return putOf({ vodId: vod.id, start: row.start, end, title: row.title, highlight: true });
  }

  // A highlight's row pressed and moved is carried into an edit (dragout.ts),
  // as a clip's pill is from the clip bar; the click it leaves doesn't play.
  let press: { row: HighlightRow; x: number; y: number } | null = null;
  let swallowClick = false;
  function onRowDown(e: PointerEvent, row: HighlightRow) {
    if (e.button === 0 && stretchOf(row)) press = { row, x: e.clientX, y: e.clientY };
  }
  function onWindowMove(e: PointerEvent) {
    if (!press || Math.hypot(e.clientX - press.x, e.clientY - press.y) < DRAG_SLOP) return;
    const { row } = press;
    press = null;
    const put = stretchOf(row);
    if (!vod || put?.kind !== "highlight") return;
    swallowClick = true;
    setTimeout(() => (swallowClick = false), 500);
    const from =
      scroller?.querySelector(`[data-row="${rowId(row)}"]`)?.getBoundingClientRect() ??
      new DOMRect(e.clientX, e.clientY);
    void carry({
      vod,
      start: put.start,
      end: put.end,
      title: put.title,
      highlight: true,
      ownPlayer: playsFromFile(vod),
      from,
      x: e.clientX,
      y: e.clientY,
    });
  }
  function onRowClick(row: PaneRow) {
    if (swallowClick) return void (swallowClick = false);
    onplay(row.start);
  }

  // From the keyboard: A on a highlight's row asks which edit it goes in
  // (AddToEdit.svelte); its menu (a right click, or the menu key) has that
  // and Play. The row has the focus again once that's done.
  let adding = $state(false);
  let addFor = $state<{ row: HighlightRow; el: HTMLElement } | null>(null);
  let menuOpen = $state(false);
  function addToEdit(row: HighlightRow, el: HTMLElement) {
    if (!stretchOf(row)) return;
    addFor = { row, el };
    adding = true;
  }
  function openMenu(e: MouseEvent, row: HighlightRow) {
    e.preventDefault();
    addFor = { row, el: e.currentTarget as HTMLElement };
    menuOpen = true;
  }
  let menu = $state<HTMLElement>();
  $effect(() => {
    if (menuOpen) void tick().then(() => menu?.querySelector<HTMLElement>("button")?.focus());
  });
  // Closed with its focus gone (the panel went with it): back on the row.
  let wasOpen = false;
  $effect(() => {
    const open = adding || menuOpen;
    const row = addFor?.row;
    if (wasOpen && !open && row)
      void tick().then(() => {
        if (!document.activeElement || document.activeElement === document.body) void focusRow(row);
      });
    wasOpen = open;
  });

  // The rows shown are one list to the keyboard, chapters and highlights alike.
  function onRowKey(e: KeyboardEvent, row: PaneRow) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    if (row.kind === "highlight" && (e.key === "a" || e.key === "A") && stretchOf(row)) {
      e.preventDefault();
      addToEdit(row, e.currentTarget as HTMLElement);
      return;
    }
    const i = rows.findIndex((r) => r.kind === row.kind && r.id === row.id);
    const to = stepRow(i, rows.length, e.key);
    if (to !== null) {
      e.preventDefault();
      void focusRow(rows[to]);
      return;
    }
    const side = sideStep(rows, i, e.key, isOpen, opens);
    if (!side) return;
    e.preventDefault();
    if ("toggle" in side) toggle(side.toggle);
    else void focusRow(rows[side.focus]);
  }
</script>

<svelte:window onpointermove={onWindowMove} onpointerup={() => (press = null)} />

<aside class="annotations-pane" class:full aria-label="Chapters and highlights">
  <div class="lists" bind:this={scroller}>
    {#if reviewVod}
      <div class="vod-review">{@render reviewVod()}</div>
    {/if}

    {#if groups.length}
      <section aria-label={chapters.length ? "Chapters" : "Highlights"}>
        <h2 class="head">
          {#if chapters.length}
            <span class="eyebrow">Chapters</span> <span class="muted">{chapters.length}</span>
            {#if highlights.length}<span class="sep muted">·</span><span class="eyebrow">Highlights</span>
              <span class="muted">{highlights.length}</span>{/if}
          {:else}
            <span class="eyebrow">Highlights</span> <span class="muted">{highlights.length}</span>
          {/if}
        </h2>
        {#each groups as group (groupKey(group))}
          {@const key = groupKey(group)}
          {@const shut = collapsed.has(key)}
          {@const count = group.highlights.length}
          {#if group.chapter}
            {@const row = group.chapter}
            {@const i = chapters.indexOf(row)}
            <div
              class="pane-row chapter"
              class:lit={i === current}
              class:hover={markHover.id === row.id}
              bind:this={chapterEls[i]}
              onpointerenter={(e) => onRowEnter(e, row)}
              onpointerleave={onRowLeave}
              role="presentation"
            >
              <div class="line">
                {#if count}
                  <button
                    class="chevron"
                    class:shut
                    aria-expanded={!shut}
                    aria-label="{shut ? 'Show' : 'Hide'} the highlights of “{row.title}”"
                    title={shut ? "Show its highlights" : "Hide its highlights"}
                    onclick={() => toggle(row)}
                  ></button>
                {:else}
                  <span class="chevron-space"></span>
                {/if}
                <button
                  class="row-main"
                  data-row={rowId(row)}
                  onclick={() => onRowClick(row)}
                  onkeydown={(e) => onRowKey(e, row)}
                  aria-current={i === current ? "true" : undefined}
                  title="Play from {formatTime(row.start)}"
                >
                  <span class="time">{formatTime(row.start)}</span>
                  {#if row.icon}<span class="emoji" aria-hidden="true">{row.icon}</span>{/if}
                  <span class="name">{row.title}</span>
                  {#if count}
                    <span class="count muted" title="{count} highlight{count === 1 ? '' : 's'}">{count}</span>
                  {/if}
                </button>
              </div>
              {#if review}<div class="review">{@render review(row)}</div>{/if}
            </div>
          {:else if chapters.length}
            <div class="pane-row chapter before">
              <div class="line">
                <button
                  class="chevron"
                  class:shut
                  aria-expanded={!shut}
                  aria-label="{shut ? 'Show' : 'Hide'} the highlights before the first chapter"
                  title={shut ? "Show its highlights" : "Hide its highlights"}
                  onclick={() => toggleGroup(key)}
                ></button>
                <span class="row-head">
                  <span class="name muted">Before the first chapter</span>
                  <span class="count muted">{count}</span>
                </span>
              </div>
            </div>
          {/if}
          {#if !shut}
            {#each group.highlights as row (row.id)}
              {@const length = lengthLabel(row.start, row.end)}
              {@const open = expanded.has(row.id)}
              {@const detail = hasDetail(row)}
              {@const carries = !!stretchOf(row)}
              <div
                class="pane-row highlight"
                class:nested={chapters.length > 0}
                class:expanded={open}
                class:lit={playing.has(row.id)}
                class:hover={markHover.id === row.id}
                onpointerenter={(e) => onRowEnter(e, row)}
                onpointerleave={onRowLeave}
                role="presentation"
              >
                <div class="line">
                  {#if detail}
                    <button
                      class="chevron"
                      class:shut={!open}
                      aria-expanded={open}
                      aria-label="{open ? 'Hide' : 'Show'} what “{row.title}” is"
                      title={open ? "Hide the details" : "Show why, and who's in it"}
                      onclick={() => toggle(row)}
                    ></button>
                  {:else}
                    <span class="chevron-space"></span>
                  {/if}
                  <button
                    class="row-main"
                    data-row={rowId(row)}
                    onclick={() => onRowClick(row)}
                    onkeydown={(e) => onRowKey(e, row)}
                    onpointerdown={(e) => onRowDown(e, row)}
                    oncontextmenu={(e) => carries && openMenu(e, row)}
                    aria-current={playing.has(row.id) ? "true" : undefined}
                    aria-keyshortcuts={carries ? "A" : undefined}
                    title="Play from {formatTime(row.start)}{carries
                      ? ' · drag it into an edit, or press A to add it to one'
                      : ''}"
                  >
                    <span class="emoji" title={row.category} aria-hidden="true">{highlightGlyph(row.category)}</span>
                    <span class="text">
                      <span class="name">{row.title}</span>
                      <!-- Its times, and at the end of that line who's in it, so the title above has the row's width. -->
                      <span class="meta-line">
                        <span class="meta muted">
                          {formatTime(row.start)}{#if length}<span class="sep">·</span
                            >{length}{/if}{#if open && row.category}<span class="sep">·</span>{row.category}{/if}
                        </span>
                        {#if !open && row.cast.length}
                          <span
                            class="faces"
                            title={people(row)
                              .map((p) => p.label)
                              .join(", ")}
                          >
                            {#each people(row).slice(0, 4) as p, j (j)}
                              <span class="face"
                                ><CastDisc
                                  name={p.label}
                                  color={p.color}
                                  member={p.member}
                                  size={16}
                                  link={false}
                                /></span
                              >
                            {/each}
                            {#if row.cast.length > 4}<span class="more muted">+{row.cast.length - 4}</span>{/if}
                          </span>
                        {/if}
                      </span>
                    </span>
                  </button>
                </div>
                {#if open}
                  <div class="detail">
                    {#if row.why}<p class="why">{row.why}</p>{/if}
                    {#if row.cast.length}
                      <p class="people">
                        {#each people(row) as p, j (j)}
                          {@const to = castLink(p.member, vod, row.start)}
                          {#if to}
                            <a
                              class="chip"
                              href={to.href}
                              title={to.pov ? to.title : `${p.member?.player?.trim() || p.member?.name} · ${to.title}`}
                              onclick={(e) => (e.preventDefault(), to.follow())}
                              ><CastDisc
                                name={p.label}
                                color={p.color}
                                member={p.member}
                                size={16}
                                link={false}
                              /><strong class:unknown={!p.color} style:color={p.color}>{p.short}</strong></a
                            >
                          {:else}
                            <span class="chip"
                              ><CastDisc
                                name={p.label}
                                color={p.color}
                                member={p.member}
                                size={16}
                                link={false}
                              /><strong class:unknown={!p.color} style:color={p.color}>{p.short}</strong></span
                            >
                          {/if}
                        {/each}
                      </p>
                    {/if}
                  </div>
                {/if}
                {#if review}<div class="review">{@render review(row)}</div>{/if}
              </div>
            {/each}
          {/if}
        {/each}
      </section>
    {/if}

    {#if mine && !chapters.length && !highlights.length}
      <!-- Some there, but the Timeline panel's Marks toggles hide them all. -->
      {#if cuts.all.some((m) => m.kind === "chapter" || m.kind === "highlight")}
        <p class="muted empty">
          Its chapters and highlights are hidden: show them from Timeline, above the transcript.
        </p>
      {:else}
        <p class="muted empty">This VOD has no chapters or highlights.</p>
      {/if}
    {/if}
  </div>
</aside>

{#if addFor}
  <Popover bind:open={menuOpen} anchor={addFor.el} label="Highlight menu" width={200} padding={4}>
    <div class="menu" role="menu" bind:this={menu}>
      <button
        role="menuitem"
        onclick={() => {
          menuOpen = false;
          if (addFor) onplay(addFor.row.start);
        }}>Play from {formatTime(addFor.row.start)}</button
      >
      <button
        role="menuitem"
        onclick={() => {
          menuOpen = false;
          if (addFor) addToEdit(addFor.row, addFor.el);
        }}>Add to edit…<kbd>A</kbd></button
      >
    </div>
  </Popover>
  <AddToEdit
    bind:open={adding}
    anchor={addFor.el}
    items={() => {
      const put = addFor && stretchOf(addFor.row);
      return put ? [put] : [];
    }}
  />
{/if}

<style>
  /* A sidebar as the library is (Library.svelte), on the window's right;
     its width is the layout's (App.svelte). */
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    border-left: 1px solid var(--border);
    background: var(--surface);
  }

  .menu {
    display: flex;
    flex-direction: column;
  }

  .menu button {
    display: flex;
    align-items: center;
    gap: 8px;
    justify-content: flex-start;
    border: none;
    background: none;
    text-align: left;
  }

  .menu button:hover,
  .menu button:focus-visible {
    background: var(--surface-2);
  }

  .menu kbd {
    margin-left: auto;
  }

  .meta {
    font-size: 12px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .sep {
    margin: 0 5px;
  }

  .lists {
    flex: 1;
    min-height: 0;
    overflow-x: hidden;
    overflow-y: auto;
    position: relative;
  }

  /* As the transcript's header (TranscriptHead.svelte), staying at the top of its list. */
  .head {
    position: sticky;
    top: 0;
    z-index: 1;
    margin: 0;
    padding: 8px 14px;
    background: var(--bg);
    border-bottom: 1px solid var(--border);
    font-size: 12px;
    font-weight: 400;
  }

  /* The window's own column: its head the toolbar's height (and the
     library's top), so the three columns' tops line up. */
  .full .head {
    display: flex;
    align-items: center;
    flex-wrap: wrap;
    gap: 0 4px;
    min-height: 53px;
    box-sizing: border-box;
    background: var(--surface);
  }

  .full .head .sep {
    margin: 0 1px;
  }

  /* --row-bg: the row's background, which rings each disc in a highlight's stack (.face). */
  .pane-row {
    --row-bg: var(--surface);
    border-bottom: 1px solid var(--border);
    background: var(--row-bg);
  }

  .pane-row:hover {
    --row-bg: var(--bg);
  }

  /* The chapter playing, and a highlight playing, lit as the library's open VOD is. */
  .pane-row.lit {
    --row-bg: var(--accent-soft);
    box-shadow: inset 3px 0 var(--accent);
  }

  /* Its mark pointed at on the timeline (or the row itself): outlined in
     the lit row's colour, lit or not. */
  .pane-row.hover {
    box-shadow: inset 0 0 0 1px var(--accent);
  }

  .pane-row.lit.hover {
    box-shadow:
      inset 3px 0 var(--accent),
      inset 0 0 0 1px var(--accent);
  }

  /* A row: its chevron, then what plays it. */
  .line {
    display: flex;
    align-items: stretch;
    min-width: 0;
  }

  .row-main {
    flex: 1;
    min-width: 0;
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 9px 14px 8px 2px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--text);
    font-weight: 400;
    text-align: left;
    justify-content: flex-start;
    /* A button keeps to one line (app.css); a long title wraps instead of being cut off. */
    white-space: normal;
  }

  /* What opens and closes a row, a chevron pointing down when it's open. */
  .chevron,
  .chevron-space {
    flex: none;
    width: 26px;
    margin-left: 4px;
  }

  .chevron {
    display: flex;
    align-items: center;
    justify-content: center;
    padding: 0;
    border: none;
    border-radius: var(--radius);
    background: none;
    color: var(--muted);
  }

  .chevron::before {
    content: "";
    width: 6px;
    height: 6px;
    border-right: 1.5px solid currentColor;
    border-bottom: 1.5px solid currentColor;
    transform: translateY(-2px) rotate(45deg);
  }

  .chevron.shut::before {
    transform: translateX(-2px) rotate(-45deg);
  }

  .chevron:hover:not(:disabled) {
    background: none;
    color: var(--text);
  }

  .chevron:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -4px;
  }

  /* A highlight under its chapter, in from it. */
  .highlight.nested .line {
    padding-left: 18px;
  }

  .highlight .chevron {
    align-items: flex-start;
    padding-top: 13px;
  }

  .row-head {
    flex: 1;
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 9px 14px 8px 2px;
    min-width: 0;
  }

  .count {
    margin-left: auto;
    flex: none;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  /* A highlight's times, then who's in it at the far end of the line; the
     times are what's cut short when both don't fit. */
  .meta-line {
    display: flex;
    align-items: center;
    gap: 8px;
    min-width: 0;
  }

  .meta-line .meta {
    flex: 1;
    min-width: 0;
  }

  /* Who's in a collapsed highlight: their discs (CastDisc.svelte), each
     overlapping the one before and ringed in the row's colour. */
  .faces {
    flex: none;
    display: flex;
    align-items: center;
  }

  .face {
    display: flex;
    border-radius: 50%;
    box-shadow: 0 0 0 1.5px var(--row-bg);
  }

  .face + .face {
    margin-left: -4px;
  }

  .more {
    margin-left: 4px;
    font-size: 11px;
  }

  .row-main:hover:not(:disabled) {
    background: none;
  }

  .row-main:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  /* A highlight's kind, as its emoji, beside its title. */
  .emoji {
    flex: none;
    width: 16px;
    align-self: flex-start;
    font-family: "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", sans-serif;
    font-size: 14px;
    line-height: 18px;
    text-align: center;
  }

  .time {
    flex: none;
    min-width: 3.2em;
    color: var(--muted);
    font-size: 12px;
    font-variant-numeric: tabular-nums;
  }

  .name {
    min-width: 0;
    font-size: 13px;
    line-height: 1.35;
    overflow-wrap: anywhere;
  }

  .text {
    flex: 1;
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  /* The chapter playing in bold; a highlight's title stays regular, lit or not. */
  .pane-row.chapter.lit .name {
    font-weight: 600;
  }

  .row-main .meta {
    font-variant-numeric: tabular-nums;
  }

  /* An expanded highlight's why and cast, under its title. */
  .detail {
    padding: 0 14px 9px 74px;
    font-size: 12px;
    line-height: 1.4;
  }

  .highlight:not(.nested) .detail {
    padding-left: 56px;
  }

  .detail p {
    margin: 0;
  }

  .why {
    color: var(--muted);
  }

  .why + .people {
    margin-top: 4px;
  }

  .people {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 10px;
  }

  /* A cast member's disc, then their name in their colour, a link to the
     streamer's channel (or stream) when they have one. A chip is never cut short: one
     that doesn't fit beside the one before goes to the next line whole, and
     only a name wider than the column breaks, between its words. */
  .chip {
    display: inline-flex;
    align-items: center;
    gap: 5px;
    max-width: 100%;
    color: inherit;
    text-decoration: none;
  }

  a.chip:hover strong {
    text-decoration: underline;
  }

  a.chip:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: var(--radius);
  }

  .people strong {
    font-weight: 600;
    overflow-wrap: break-word;
  }

  .people strong.unknown {
    color: var(--text);
  }

  /* Under the row's title, as its detail is. */
  .review {
    padding: 0 14px 9px 32px;
  }

  .highlight .review {
    padding-left: 56px;
  }

  .highlight.nested .review {
    padding-left: 74px;
  }

  .vod-review {
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }

  .empty {
    padding: 16px 14px;
    font-size: 13px;
  }
</style>
