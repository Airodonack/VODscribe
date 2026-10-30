<script lang="ts">
  import { flushSync, untrack } from "svelte";
  import { SvelteMap, SvelteSet } from "svelte/reactivity";
  import {
    api,
    events,
    type ChatOverview,
    type ChatWindow,
    type Line,
    type Mark,
    type SearchHit,
    type Stretch,
    type Vod,
  } from "./api";
  import ChatMessage from "./ChatMessage.svelte";
  import Minimap from "./Minimap.svelte";
  import TranscriptHead, { type PaneTabs } from "./TranscriptHead.svelte";
  import { layers } from "./layers.svelte";
  import { eventColor } from "./events";
  import { followTarget } from "./follow";
  import { Marks } from "./marks.svelte";
  import { playedLength, playedSpans } from "./media";
  import { formatTime } from "./platform";
  import { pressToDrag } from "./pressToDrag";
  import {
    CHAT_BINS,
    CHAT_SHOWN,
    HEAD_HEIGHT,
    buildRows,
    linesFrom,
    linesSpan,
    messagesIn,
    nearestLine,
    type Row,
  } from "./rows";
  import { shownGaps, statusLabel, type Gap } from "./stretches";
  import TranscribeFrom from "./TranscribeFrom.svelte";
  import { cuts } from "./cuts.svelte";
  import { edits } from "./edits.svelte";
  import { load, save } from "./stored";
  import Icon from "./Icon.svelte";
  import { annotated } from "./annotated.svelte";
  import { castIndex, speakerColor } from "./annotated";
  import { GROUP_HEAD_HEIGHT, anyoneNamed, groupHeads, type GroupHead } from "./transcriptGroups";
  import { groupSpans, pinnedAt, reserveAt, type Divider } from "./pinned";
  import { castLink } from "./castLink.svelte";
  import { speakerName } from "./castNames";
  import CastDisc from "./CastDisc.svelte";

  // A VOD's transcript, or (`edit`) the open edit's: its pieces' lines
  // where they play, on its timeline, with their VODs' chat. A click on a line
  // plays from it; dragging across lines selects them: a VOD's to cut
  // around (Split), an edit's to take out (Delete). The line at the
  // playhead is kept in view, playing or not, until the reader scrolls away.
  // It reads as a chat window: a speaker's lines one after another sit under
  // one head with their name (transcriptGroups.ts), and each chapter's start
  // has a divider with its title. While a chapter's or a speaker's lines are
  // at the top, their heading stays pinned there (pinned.ts).
  let {
    vod = null,
    edit = null,
    vods = new Map(),
    reveal = null,
    onrevealed,
    tabs = null,
    onplay,
  }: {
    vod?: Vod | null;
    edit?: number | null;
    /** The library's VODs, for an edit's: whose chat it has. */
    vods?: Map<number, Vod>;
    /** A search match to scroll to and light for a moment (a double-click on it, App.svelte). */
    reveal?: SearchHit | null;
    /** `reveal` taken in hand: it isn't shown again. */
    onrevealed?: () => void;
    /** The switch between the transcript and the chapters and highlights,
     *  on a window too narrow for their column (App.svelte); the heading
     *  stays over the pane while it's open. */
    tabs?: PaneTabs | null;
    onplay: (time: number) => void;
  } = $props();

  // An edit's VODs, the one with most time in it first (the Timeline panel's).
  let editVods = $derived.by(() => {
    if (edit === null) return [];
    const time = new Map<number, number>();
    for (const p of edits.pieces) {
      time.set(p.vod_id, (time.get(p.vod_id) ?? 0) + playedLength(p));
    }
    return [...time.entries()].sort((a, b) => b[1] - a[1]).flatMap(([id]) => (vods.get(id) ? [vods.get(id)!] : []));
  });
  let chatVods = $derived(vod ? [vod] : editVods);

  /** Where VOD `vodId`'s moment `time` shows here: on a VOD, itself; on an
   *  edit, where it first plays, if it does. */
  function toTimeline(vodId: number, time: number): number | null {
    if (edit === null) return time;
    let at = 0;
    for (const p of edits.pieces) {
      for (const [a, b] of playedSpans(p.start, p.end, p.struck)) {
        if (p.vod_id === vodId && time >= a && time < b) return at + time - a;
        at += b - a;
      }
    }
    return null;
  }

  let lines = $state.raw<Line[]>([]);
  // Who's who, from an annotations import: the speakers' colours.
  $effect(() => annotated.load(vod?.id));
  let cast = $derived(vod ? (annotated.of(vod.id)?.cast ?? []) : []);
  // What of the VOD is transcribed, and asked to be (stretches.ts).
  let stretches = $state.raw<Stretch[]>([]);
  let error = $state<string | null>(null);
  // Whether the lines and stretches have been read once: until then nothing
  // is known of what's transcribed, so neither the parts not transcribed nor
  // the empty state show (they did for a frame or two, the whole VOD "not
  // transcribed", before its lines replaced them).
  let read = $state(false);

  // Everything is read again when the VOD changes, and when what's asked of it
  // does (a request made, started, ended); while a stretch is transcribed,
  // its lines are read again as they come (below).
  // What chat is of: the VOD, or the edit as it is (its pieces place it).
  let vodId = $derived(vod?.id ?? -1);
  let asked = $derived(vod ? `${vod.status} ${vod.stretch} ${vod.transcribed} ${vod.waiting}` : null);
  // The request running (App.svelte keeps the VOD's status and progress as
  // its events come), and how far, in twentieths: each step reads its lines.
  let running = $derived(vod?.status === "transcribing" ? vod.stretch : null);
  let step = $derived(running !== null ? Math.floor(vod!.progress * 20) : null);
  // An edit's lines are asked for again at each change to it.
  let revision = $derived(edit !== null && edits.open?.id === edit ? edits.open.revision : null);
  let duration = $derived(edit !== null ? edits.length : (vod?.duration ?? 0));
  // Only the latest request's answer counts, so a slow older reload can't win;
  // reading everything again makes any stretch's read under way stale too.
  let seq = 0;
  let rangeSeq = 0;
  $effect(() => {
    const id = vodId;
    void asked;
    void revision;
    // Annotations imported bring a transcript of their own.
    void annotated.imports(vod?.id);
    const mine = ++seq;
    rangeSeq++;
    if (edit !== null) {
      api.editLines(edit).then(
        (l) => mine === seq && ((lines = l), (error = null), (read = true)),
        (e) => mine === seq && ((error = String(e)), (read = true)),
      );
      return;
    }
    Promise.all([api.transcript(id), api.transcriptStretches(id)]).then(
      ([l, s]) => {
        if (mine !== seq) return;
        lines = l;
        stretches = s;
        error = null;
        read = true;
      },
      (e) => mine === seq && ((error = String(e)), (read = true)),
    );
  });

  // A stretch being transcribed replaces the lines in its range as it goes
  // (crates/core `replace_segments`): those are read again, and put in place
  // of what was there, so a stretch in the middle doesn't mean reading all;
  // and the stretches, so its row of what's still to come moves on below them.
  $effect(() => {
    const id = running;
    void step;
    if (id === null) return;
    untrack(() => {
      const stretch = stretches.find((s) => s.id === id);
      // Not read yet: everything is being read, it with it.
      if (!stretch) return;
      const mine = ++rangeSeq;
      const { start, end } = stretch;
      Promise.all([api.transcriptRange(vodId, start, end), api.transcriptStretches(vodId)]).then(
        ([l, s]) => {
          if (mine !== rangeSeq) return;
          const before = lines.filter((line) => line.start < start);
          const after = end === null ? [] : lines.filter((line) => line.start >= end);
          lines = [...before, ...l, ...after];
          stretches = s;
        },
        (e) => mine === rangeSeq && (error = String(e)),
      );
    });
  });

  // The parts not transcribed, each with the request that will transcribe it,
  // if any: rows of their own among the lines, even with none yet (a part
  // transcribed that was silence). Before anything is transcribed or asked
  // for, the empty state says so instead (stretches.ts `shownGaps`).
  let gaps = $derived(vod && read ? shownGaps(stretches, vod.duration, vod.transcribed, lines.length) : []);

  /** Asks for `start` to `end` (null: the end) to be transcribed. */
  function transcribe(start: number, end: number | null) {
    if (!vod) return;
    api.transcribeStretch(vod.id, start, end).catch((e) => (error = String(e)));
  }

  // "Transcribe from here": how much from the playhead, from the player
  // bar's menu (TranscribeFrom.svelte), from where it was when opened.
  let fromButton = $state<HTMLElement>();
  let fromMenu = $state(false);
  let fromHere = $state(0);
  function transcribeFromHere() {
    fromHere = mine ? cuts.now : 0;
    fromMenu = !fromMenu;
  }

  /** Stops request `id`, keeping the lines it has written. */
  function stop(id: number) {
    api.cancelStretch(id).catch((e) => (error = String(e)));
  }

  /** Request `request`'s state, as far as it's got: the VOD's own events have the latest of the one it shows. */
  function requestLabel(request: Stretch): string {
    const live = vod?.stretch === request.id ? vod : request;
    const label = statusLabel[live.status];
    const busy = live.status === "downloading" || live.status === "transcribing";
    return busy ? `${label} ${Math.round(live.progress * 100)}%` : label;
  }
  // The stretch being transcribed, for the header.
  let runningStretch = $derived(stretches.find((s) => s.id === running) ?? null);

  // Chat, beside the transcript and collapsible (remembered across VODs and
  // launches). A busy VOD's is a hundred thousand messages and more, so the
  // view never holds it all: it asks for the counts and events it draws on the
  // minimap (`overview`), and for each row it shows, that row's first messages
  // (`windows`). Both catch up as a fetch saves more.
  const CHAT_OPEN = "vodscribe.chatOpen";
  let chatError = $state<string | null>(null);
  let chatProgress = $state<number | null>(null);
  let chatOpen = $state(load(CHAT_OPEN) !== "false");
  // The chat as a whole (counts and events), when it's been asked for.
  let overview = $state.raw<ChatOverview | null>(null);
  // The length its histogram's bins divide.
  let overviewDuration = 0;
  // An edit's chat is its pieces' VODs' where they play: shown once there's some.
  // A file's chat is one an annotations import brought.
  let hasChat = $derived(vod ? vod.platform !== "local" || vod.chat === "done" : (overview?.count ?? 0) > 0);
  // A VOD without a chat replay (a video that wasn't a livestream, say): its
  // toggle is grayed out and the column stays shut. A fetch that stopped
  // partway (failed) keeps what it fetched, shown like the rest until Try
  // again brings the others.
  let chatStatus = $derived(vod ? vod.chat : "done");
  let noChat = $derived(chatStatus === "none");
  let showChat = $derived(hasChat && chatOpen && !noChat);
  let canHaveChat = $derived(!vod || (hasChat && !noChat));

  let root: HTMLDivElement;

  // Makes `change`, keeping the row at the top of the view where it was:
  // showing or hiding chat changes every row's height. (Heights that change
  // as rows are measured or their chat arrives are kept in place by `remeasure`.)
  function keepPlace(change: () => void) {
    // A pixel's leeway: scroll positions are rounded, so the row at the top
    // can otherwise read as the one ending just above it.
    const y = viewY();
    const index = rowAtY(y + 1);
    const within = y - offsets[index];
    const key = rows[index] ? rows[index].key : null;
    flushSync(change);
    if (!scroller || key === null) return;
    const now = rows.findIndex((r) => r.key === key);
    if (now >= 0) scroller.scrollTop = listTop + offsets[now] + within - headHeight;
  }

  function toggleChat() {
    keepPlace(() => (chatOpen = !chatOpen));
    save(CHAT_OPEN, String(chatOpen));
  }

  // The rows: each line with its chat, and minutes of chat where there's no line (rows.ts).
  // None until the lines are read: a VOD's minutes of chat stood in for its
  // lines meanwhile, every one of them asked for its chat.
  let rows = $derived(read ? buildRows(lines, duration, showChat, gaps) : []);

  // Whether the marks open (cuts.svelte.ts) are this VOD's or edit's: then
  // so are the playhead and the moment pointed at on the timeline there.
  const mine = $derived(vod ? cuts.vodId === vod.id : cuts.doc?.kind === "edit" && cuts.doc.id === edit);

  // Each chapter's title (cuts.svelte.ts) as a divider over the row that
  // starts nearest its start: a line's, or a part not transcribed.
  const heads = $derived.by(() => {
    const out = new Map<string, Mark[]>();
    if (!mine) return out;
    const anchors = rows.filter((row) => row.line || row.gap).map((row) => ({ start: row.from, key: row.key }));
    if (!anchors.length) return out;
    for (const chapter of cuts.chapters) {
      const key = anchors[nearestLine(anchors, chapter.start)].key;
      out.set(key, [...(out.get(key) ?? []), chapter]);
    }
    return out;
  });

  // The head over each row that starts a group of one speaker's lines (by
  // place in `rows`): a group ends at a chapter's divider, so they agree.
  // A cast member's alias is them, grouped by their name, so one person reads
  // as one; a name not in the cast as the line has it. The head reads their
  // display name (castNames.ts): the character first, the streamer in parentheses where one plays them.
  // Where anyone is named, lines no one is named for are grouped under "Unknown", so they never read as
  // the speaker's before them; where no one is, there are no heads at all.
  const named = $derived(anyoneNamed(rows.map((row) => row.line)));
  const speakerHeads = $derived(
    groupHeads(
      rows.map((row) => ({ line: row.line, divided: heads.has(row.key) })),
      (name) => cast[castIndex(cast, name)]?.name ?? name,
      named,
    ),
  );

  // Only the rows in and near the view are laid out; empty space of the right
  // height stands in for the rest, so a long VOD with a busy chat opens, toggles
  // chat and jumps around at once, however far it goes. Each row's height is
  // measured the first time it's laid out and kept for the session (per row,
  // and per whether chat is shown, since that lays rows out differently); until
  // then it's estimated from its text and how many messages it shows.
  const measured = new Map<string, number>();
  let measuredGen = $state(0);
  /** Each row's key for its measured height, made once per set of rows. */
  let heightKeys = $derived(rows.map((row) => `${showChat ? "chat" : "text"}|${row.key}`));
  let width = $state(0);
  function estimate(row: Row, index: number): number {
    const textWidth = Math.max(160, (showChat ? width * 0.6 : width) - 110);
    const head = (heads.get(row.key)?.length ?? 0) * HEAD_HEIGHT + (speakerHeads[index] ? GROUP_HEAD_HEIGHT : 0);
    // A gap's row says what it is in about this many characters.
    const chars = row.line ? row.line.text.length : row.gap ? 60 : 0;
    const text = chars ? head + 10 + 22.5 * Math.max(1, Math.ceil((chars * 7.8) / textWidth)) : 0;
    if (!showChat) return text;
    const win = windows.get(row.key);
    // Before its chat is fetched, a row is guessed to hold as many messages as
    // the chat's counts say it has (a couple, before they're read).
    const total = win
      ? win.total
      : overview
        ? Math.round(messagesIn(overview.histogram, overviewDuration, row.from, row.to))
        : 2;
    const shown = win ? win.messages.length : Math.min(total, limitOf(row));
    if (!row.line && !row.gap && !total) return 0;
    const chat = shown * 21 + (total > shown ? 22 : 0) + (shown ? 4 : 0);
    return Math.max(text, chat);
  }
  /** Where each row starts in the list, and (last) where it ends. */
  let offsets = $derived.by(() => {
    void measuredGen;
    void windows;
    const out = new Float64Array(rows.length + 1);
    const keys = heightKeys;
    rows.forEach((row, i) => (out[i + 1] = out[i] + (measured.get(keys[i]) ?? estimate(row, i))));
    return out;
  });
  /** The last row whose `at(i)` is at most `x`, or the first row (rows are in order). */
  function lastRowAtOrBefore(at: (i: number) => number, x: number): number {
    let lo = 0;
    let hi = rows.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (at(mid) <= x) lo = mid;
      else hi = mid - 1;
    }
    return Math.max(0, lo);
  }
  /** The row at `y` in the list (clamped). */
  const rowAtY = (y: number) => lastRowAtOrBefore((i) => offsets[i], y);

  // The scroller's position, and where the list starts in it (below the header).
  let scrollTop = $state(0);
  let listTop = $state(0);
  let topPad = $state<HTMLElement>();
  /** The top of the view, in the list: just below the header, which stays at the top. */
  const viewY = () => (scroller?.scrollTop ?? 0) + headHeight - listTop;
  // Laid out: what's in view and this much either side, so scrolling finds rows ready.
  const AROUND = 1000;
  let range = $derived.by((): [number, number] => {
    const top = scrollTop + headHeight - listTop;
    const from = rowAtY(top - AROUND);
    const to = Math.min(rows.length, rowAtY(top + railHeight + AROUND) + 1);
    return [from, Math.max(from, to)];
  });

  // The headings pinned at the top of the view (pinned.ts): the chapter and
  // the speaker whose lines are there, once their own heading is above it.
  // Only the rows near the view are laid out, so they're drawn over the list
  // (`.pinned`) from where the rows are, not by the headings sticking.

  // How tall each row's dividers and head are, as laid out (`remeasure`);
  // those not laid out yet as the last that was (a title or a name that
  // wraps in a narrow pane is taller).
  const headHeights = new Map<string, { divider: number; speaker: number }>();
  let headsGen = $state(0);
  let dividerHeight = HEAD_HEIGHT;
  let speakerHeight = GROUP_HEAD_HEIGHT;
  const dividers = $derived.by(() => {
    void headsGen;
    return rows.flatMap((row, i): Divider[] => {
      const count = heads.get(row.key)?.length ?? 0;
      return count ? [{ row: i, height: headHeights.get(row.key)?.divider || dividerHeight * count }] : [];
    });
  });
  const spans = $derived(
    groupSpans(
      speakerHeads,
      rows.map((row) => !!row.line && (named || !!row.line.speaker?.trim())),
    ),
  );
  const groups = $derived.by(() => {
    void headsGen;
    return spans.map((span) => ({ ...span, height: headHeights.get(rows[span.row].key)?.speaker || speakerHeight }));
  });
  const pinned = $derived.by(() => pinnedAt(offsets, dividers, groups, scrollTop + headHeight - listTop));
  const pinnedChapters = $derived(pinned.chapter >= 0 ? (heads.get(rows[dividers[pinned.chapter].row].key) ?? []) : []);
  const pinnedGroup = $derived(pinned.group >= 0 ? speakerHeads[groups[pinned.group].row] : null);
  /** How much of the view's top the pinned headings cover with row `i` at the top. */
  const reserve = (i: number) => reserveAt(i, dividers, groups);
  /** Notes how tall the dividers and head over row `el` (row `index`) are, as laid out. */
  function measureHeads(el: HTMLElement, index: number) {
    let divider = 0;
    for (const d of el.querySelectorAll<HTMLElement>(":scope > .chapter-head"))
      divider += d.getBoundingClientRect().height + parseFloat(getComputedStyle(d).marginTop);
    const speaker = el.querySelector<HTMLElement>(":scope > .group-head")?.getBoundingClientRect().height ?? 0;
    if (!divider && !speaker) return;
    const key = rows[index].key;
    const was = headHeights.get(key);
    if (was?.divider === divider && was?.speaker === speaker) return;
    headHeights.set(key, { divider, speaker });
    if (divider) dividerHeight = divider / (heads.get(key)?.length || 1);
    if (speaker) speakerHeight = speaker;
    headsGen++;
  }

  /** Scrolls row `i` to the top of the view, below what's pinned with it there. */
  function toRow(i: number) {
    if (!scroller || i < 0 || i >= rows.length) return;
    aim = null;
    scroller.scrollTop = listTop + offsets[i] - headHeight - reserve(i);
  }

  // A line focused from the keyboard is scrolled clear of the header and the
  // headings pinned under it, as are the rows brought into view below.
  $effect(() => {
    const s = scroller;
    if (!s) return;
    void headsGen;
    const most = (dividers.length ? dividerHeight : 0) + (groups.length ? speakerHeight : 0);
    s.style.scrollPaddingTop = `${headHeight + most}px`;
    return () => void (s.style.scrollPaddingTop = "");
  });

  // Measures rows as they're laid out or change size. A row above the view
  // that turns out taller or shorter than thought moves the view by as much, so
  // what's being read stays put (WebKit has no scroll anchoring of its own).
  let resizes: ResizeObserver | undefined;
  function remeasure(entries: ResizeObserverEntry[]) {
    const y = viewY();
    let shift = 0;
    let changed = false;
    for (const entry of entries) {
      const el = entry.target as HTMLElement;
      const index = Number(el.dataset.index);
      const row = rows[index];
      // Hidden (the pane in the transcript's place), a row has no height of
      // its own: kept as it was, rather than every row laid out as 0 px
      // tall, all of them then laid out, and measured again on the way back.
      if (!row || !el.isConnected || !el.getClientRects().length) continue;
      measureHeads(el, index);
      const key = heightKeys[index];
      const before = measured.get(key) ?? estimate(row, index);
      // As laid out, to the fraction: rows a fraction of a pixel taller than
      // whole pixels would put each next row that much further from where
      // the list thinks, and the pinned headings would be pushed out early.
      const height = entry.borderBoxSize?.[0]?.blockSize ?? el.getBoundingClientRect().height;
      if (height === before) continue;
      measured.set(key, height);
      changed = true;
      if (offsets[index] + before <= y) shift += height - before;
    }
    // Rows laid out at the height they were thought to be change nothing, but
    // just after a seek, the one sought may be among them.
    if (!changed) {
      if (aimed()) place();
      caughtUp();
      return;
    }
    flushSync(() => measuredGen++);
    // Just after a seek, the moment sought stays centered; otherwise what's read stays put.
    if (aimed()) place();
    else if (shift && scroller) scroller.scrollTop += shift;
    caughtUp();
  }
  /** Where the view is, now: measuring runs after the frame's scroll
   *  events, just before it's painted, so a move it makes would otherwise
   *  reach what's drawn from it (the pinned headings, the rows laid out) a
   *  frame late, drawn for the old place over the rows at the new one. */
  function caughtUp() {
    const s = scroller;
    if (s && s.scrollTop !== scrollTop) flushSync(() => (scrollTop = s.scrollTop));
  }
  function measure(node: HTMLElement) {
    resizes ??= new ResizeObserver(remeasure);
    resizes.observe(node);
    return { destroy: () => resizes?.unobserve(node) };
  }
  $effect(() => () => resizes?.disconnect());

  // A row shows CHAT_SHOWN messages at first (rows.ts); each "more" shows this many more.
  const SHOWN = CHAT_SHOWN;
  const MORE = 200;
  let windows = $state(new Map<string, ChatWindow>());
  /** Windows to fetch again: chat arrived since (a fetch is under way). */
  const stale = new Set<string>();
  /** How many messages expanded rows show, by window. */
  const limits = new SvelteMap<string, number>();
  const limitOf = (row: Row) => limits.get(row.key) ?? SHOWN;
  /** Windows being fetched: their "more" buttons wait. */
  const loading = new SvelteSet<string>();
  let chatGen = 0;
  // With chat shown, the rows wait for the chat of those in view: laid out
  // before it, they grew as it came and pushed the lines out of view (a
  // busy chat's first minutes are rows of hundreds of pixels each).
  let chatIn = $state(false);
  let held = $derived(showChat && !chatIn);
  $effect(() => {
    if (read && !showChat) chatIn = true;
  });

  // A different VOD, or a different chat status, starts afresh.
  let chatOf: number | string | null = null;
  $effect(() => {
    const id = vod ? vodId : `edit ${edit} ${revision}`;
    void chatStatus;
    void canHaveChat;
    // Only those: what the loads read and change mustn't start this over.
    untrack(() => {
      chatGen++;
      if (chatOf !== id) {
        chatOf = id;
        windows = new Map();
        // Row keys repeat from VOD to VOD; their heights don't.
        measured.clear();
        limits.clear();
        overview = null;
      } else {
        // Same VOD, now fetched in full (or failed): what's shown is refreshed in place.
        for (const key of windows.keys()) stale.add(key);
      }
      loading.clear();
      chatError = null;
      refreshOverview();
      loadWindows();
    });
  });

  /** Fetches the windows of the rows laid out (and a few more) that aren't loaded, or are stale. */
  function loadWindows() {
    if (!canHaveChat || !showChat) return;
    const id = vodId;
    const gen = chatGen;
    // The rows laid out, and a few more either side to have ready.
    const [from, to] = range;
    const wanted: Row[] = [];
    const near = rows.slice(Math.max(0, from - 10), to + 10);
    near.forEach((row) => {
      const key = row.key;
      if (loading.has(key)) return;
      if (!windows.has(key) || stale.has(key)) wanted.push(row);
    });
    if (read && near.every((row) => windows.has(row.key))) chatIn = true;
    if (wanted.length === 0) return;
    for (const row of wanted) loading.add(row.key);
    const fetch = (batch: Row[], cap: number) =>
      edit !== null
        ? api.editChatWindows(
            edit,
            batch.map((r) => [r.from, r.to]),
            cap,
          )
        : api.chatWindows(
            id,
            batch.map((r) => [r.from, r.to]),
            cap,
          );
    // One request per limit: almost every row has the first one.
    const byLimit = new Map<number, Row[]>();
    for (const row of wanted) byLimit.set(limitOf(row), [...(byLimit.get(limitOf(row)) ?? []), row]);
    const groups = [...byLimit];
    Promise.all(groups.map(([cap, batch]) => fetch(batch, cap))).then(
      (results) => {
        chatIn = true;
        if (gen !== chatGen || id !== vodId) return;
        const next = new Map(windows);
        groups.forEach(([, batch], g) =>
          batch.forEach((row, i) => {
            const key = row.key;
            next.set(key, results[g][i]);
            stale.delete(key);
            loading.delete(key);
          }),
        );
        // Rows fetched above the view grow (their estimates now count their
        // messages), which would push the view down.
        if (aimed()) {
          flushSync(() => (windows = next));
          place();
        } else keepPlace(() => (windows = next));
      },
      (e) => {
        chatIn = true;
        for (const row of wanted) loading.delete(row.key);
        if (gen === chatGen) chatError = String(e);
      },
    );
  }
  // More rows shown (scrolling, a seek), chat shown, or new lines: their windows.
  $effect(() => {
    void rows;
    void range;
    void showChat;
    untrack(() => loadWindows());
  });

  function expand(row: Row) {
    const key = row.key;
    limits.set(key, limitOf(row) + MORE);
    stale.add(key);
    loadWindows();
  }

  // The chat as a whole: how many messages, how busy each stretch was, and the events.
  const BINS = CHAT_BINS;
  let mapDuration = $derived(Math.max(duration, lines.at(-1)?.start ?? 0));
  /** The counts, and with `events` the subs, gifts and tips too, which
   *  otherwise keep what was had: while a fetch is saving, they're a megabyte
   *  and more for a busy chat, so they're read now and then, not each time. */
  const EVENTS_EVERY = 15000;
  let eventsAt = 0;
  // Only the latest of each kind counts, so a quick refresh of the counts
  // can't cancel a slower one that brings the events (a fetch just finished).
  let eventsSeq = 0;
  let countsSeq = 0;
  function refreshOverview(withEvents = true) {
    if (!canHaveChat) return void (overview = null);
    const id = vodId;
    // A refresh with the events has the counts too, newer than any asked for before it.
    const mine = withEvents ? ++eventsSeq : ++countsSeq;
    if (withEvents) {
      countsSeq++;
      eventsAt = Date.now();
    }
    const latest = () => id === vodId && mine === (withEvents ? eventsSeq : countsSeq);
    const over = mapDuration;
    (edit !== null
      ? api.editChatOverview(edit, mapDuration, BINS, withEvents)
      : api.chatOverview(id, mapDuration, BINS, withEvents)
    ).then(
      (o) => {
        if (!latest()) return;
        overviewDuration = over;
        overview = { ...o, events: o.events ?? (overview?.events || []) };
      },
      (e) => latest() && (chatError = String(e)),
    );
  }
  let chatCount = $derived(overview?.count ?? 0);
  // An edit's length is known once its pieces load, after it opens: its
  // minimap's counts are asked for again on that timeline.
  $effect(() => {
    if (edit === null) return;
    void mapDuration;
    untrack(() => refreshOverview());
  });

  // While a fetch saves batches: at most this often, the counts and the rows in
  // view are fetched again; other rows when they come into view.
  const REFRESH_EVERY = 1500;
  $effect(() => {
    const id = vodId;
    // A change of status (the fetch done) reads everything afresh; a refresh
    // still waiting here would only come after it. So does an import's chat.
    void chatStatus;
    void annotated.imports(vod?.id);
    let timer: ReturnType<typeof setTimeout> | undefined;
    let last = 0;
    const refresh = () => {
      timer = undefined;
      last = Date.now();
      for (const key of windows.keys()) stale.add(key);
      refreshOverview(Date.now() - eventsAt >= EVENTS_EVERY);
      loadWindows();
    };
    const unlisten = events.chatSaved((vid) => {
      if (vid !== id || timer) return;
      timer = setTimeout(refresh, Math.max(0, REFRESH_EVERY - (Date.now() - last)));
    });
    return () => {
      clearTimeout(timer);
      void unlisten.then((u) => u());
    };
  });

  $effect(() => {
    const id = vodId;
    chatProgress = null;
    const unlisten = events.chatProgress((vid, f) => vid === id && (chatProgress = f));
    return () => void unlisten.then((u) => u());
  });

  // Playing from a line (or a chapter, a chat message, lines selected, a
  // search match shown here) is choosing to follow: the transcript follows
  // the playhead from there.
  function play(time: number) {
    followFrom(time);
    onplay(time);
  }

  // Pressing on a line and dragging across others selects them (cuts.svelte.ts:
  // Split then cuts either side); a click without a drag still plays the line,
  // and the click a drag's release leaves isn't one on a line (pressToDrag).
  // Its ends are the lines' starts (rows.ts `linesFrom`), not their rows.
  let drag = $state<{ from: number; to: number } | null>(null);
  let pointer = { x: 0, y: 0 };
  let scrolling = 0;

  /** The index in `rows` of the line row under the point, if any. */
  function lineRowAt(x: number, y: number): number | null {
    const el = document.elementFromPoint(x, y)?.closest<HTMLElement>(".line");
    const i = el && root.contains(el) ? Number(el.dataset.index) : NaN;
    return Number.isInteger(i) && rows[i]?.line ? i : null;
  }

  function onRowDown(e: PointerEvent, line: Line) {
    if (e.pointerType === "touch") return false;
    drag = { from: line.start, to: line.start };
    pointer = { x: e.clientX, y: e.clientY };
  }

  function onDragMove(e: PointerEvent) {
    if (!drag) return;
    pointer = { x: e.clientX, y: e.clientY };
    follow();
    // Near the top or bottom of the view, it scrolls on, for picks longer than a screen.
    if (!scrolling) scrolling = requestAnimationFrame(autoScroll);
  }

  function follow() {
    if (!drag) return;
    const i = lineRowAt(pointer.x, pointer.y);
    const at = i === null ? null : rows[i].line!.start;
    if (at !== null && at !== drag.to) drag.to = at;
  }

  function autoScroll() {
    scrolling = 0;
    if (!drag || !scroller) return;
    const box = scroller.getBoundingClientRect();
    const edge = 48;
    const speed =
      pointer.y < box.top + edge
        ? -(box.top + edge - pointer.y)
        : pointer.y > box.bottom - edge
          ? pointer.y - (box.bottom - edge)
          : 0;
    if (!speed) return;
    scroller.scrollBy(0, Math.max(-24, Math.min(24, speed / 2)));
    follow();
    scrolling = requestAnimationFrame(autoScroll);
  }

  /** The press is over; `dragged`, the lines dragged across (one, or more) are selected. */
  function onDragEnd(dragged: boolean) {
    if (!drag) return;
    const { from, to } = drag;
    drag = null;
    cancelAnimationFrame(scrolling);
    scrolling = 0;
    if (!dragged) return;
    const span = linesFrom(lines, from, to);
    if (span) select(span[0], span[1]);
  }

  /** Selects lines `a` to `b` (either way round) and plays from the first. */
  function select(a: number, b: number) {
    if (a < 0 || b < 0) return;
    const { start, end } = linesSpan(lines, Math.min(a, b), Math.max(a, b), duration);
    // The lines selected (Split then cuts either side of them, or in an edit
    // Delete takes them out), and the playhead to their start, showing the
    // VOD in the player if it wasn't. The selection is the marks' (cuts.svelte.ts),
    // so a mark selected lets go: Delete is then the lines'.
    if (vod) cuts.open(vod.id);
    if (mine) cuts.selection = { start, end };
    play(start);
  }

  function onRowClick(e: MouseEvent, line: Line) {
    // Shift+click selects the line, or the lines from the selection's first to it.
    if (e.shiftKey) {
      const pick = mine ? cuts.selection : null;
      const at = lines.indexOf(line);
      select(pick ? nearestLine(lines, pick.start) : at, at);
      return;
    }
    const time = line.start;
    // A click elsewhere than the selection's edges lets go of it.
    if (mine) cuts.clearSelection();
    play(time);
  }

  /** Whether `row` is a line being dragged across, or selected. */
  function picked(row: Row): boolean {
    if (drag) {
      const start = row.line?.start;
      return start !== undefined && start >= Math.min(drag.from, drag.to) && start <= Math.max(drag.from, drag.to);
    }
    const pick = mine ? cuts.selection : null;
    return !!pick && !!row.line && row.line.start >= pick.start - 0.01 && row.line.start < pick.end - 0.01;
  }

  // The minimap at the right edge (Minimap.svelte): chat activity over the
  // whole VOD, or speech when there's no chat, as tall as the visible area.
  let scroller = $state<HTMLElement | null>(null);
  let railHeight = $state(0);
  $effect(() => {
    const s = root.closest<HTMLElement>(".body");
    scroller = s;
    if (!s) return;
    // At once, not when the observer first reports: the minimap was drawn a
    // few pixels tall in the first frame and grew to the view's height in the next.
    railHeight = s.clientHeight;
    const ro = new ResizeObserver(() => (railHeight = s.clientHeight));
    ro.observe(s);
    return () => ro.disconnect();
  });
  let lineTimes = $derived(lines.map((l) => l.start));
  let mapLabel = $derived(chatCount > 0 ? "How busy chat was over the VOD" : "Speech over the VOD");
  // Chat alone is enough to draw: it can be fetched before the transcript has a line.
  // From the first frame where the VOD has lines or chat to show on it, so it
  // doesn't come as they're read, narrowing the lines beside it.
  let showMap = $derived(
    (lines.length > 0 || chatCount > 0 || (!!vod && (vod.transcribed !== "none" || vod.chat === "done"))) &&
      mapDuration > 0,
  );

  // The header spans the transcript and the minimap, and stays at the top.
  let headHeight = $state(0);

  // The Timeline layers' matches and the chat events, on the minimap and in chat (marks.svelte.ts).
  $effect(() => void layers.load());
  const marks = new Marks({
    get vods() {
      return chatVods;
    },
    toTimeline,
    get canHaveChat() {
      return canHaveChat;
    },
    get chatCount() {
      return chatCount;
    },
    get overview() {
      return overview;
    },
  });

  /** When row `i` starts; the row of chat before the first line starts at 0. */
  const rowStart = (i: number) => rows[i]?.line?.start ?? Math.max(0, rows[i]?.from ?? 0);

  /** When row `i` ends: when the next one starts, or (the last) when the VOD ends. */
  const rowEnd = (i: number) => (i + 1 < rows.length ? rowStart(i + 1) : Math.max(mapDuration, rowStart(i)));

  // Times and places in the list map to each other evenly within each row:
  // a row's time is spread over its height. So the minimap's box moves as
  // smoothly as the list scrolls, even across a long row (a stretch without
  // speech), and seeking to a moment and reading the box back agree.

  /** The moment at `y` in the list. */
  function timeAtY(y: number): number {
    if (rows.length === 0) return 0;
    const i = rowAtY(y);
    const height = offsets[i + 1] - offsets[i];
    const f = height > 0 ? Math.min(1, Math.max(0, (y - offsets[i]) / height)) : 0;
    return rowStart(i) + f * (rowEnd(i) - rowStart(i));
  }

  /** The row moment `time` is in (clamped). */
  const rowAtTime = (time: number) => lastRowAtOrBefore(rowStart, time);

  /** Where moment `time` is in the list: the inverse of `timeAtY`. */
  function yAtTime(time: number): number {
    if (rows.length === 0) return 0;
    const lo = rowAtTime(time);
    const span = rowEnd(lo) - rowStart(lo);
    const f = span > 0 ? Math.min(1, Math.max(0, (time - rowStart(lo)) / span)) : 0;
    return offsets[lo] + f * (offsets[lo + 1] - offsets[lo]);
  }

  // The part of the VOD in view, for the minimap's box.
  let view = $derived.by(() => {
    if (!showMap || rows.length === 0) return null;
    const top = scrollTop + headHeight - listTop;
    const from = timeAtY(top);
    const to = timeAtY(top + railHeight - headHeight);
    return { from: from / mapDuration, to: Math.min(1, to / mapDuration) };
  });
  $effect(() => {
    const s = scroller;
    // Where the list starts moves with the header, and with a message above the rows.
    void headHeight;
    void lines.length;
    if (!s) return;
    let frame = 0;
    const read = () => {
      scrollTop = s.scrollTop;
      if (topPad) listTop = topPad.getBoundingClientRect().top - s.getBoundingClientRect().top + s.scrollTop;
    };
    const later = () => {
      cancelAnimationFrame(frame);
      frame = requestAnimationFrame(read);
    };
    read();
    s.addEventListener("scroll", later, { passive: true });
    return () => {
      cancelAnimationFrame(frame);
      s.removeEventListener("scroll", later);
    };
  });

  // Seeking scrolls a moment to the middle of the view, as the minimap's box
  // centers on where it's clicked or dragged. Rows far off have estimated
  // heights until they're laid out and measured, and grow when their chat
  // arrives, so for a moment after a seek, each of those re-centers it
  // (`remeasure`, `loadWindows`), until the reader scrolls by themselves.
  const SETTLE_MS = 2000;
  // A search match aimed at is found by itself (`found`), and lit once found.
  let aim: { time: number; until: number; hit?: SearchHit; lit?: boolean } | null = null;
  const aimed = () => aim !== null && performance.now() < aim.until;

  /** Scrolls the moment aimed at to the middle of the view: the chat message
   *  nearest it once its row is laid out (a pixel of the minimap can be many
   *  seconds, so a click a hair past an event still finds it), until then
   *  where the row's time puts it. */
  function place() {
    if (!aim || !scroller) return;
    // The middle of what's in view below the header and the headings pinned there.
    const pin = reserve(rowAtTime(aim.time));
    const middle = headHeight + pin + (railHeight - headHeight - pin) / 2;
    const hit = aim.hit;
    const target = hit ? found(hit) : nearestMessage(aim.time);
    if (target) {
      const box = target.getBoundingClientRect();
      // A match's middle goes to the view's; a message's top, as the minimap places it.
      const top = box.top - scroller.getBoundingClientRect().top + (hit ? box.height / 2 : 0);
      scroller.scrollTop += top - middle;
      if (hit && !aim.lit) {
        aim.lit = true;
        light(target);
      }
    } else {
      scroller.scrollTop = listTop + yAtTime(aim.time) - middle;
    }
  }

  /** The laid-out chat message nearest moment `time` (within 30 s), found in
   *  the chat fetched for its row and the rows beside it, then on the page:
   *  only that one element, so dragging the minimap doesn't search them all. */
  function nearestMessage(time: number): HTMLElement | null {
    if (!showChat || rows.length === 0) return null;
    const at = rowAtTime(time);
    let best: { index: number; time: number } | null = null;
    let gap = 30;
    for (let index = at - 1; index <= at + 1; index++) {
      const row = rows[index];
      for (const m of (row && windows.get(row.key)?.messages) || []) {
        const d = Math.abs(m.time - time);
        if (d < gap) [best, gap] = [{ index, time: m.time }, d];
      }
    }
    if (!best) return null;
    return root.querySelector<HTMLElement>(`.line[data-index="${best.index}"] .chat [data-time="${best.time}"]`);
  }

  /** Search match `hit`, if it's laid out: its chat message (or, with chat
   *  hidden or the message not among those shown, the one nearest its time),
   *  or its line of the transcript. */
  function found(hit: SearchHit): HTMLElement | null {
    if (hit.source === "chat" && showChat) {
      return root.querySelector<HTMLElement>(`.chat [data-id="${hit.id}"]`) ?? nearestMessage(hit.time);
    }
    const index = rowAtTime(hit.time);
    if (!rows[index]?.line) return null;
    return root.querySelector<HTMLElement>(`.line[data-index="${index}"] > .row:not(.chapter-head)`);
  }

  /** Lights `el` for a moment, so the eye finds it (`.lit`). */
  function light(el: HTMLElement) {
    el.classList.remove("lit");
    void el.offsetWidth;
    el.classList.add("lit");
    el.addEventListener("animationend", () => el.classList.remove("lit"), { once: true });
  }

  // A search match double-clicked (App.svelte): to the middle of the view
  // once the rows are in, kept there as they're measured, as a seek is.
  $effect(() => {
    const hit = reveal;
    if (!hit || rows.length === 0 || !scroller) return;
    untrack(() => {
      onrevealed?.();
      // It plays from there (App.svelte), as a click on its line does.
      followFrom(hit.time);
      aim = { time: hit.time, until: performance.now() + SETTLE_MS, hit };
      place();
    });
  });

  function seek(time: number) {
    aim = { time, until: performance.now() + SETTLE_MS };
    place();
  }

  // Scrolling by wheel, touch, keys or the scrollbar is the reader's own: no
  // more re-centering, and no more following (until they ask for it again).
  const SCROLL_KEYS = new Set(["ArrowUp", "ArrowDown", "PageUp", "PageDown", "Home", "End"]);
  $effect(() => {
    const s = scroller;
    if (!s) return;
    const stop = (e: Event) => {
      if (e instanceof KeyboardEvent && !SCROLL_KEYS.has(e.key)) return void (aim = null);
      // A press on the scroller itself, not on what's in it, is on its scrollbar.
      if (e.type === "pointerdown" && e.target !== s) return;
      aim = null;
      if (s.scrollHeight > s.clientHeight) following = false;
    };
    const kinds = ["wheel", "touchstart", "keydown", "pointerdown"] as const;
    for (const kind of kinds) s.addEventListener(kind, stop, { passive: true });
    return () => kinds.forEach((kind) => s.removeEventListener(kind, stop));
  });

  // Following the playhead, from the moment the transcript opens (each VOD
  // or edit opens it afresh, so it always starts following) and whether it
  // plays or not: the line at the playhead stays in view (follow.ts). The view
  // stays still while it moves down the page, and once it nears the bottom
  // (or is out of view, after a jump of the playhead: the timeline, a mark,
  // the keys, the site's own player), the view moves so that it's a third of
  // the way down, with what's coming below it. Moving once a screenful rather
  // than with every line keeps the text still to read. Scrolling away stops it
  // (above, or by any other way the line playing leaves the view, as the
  // minimap does); playing from the transcript, or the button that then
  // shows, starts it again.
  let following = $state(true);
  /** Just after playing from the transcript, the moment played from, until
   *  the player says it's there: the line followed meanwhile, not the one before. */
  const HOLD_MS = 3000;
  let playedFrom = $state<number | null>(null);
  let playedTimer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => () => clearTimeout(playedTimer));
  function followFrom(time: number) {
    following = true;
    playedFrom = time;
    clearTimeout(playedTimer);
    playedTimer = setTimeout(() => (playedFrom = null), HOLD_MS);
  }
  let now = $derived.by(() => {
    if (!mine) return null;
    return playedFrom !== null && Math.abs(cuts.now - playedFrom) > 2 ? playedFrom : cuts.now;
  });
  $effect(() => {
    if (playedFrom !== null && mine && Math.abs(cuts.now - playedFrom) <= 2) playedFrom = null;
  });
  /** The row playing, or -1. */
  let nowRow = $derived(now === null || rows.length === 0 ? -1 : rowAtTime(now));
  /** How tall the view is below the header. */
  const viewHeight = () => Math.max(0, railHeight - headHeight);
  /** Where the line playing is, next to the view: "above", "below" or in "view". */
  let nowPlace = $derived.by(() => {
    if (nowRow < 0) return null;
    const top = scrollTop + headHeight - listTop;
    if (offsets[nowRow + 1] <= top) return "above";
    if (offsets[nowRow] >= top + viewHeight()) return "below";
    return "view";
  });

  /** The view gliding there itself, from `from` to `to`, until `until` at the latest. A
   *  scroll anywhere else meanwhile (back up, say) isn't its own. */
  let ownScroll: { from: number; to: number; until: number } | null = null;
  const ownScrolling = () => {
    if (!ownScroll || !scroller || performance.now() >= ownScroll.until) return false;
    const { from, to } = ownScroll;
    const at = scroller.scrollTop;
    return at >= Math.min(from, to) - 1 && at <= Math.max(from, to) + 1 && Math.abs(at - to) > 1;
  };

  /** Brings row `i` into view a third of the way down, unless it's well in view already. */
  function bring(i: number) {
    if (!scroller || i < 0 || i >= rows.length) return;
    // In view means below the headings pinned at the top, not under them.
    const pin = reserve(i);
    const go = followTarget(offsets[i], offsets[i + 1], viewY() + pin, viewHeight() - pin);
    if (!go) return;
    const target = Math.max(0, listTop + go.top - pin - headHeight);
    // Nearby, it glides there, so the eye can keep its place; far (a jump), it's there at once.
    ownScroll = go.far ? null : { from: scroller.scrollTop, to: target, until: performance.now() + 1000 };
    scroller.scrollTo({ top: target, behavior: go.far ? "auto" : "smooth" });
  }

  // Playing or paused: a move of the playhead while paused (a click on the
  // timeline, a search match, the arrow keys) is followed as one playing is.
  $effect(() => {
    const i = nowRow;
    if (!following || !mine || i < 0) return;
    // Again as rows are measured or the view resizes: where the row is changes.
    void offsets;
    void railHeight;
    void headHeight;
    void listTop;
    // A seek or a search match being centred (`place`) goes first.
    untrack(() => !aimed() && bring(i));
  });

  // The line playing scrolled out of view by anything but following (the
  // minimap, a script, a way of scrolling not caught above): no more following.
  // A jump of the playhead isn't a scroll: following catches up with it.
  let seen = { row: -1, top: 0, place: null as string | null };
  $effect(() => {
    const row = nowRow;
    const place = nowPlace;
    const top = scrollTop;
    untrack(() => {
      const was = seen;
      seen = { row, top, place };
      if (
        following &&
        row === was.row &&
        top !== was.top &&
        was.place === "view" &&
        place !== "view" &&
        !ownScrolling() &&
        // A search match being centred (`reveal`) is the view's own too.
        !(aimed() && aim?.hit)
      )
        following = false;
    });
  });

  /** The follow button: back to the line playing, and following again. */
  function followAgain() {
    following = true;
    bring(nowRow);
  }

  // The moment of the line or chat message under the pointer, marked on the
  // timeline (Scrubber.svelte); a mouse's or a pen's, not a finger's.
  function pointAt(time: number | null) {
    if (!mine) return;
    if (time !== null) cuts.pointed = { time, on: "transcript" };
    else if (cuts.pointed?.on === "transcript") cuts.pointed = null;
  }
  function onHover(e: PointerEvent) {
    if (e.pointerType === "touch") return;
    const el = (e.target as HTMLElement | null)?.closest<HTMLElement>("[data-time]");
    pointAt(el && root.contains(el) ? Number(el.dataset.time) : null);
  }
  $effect(() => () => pointAt(null));
  // The moment pointed at on the timeline, marked on the minimap.
  let pointed = $derived(mine && cuts.pointed?.on === "timeline" ? cuts.pointed.time : null);
