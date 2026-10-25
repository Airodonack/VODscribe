<script lang="ts">
  // A VOD's annotations as a list beside its player: its chapters (the one
  // playing lit and kept in view), its highlights in time order with their
  // kind's colour and why, and its cast with their roles and other names. A
  // click on a chapter or highlight plays from its start; the rows take the
  // keyboard (↑/↓, Home and End between them, Enter to play). The website's
  // VOD page shows it in the library pane's place (App.svelte `leftPane`),
  // with its review controls under each row (`review`) and the VOD's own
  // (`reviewVod`) at the top; the app has none of those. Its data comes as the
  // other displays' does: the marks open (cuts.svelte.ts) and the VOD's
  // annotations (annotated.svelte.ts).
  import { tick, type Snippet } from "svelte";
  import type { Vod } from "./api";
  import { castDetail, chapterColor, highlightColor, speakerColor } from "./annotated";
  import { annotated } from "./annotated.svelte";
  import { chapterRows, currentChapter, highlightRows, lengthLabel, stepRow, type PaneRow } from "./annotationRows";
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
  const current = $derived(mine ? currentChapter(chapters, cuts.now) : -1);

  // A highlight clicked shows its why under it until another is.
  let expanded = $state<number | null>(null);

  function activate(row: PaneRow) {
    if (row.kind === "highlight") expanded = row.id;
    onplay(row.start);
  }

  let scroller = $state<HTMLElement>();
  const chapterEls: HTMLElement[] = $state([]);

  // The chapter playing stays in view as it changes: scrolled within the
  // pane only (never the page around it), and only as far as it takes.
  $effect(() => {
    const el = chapterEls[current];
    const box = scroller;
    if (!el || !box) return;
    void tick().then(() => {
      // The rows' offsets are from the list (position: relative).
      const top = el.offsetTop;
      const head = box.querySelector<HTMLElement>(".head")?.offsetHeight ?? 0;
      if (top - head < box.scrollTop) box.scrollTop = top - head;
      else if (top + el.offsetHeight > box.scrollTop + box.clientHeight)
        box.scrollTop = top + el.offsetHeight - box.clientHeight;
    });
  });

  // ↑/↓, Home and End go from row to row, across the chapters and highlights.
  function onRowKey(e: KeyboardEvent) {
    if (e.altKey || e.ctrlKey || e.metaKey || e.shiftKey) return;
    const rows = [...(scroller?.querySelectorAll<HTMLElement>("button.row-main") ?? [])];
    const to = stepRow(rows.indexOf(e.currentTarget as HTMLElement), rows.length, e.key);
    if (to === null) return;
    e.preventDefault();
    rows[to].focus();
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

    {#if chapters.length}
      <section aria-label="Chapters">
        <h2 class="head"><span class="eyebrow">Chapters</span> <span class="muted">{chapters.length}</span></h2>
        {#each chapters as row, i (row.id)}
          <div class="pane-row" class:lit={i === current} bind:this={chapterEls[i]}>
            <button
              class="row-main"
              onclick={() => activate(row)}
              onkeydown={onRowKey}
              aria-current={i === current ? "true" : undefined}
              title="Play from {formatTime(row.start)}"
            >
              <span class="swatch" style:background={chapterColor(i)}></span>
              <span class="time">{formatTime(row.start)}</span>
              <span class="name">{row.title}</span>
            </button>
            {#if review}<div class="review">{@render review(row)}</div>{/if}
          </div>
        {/each}
      </section>
    {/if}

    {#if highlights.length}
      <section aria-label="Highlights">
        <h2 class="head"><span class="eyebrow">Highlights</span> <span class="muted">{highlights.length}</span></h2>
        {#each highlights as row (row.id)}
          {@const length = lengthLabel(row.start, row.end)}
          <div class="pane-row" class:expanded={expanded === row.id}>
            <button
              class="row-main"
              onclick={() => activate(row)}
              onkeydown={onRowKey}
              title={row.why ?? `Play from ${formatTime(row.start)}`}
              aria-expanded={row.why ? expanded === row.id : undefined}
            >
              <span class="dot" style:background={highlightColor(row.category)}></span>
              <span class="text">
                <span class="name">{row.title}</span>
                <span class="meta muted">
                  {formatTime(row.start)}{#if length}<span class="sep">·</span>{length}{/if}{#if row.category}<span
                      class="sep">·</span
                    >{row.category}{/if}
                </span>
              </span>
            </button>
            {#if row.why}<p class="why">{row.why}</p>{/if}
            {#if review}<div class="review">{@render review(row)}</div>{/if}
          </div>
        {/each}
      </section>
    {/if}

    {#if cast.length}
      <section aria-label="Cast">
        <h2 class="head"><span class="eyebrow">Cast</span> <span class="muted">{cast.length}</span></h2>
        <ul class="cast">
          {#each cast as member (member.name)}
            {@const color = speakerColor(cast, member.name)}
            <li>
              <span class="dot" style:background={color}></span>
              <span class="text">
                <strong style:color>{member.name}</strong>
                {#if castDetail(member)}<span class="meta muted">{castDetail(member)}</span>{/if}
              </span>
            </li>
          {/each}
        </ul>
      </section>
    {/if}

    {#if mine && !chapters.length && !highlights.length && !cast.length}
      <p class="muted empty">This VOD has no chapters, highlights or cast.</p>
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

  /* The chapter playing, lit as the library's open VOD is. */
  .pane-row.lit {
    background: var(--accent-soft);
    box-shadow: inset 3px 0 var(--accent);
  }

  .row-main {
    display: flex;
    align-items: baseline;
    gap: 8px;
    width: 100%;
    padding: 9px 14px 8px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--text);
    font-weight: 400;
    text-align: left;
    justify-content: flex-start;
  }

  .row-main:hover:not(:disabled) {
    background: none;
  }

  .row-main:focus-visible {
    outline: 2px solid var(--accent);
    outline-offset: -2px;
  }

  .swatch,
  .dot {
    flex: none;
    width: 9px;
    height: 9px;
    align-self: center;
  }

  .swatch {
    border-radius: 2px;
  }

  .dot {
    border-radius: 50%;
  }

  .row-main .dot {
    align-self: flex-start;
    margin-top: 5px;
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

  /* Why it's a highlight: under it when it's clicked or has the focus, on hover always. */
  .why {
    display: none;
    margin: -2px 0 0;
    padding: 0 14px 9px 31px;
    font-size: 12px;
    line-height: 1.4;
    color: var(--muted);
  }

  .pane-row.expanded .why,
  .pane-row:focus-within .why {
    display: block;
  }

  .review {
    padding: 0 14px 9px;
  }

  .vod-review {
    padding: 10px 14px;
    border-bottom: 1px solid var(--border);
  }

  .cast {
    list-style: none;
    margin: 0;
    padding: 6px 0;
  }

  .cast li {
    display: flex;
    align-items: baseline;
    gap: 8px;
    padding: 5px 14px;
    font-size: 13px;
  }

  .cast .dot {
    align-self: flex-start;
    margin-top: 5px;
  }

  .empty {
    padding: 16px 14px;
    font-size: 13px;
  }
</style>
