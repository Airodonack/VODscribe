<script lang="ts">
  // A VOD's annotations as a list beside its player: its chapters (each with
  // its emoji when the file gave one), each with the highlights that start in
  // it nested under it; the chapter playing, and any highlight playing, lit
  // and kept in view. (Who's who is in the transcript, over their lines.) A chapter
  // shows its highlights until it's collapsed, then how many it holds; a
  // highlight shows its kind's emoji, title, start and length (and dots for
  // who's in it) until it's expanded, then its kind's word too, why and cast
  // by name. A click on a
  // row plays from its start; the chevron beside it opens or closes it. The
  // rows take the keyboard as one list (↑/↓, Home and End between them, Enter
  // to play, → and ← to open and close, ← from a highlight to its chapter).
  // The website's VOD page shows it in the library pane's place (App.svelte
  // `leftPane`), with its review controls under each row (`review`) and the
  // VOD's own (`reviewVod`) at the top; the app has none of those. Its data
  // comes as the other displays' does: the marks open (cuts.svelte.ts) and
  // the VOD's annotations (annotated.svelte.ts).
  import { tick, type Snippet } from "svelte";
  import { SvelteSet } from "svelte/reactivity";
  import type { Vod } from "./api";
  import { speakerColor } from "./annotated";
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
  import { formatTime } from "./platform";

  let {
    vod,
    onplay,
    review,
    reviewVod,
  }: {
    vod: Vod | null | undefined;
    /** Plays the VOD from `time`. */
    onplay: (time: number) => void;
    /** What goes under each chapter and highlight row (the website's review controls). */
    review?: Snippet<[PaneRow]>;
    /** What goes above the lists, for the VOD as a whole. */
    reviewVod?: Snippet;
  } = $props();

  $effect(() => annotated.load(vod?.id));
  const cast = $derived(vod ? (annotated.of(vod.id)?.cast ?? []) : []);
  // The marks open are this VOD's once its player has opened them.
  const mine = $derived(!!vod && cuts.isOf({ kind: "vod", id: vod.id }));
  const chapters = $derived(mine ? chapterRows(cuts.all, vod?.duration) : []);
  const highlights = $derived(mine ? highlightRows(cuts.all) : []);
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

  /** Who is in a highlight, in the cast's colours (null for a name not in it). */
  const people = (row: HighlightRow) => row.cast.map((name) => ({ name, color: speakerColor(cast, name) }));

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

  async function focusRow(row: PaneRow) {
    await tick();
    scroller?.querySelector<HTMLElement>(`button.row-main[data-row="${rowId(row)}"]`)?.focus();
  }

  // The rows shown are one list to the keyboard, chapters and highlights alike.
  function onRowKey(e: KeyboardEvent, row: PaneRow) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
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

<aside class="annotations-pane" aria-label="Annotations">
  <div class="top">
    {#if vod}
      <span class="title" title={vod.title}>{vod.title}</span>
      <span class="meta muted">
        <span class="platform {vod.platform}">{vod.platform[0].toUpperCase() + vod.platform.slice(1)}</span>
        {#if vod.channel}<span class="sep">·</span>{vod.channel}{/if}
        {#if vod.duration}<span class="sep">·</span>{formatTime(vod.duration)}{/if}
      </span>
    {/if}
  </div>

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
            <div class="pane-row chapter" class:lit={i === current} bind:this={chapterEls[i]}>
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
                  onclick={() => onplay(row.start)}
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
              <div
                class="pane-row highlight"
                class:nested={chapters.length > 0}
                class:expanded={open}
                class:lit={playing.has(row.id)}
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
                    onclick={() => onplay(row.start)}
                    onkeydown={(e) => onRowKey(e, row)}
                    aria-current={playing.has(row.id) ? "true" : undefined}
                    title="Play from {formatTime(row.start)}"
                  >
                    <span class="emoji" title={row.category} aria-hidden="true">{highlightGlyph(row.category)}</span>
                    <span class="text">
                      <span class="name">{row.title}</span>
                      <span class="meta muted">
                        {formatTime(row.start)}{#if length}<span class="sep">·</span
                          >{length}{/if}{#if open && row.category}<span class="sep">·</span>{row.category}{/if}
                      </span>
                    </span>
                    {#if !open && row.cast.length}
                      <span class="faces" title={row.cast.join(", ")}>
                        {#each people(row).slice(0, 4) as p, j (j)}
                          <span class="face" class:unknown={!p.color} style:background={p.color}></span>
                        {/each}
                        {#if row.cast.length > 4}<span class="more muted">+{row.cast.length - 4}</span>{/if}
                      </span>
                    {/if}
                  </button>
                </div>
                {#if open}
                  <div class="detail">
                    {#if row.why}<p class="why">{row.why}</p>{/if}
                    {#if row.cast.length}
                      <p class="people">
                        {#each people(row) as p, j (j)}{#if j}<span class="muted">, </span>{/if}<strong
                            class:unknown={!p.color}
                            style:color={p.color}>{p.name}</strong
                          >{/each}
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
      <p class="muted empty">This VOD has no chapters or highlights.</p>
    {/if}
  </div>
</aside>

<style>
  /* The library pane's place and look (Library.svelte). */
  aside {
    display: flex;
    flex-direction: column;
    min-height: 0;
    min-width: 0;
    border-right: 1px solid var(--border);
    background: var(--surface);
  }

  /* As high as the library's header, level with the toolbar beside it. */
  .top {
    flex: none;
    display: flex;
    flex-direction: column;
    justify-content: center;
    gap: 1px;
    box-sizing: border-box;
    height: 53px;
    padding: 0 14px;
    border-bottom: 1px solid var(--border);
    min-width: 0;
  }

  .top .title {
    font-weight: 700;
    font-size: 14px;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
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

  .pane-row {
    border-bottom: 1px solid var(--border);
    background: var(--surface);
  }

  .pane-row:hover {
    background: var(--bg);
  }

  /* The chapter playing, and a highlight playing, lit as the library's open VOD is. */
  .pane-row.lit {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 var(--accent);
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

  /* Who's in a collapsed highlight: a dot each in their cast colour. */
  .faces {
    margin-left: auto;
    flex: none;
    display: flex;
    align-items: center;
    gap: 3px;
    align-self: center;
  }

  .face {
    width: 7px;
    height: 7px;
    border-radius: 50%;
  }

  .face.unknown {
    background: var(--muted);
  }

  .more {
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
    display: flex;
    flex-direction: column;
    gap: 1px;
    min-width: 0;
  }

  /* A highlight's title over its times; the chapter playing in bold. */
  .text .name,
  .pane-row.lit .name {
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

  .people strong {
    font-weight: 600;
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