</script>

<!-- A part not transcribed: where it is, whether it's asked for, and Transcribe or Stop. -->
{#snippet gapRow(gap: Gap)}
  {@const request = gap.request}
  <div class="row gap" data-time={gap.start}>
    <span class="time"></span>
    <span class="muted" title={request?.status === "error" ? (request.error ?? undefined) : undefined}>
      {formatTime(gap.start)} – {formatTime(gap.end ?? duration)} · {request
        ? requestLabel(request)
        : "not transcribed"} ·
      {#if request}
        <button class="link accent" onclick={() => stop(request.id)}>Stop</button>
      {:else}
        <button class="link accent" onclick={() => transcribe(gap.start, gap.end)}>Transcribe</button>
      {/if}
    </span>
  </div>
{/snippet}

<!-- A chapter's divider: a rule across with its icon and title. Over its first
     line it plays from its start; pinned at the top, it scrolls back there. -->
{#snippet chapterHead(chapter: Mark, pinnedCopy: boolean)}
  <button
    class="row {pinnedCopy ? 'pinned-chapter' : 'chapter-head'}"
    data-time={chapter.start}
    tabindex={pinnedCopy ? -1 : undefined}
    onclick={() => (pinnedCopy ? toRow(dividers[pinned.chapter].row) : play(chapter.start))}
    title={pinnedCopy ? "Back to where this chapter starts" : `Play from ${formatTime(chapter.start)}`}
  >
    <span class="rule lead"></span>
    {#if chapter.icon}<span class="emoji" aria-hidden="true">{chapter.icon}</span>{/if}
    <span class="title" class:untitled={!chapter.text}>{chapter.text ?? "Untitled chapter"}</span>
    <span class="rule"></span>
  </button>
{/snippet}

<!-- Who says the lines below, and when the first is: their picture, or their
     initial; their name links to their channel, or to their own stream at the
     moment the lines start when it ran alongside (castLink.svelte.ts). Pinned at
     the top, the rest of it scrolls back to where their lines start.
     Lines no one is named for, where others are, are Unknown's: a person's outline, no link. -->
{#snippet speakerHead(group: GroupHead, pinnedCopy: boolean)}
  {@const color = group.unknown ? null : speakerColor(cast, group.name)}
  {@const member = group.unknown ? undefined : cast[castIndex(cast, group.name)]}
  {@const to = castLink(member, vod, group.start)}
  {@const label = group.unknown ? group.name : speakerName(member, group.name)}
  <span class="avatar-slot">
    <CastDisc name={label} {color} {member} {to} anonymous={group.unknown} />
  </span>
  <span class="who">
    {#if to}
      <a
        class="speaker channel"
        class:unknown={!color}
        style:color
        href={to.href}
        title={to.title}
        tabindex={pinnedCopy ? -1 : undefined}
        onclick={(e) => (e.preventDefault(), to.follow())}
        >{label}{#if to.outside}<span class="out"><Icon name="external" size={10} /></span>{/if}</a
      >
    {:else}
      <span class="speaker" class:unknown={!color} style:color>{label}</span>
    {/if}
    <span class="head-time">{formatTime(group.start)}</span>
  </span>
{/snippet}

<div class="layout">
  <TranscriptHead
    {vod}
    {cast}
    stretch={runningStretch}
    timelineVod={chatVods[0] ?? null}
    lineCount={lines.length}
    {chatCount}
    chatRead={overview !== null}
    {chatProgress}
    bind:chatError
    {hasChat}
    {showChat}
    {noChat}
    ontoggle={toggleChat}
    emotes={marks.emotes}
    counts={marks.counts}
    eventCounts={marks.eventCounts}
    bind:height={headHeight}
    {tabs}
  />
  <div
    class="cols"
    class:away={tabs?.open === "annotations"}
    id={tabs ? "transcript-panel" : undefined}
    role={tabs ? "tabpanel" : undefined}
    aria-labelledby={tabs ? "tab-transcript" : undefined}
  >
    <!-- Hover only marks the timeline: the lines and messages are the buttons. -->
    <!-- svelte-ignore a11y_no_static_element_interactions -->
    <div
      class="transcript"
      id="transcript"
      class:with-chat={showChat}
      class:dragging={drag}
      bind:this={root}
      bind:clientWidth={width}
      onpointermove={onHover}
      onpointerleave={() => pointAt(null)}
    >
      <!-- The headings pinned at the top of the view (pinned.ts): a copy for the
           eye and the pointer. Screen readers and the keyboard have the
           headings themselves, each over its lines in the list. -->
      <div class="pinned-anchor" style:top="{headHeight}px">
        {#if pinned.height > 0}
          <div class="pinned" aria-hidden="true" style:height="{pinned.height}px">
            {#if pinnedGroup}
              {@const start = groups[pinned.group].row}
              <div class="pinned-part" style:transform="translateY({pinned.speakerTop}px)">
                <!-- svelte-ignore a11y_click_events_have_key_events, a11y_no_static_element_interactions -->
                <div
                  class="pinned-speaker"
                  data-time={pinnedGroup.start}
                  title="Back to where these lines start"
                  onclick={(e) => !(e.target as Element).closest("a") && toRow(start)}
                >
                  {@render speakerHead(pinnedGroup, true)}
                </div>
              </div>
            {/if}
            {#if pinnedChapters.length}
              <div class="pinned-part" style:transform="translateY({pinned.chapterTop}px)">
                {#each pinnedChapters as chapter (chapter.id)}
                  {@render chapterHead(chapter, true)}
                {/each}
              </div>
            {/if}
          </div>
        {/if}
      </div>
      {#if error}
        <p class="error-text">{error}</p>
      {/if}
      {#if read && lines.length === 0 && gaps.length === 0}
        <p class="muted">
          {#if !vod}
            Nothing is said in this edit.
          {:else if vod.transcribed === "none" && vod.status === "none"}
            Not transcribed yet.
            <button class="link accent" onclick={() => transcribe(0, null)}>Transcribe the whole VOD</button> ·
            <button class="link accent" bind:this={fromButton} onclick={transcribeFromHere}>Transcribe from here</button
            >
            <TranscribeFrom
              bind:open={fromMenu}
              anchor={fromButton}
              {vod}
              from={fromHere}
              onerror={(e) => (error = e)}
            />
          {:else if vod.transcribed === "all"}
            No speech was found in this VOD.
          {:else}
            The transcript appears here as it is produced.
          {/if}
        </p>
      {/if}
      <div bind:this={topPad} style:height="{offsets[range[0]]}px"></div>
      {#each held ? [] : rows.slice(range[0], range[1]) as row, k (row.key)}
        {@const key = row.key}
        {@const win = windows.get(key)}
        <div
          class="line"
          data-index={range[0] + k}
          use:measure
          class:empty={!row.line && !row.gap && !win?.total}
          class:picked={row.line && picked(row)}
          class:now={row.line && range[0] + k === nowRow}
        >
          {#each heads.get(key) ?? [] as chapter (chapter.id)}
            {@render chapterHead(chapter, false)}
          {/each}
          {#if speakerHeads[range[0] + k]}
            {@const group = speakerHeads[range[0] + k]!}
            <div class="group-head" data-time={group.start}>
              {@render speakerHead(group, false)}
            </div>
          {/if}
          {#if row.line}
            {@const line = row.line}
            <button
              class="row"
              data-time={line.start}
              use:pressToDrag={{
                press: (e) => onRowDown(e, line),
                start: onDragMove,
                move: onDragMove,
                end: onDragEnd,
              }}
              onclick={(e) => onRowClick(e, line)}
            >
              <span class="time">{formatTime(line.start)}</span>
              <span>{line.text}</span>
            </button>
          {:else if row.gap}
            {@render gapRow(row.gap)}
          {:else}
            <span></span>
          {/if}
          {#if showChat}
            <div class="chat">
              {#each win?.messages ?? [] as message (message.id)}
                <ChatMessage
                  {message}
                  platform={message.platform ?? vod?.platform ?? ""}
                  highlight={marks.highlightOf(message.id)}
                  words={marks.words}
                  onplay={play}
                />
              {/each}
              {#if win && win.total > win.messages.length}
                <button class="link accent more" onclick={() => expand(row)} disabled={loading.has(key)}
                  >{(win.total - win.messages.length).toLocaleString()} more</button
                >
              {/if}
            </div>
          {/if}
        </div>
      {/each}
      <div style:height="{offsets[rows.length] - offsets[range[1]]}px"></div>
      {#if !following && nowRow >= 0}
        <!-- At the bottom of the view, over the lines: back to the line playing. -->
        <div class="follow-anchor">
          <button class="follow" onclick={followAgain} title="Scroll to the line playing and keep it in view">
            {#if nowPlace === "above"}<Icon name="chevron-up" size={14} />{:else if nowPlace === "below"}<Icon
                name="chevron-down"
                size={14}
              />{/if}
            Follow along
          </button>
        </div>
      {/if}
    </div>
    {#if showMap}
      <div class="rail" style:top="{headHeight}px" style:height="{Math.max(0, railHeight - headHeight)}px">
        <Minimap
          times={lineTimes}
          histogram={chatCount > 0 ? overview?.histogram : undefined}
          duration={mapDuration}
          {view}
          label={mapLabel}
          overlays={marks.overlays}
          events={marks.eventMarks}
          eventColor={eventColor(vod?.platform ?? "local")}
          {pointed}
          onseek={seek}
          onpoint={pointAt}
        />
      </div>
    {/if}
  </div>
</div>

<style>
  .cols {
    display: flex;
    align-items: flex-start;
  }

  .cols.away {
    display: none;
  }

  .rail {
    position: sticky;
    user-select: none;
    top: 0;
    flex: none;
    width: 48px;
    margin-left: auto;
    padding: 8px 6px;
    box-sizing: border-box;
    border-left: 1px solid var(--border);
  }

  .transcript {
    flex: 1 1 auto;
    min-width: 0;
    padding: 8px 20px 32px;
    max-width: 960px;
    /* toggleChat keeps the view in place itself; the browser's own scroll
       anchoring (Firefox, Chromium) would move it a second time. */
    overflow-anchor: none;
  }

  .transcript.with-chat {
    max-width: 1320px;
  }

  /* The margin over a row's divider is inside the row, so the row's measured
     height (and where the next row starts) counts it. */
  .line {
    display: flow-root;
  }

  /* Columns apart; a divider or a group's head over the line sits as close
     as it does without chat. */
  .with-chat .line {
    display: grid;
    grid-template-columns: minmax(0, 3fr) minmax(0, 2fr);
    gap: 0 16px;
  }

  .chat {
    border-left: 1px solid var(--border);
    padding: 2px 0 2px 8px;
    min-width: 0;
  }

  .chat:empty {
    padding: 0 0 0 8px;
  }

  .more {
    margin: 0 8px 4px;
  }

  /* The row for chat before the first line, when there's none. */
  .line.empty {
    display: none;
  }

  .dragging {
    user-select: none;
    cursor: ns-resize;
  }

  /* Over a group of one speaker's lines, from an annotations import: a disc
     with their initial where their picture goes, over the lines' times, then
     their name in their colour in the cast and when the first line is. As
     wide as the row, as a chapter's divider is, so the chat beside the lines
     stays beside them. */
  .group-head,
  .pinned-speaker {
    grid-column: 1 / -1;
    display: flex;
    align-items: center;
    gap: 14px;
    padding: 8px 10px 0;
    margin: 0 -10px;
    line-height: 1.5;
  }

  .avatar-slot {
    flex: none;
    display: flex;
    justify-content: flex-end;
    min-width: 56px;
  }

  .who {
    display: flex;
    align-items: baseline;
    gap: 8px;
    min-width: 0;
  }

  .speaker {
    font-weight: 600;
    overflow-wrap: anywhere;
  }

  .speaker.unknown {
    color: var(--text);
  }

  .speaker.channel {
    text-decoration: none;
  }

  .speaker.channel:hover {
    text-decoration: underline;
  }

  /* The sign that the name's link leaves the app (castLink.svelte.ts `outside`). */
  .out {
    margin-left: 3px;
    opacity: 0.7;
  }

  .head-time {
    flex: none;
    font-family: var(--mono);
    font-size: 12px;
    color: var(--muted);
    font-variant-numeric: tabular-nums;
  }

  /* The headings pinned at the top of the view, over the lines going under
     them: in the list's place, as wide as its rows, opaque. A divider is over
     the speaker's head it pushes out with it. It takes no room in the list. */
  .pinned-anchor {
    position: sticky;
    z-index: 1;
    height: 0;
  }

  .pinned {
    position: absolute;
    top: 0;
    left: -20px;
    right: -20px;
    overflow: hidden;
  }

  .pinned-part {
    position: absolute;
    top: 0;
    left: 0;
    right: 0;
    padding: 0 20px;
    background: var(--bg);
  }

  .pinned-speaker {
    cursor: pointer;
  }

  /* A chapter's start: a rule across with its icon and title, muted, over its first line. */
  .chapter-head,
  .pinned-chapter {
    grid-column: 1 / -1;
    align-items: center;
    gap: 8px;
    margin-top: 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--muted);
  }

  .chapter-head .rule,
  .pinned-chapter .rule {
    flex: 1;
    height: 1px;
    background: var(--border);
  }

  /* As far in as the lines' times, so the title starts where their text does. */
  .chapter-head .rule.lead,
  .pinned-chapter .rule.lead {
    flex: none;
    width: 62px;
  }

  .chapter-head .emoji,
  .pinned-chapter .emoji {
    font-family: "Apple Color Emoji", "Segoe UI Emoji", "Noto Color Emoji", sans-serif;
    font-size: 13px;
  }

  .chapter-head .title,
  .pinned-chapter .title {
    overflow-wrap: anywhere;
  }

  /* A part not transcribed: it says so, and isn't a line to click. */
  .row.gap:hover {
    background: none;
  }

  .chapter-head .untitled,
  .pinned-chapter .untitled {
    font-weight: 400;
    font-style: italic;
  }

  /* A search match brought into view (`light`): the accent, fading. */
  .transcript :global(.lit) {
    animation: -global-transcript-lit 1.6s ease-out;
  }

  @keyframes -global-transcript-lit {
    from {
      background: color-mix(in srgb, var(--accent) 24%, transparent);
    }
  }

  /* The line playing. */
  .line.now > :global(.row:not(.chapter-head)) {
    background: color-mix(in srgb, var(--accent) 9%, transparent);
    box-shadow: inset 2px 0 0 var(--accent);
  }

  /* Sticks to the bottom of the view; takes no room in the list. */
  .follow-anchor {
    position: sticky;
    bottom: 0;
    height: 0;
  }

  .follow {
    position: absolute;
    bottom: 16px;
    left: 50%;
    transform: translateX(-50%);
    gap: 4px;
    padding: 4px 12px;
    border-radius: 999px;
    font-size: 12.5px;
    color: var(--accent);
    box-shadow: var(--shadow);
  }

  .line.picked > :global(.row:not(.chapter-head)) {
    background: color-mix(in srgb, var(--pick) 16%, transparent);
  }
</style>
