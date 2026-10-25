<script lang="ts">
  // A VOD with annotations, in the app's own view (Demo.svelte): its
  // transcript, chat and player. Its files come from `source` (the download
  // host's annotations/<platform>/<id>.json and .chat.json beside it, or
  // ?src=<url>), or from files dropped on the page or picked, which are read
  // here in the browser and never uploaded. The VOD pages (routes/v/[platform]/[id])
  // show it with their source, as the whole page: the app's view fills the
  // window under the site's bar (+layout.svelte), whose line carries the note
  // and Open a file (siteBar.svelte.ts), with the VOD's annotations
  // (AnnotationsPane.svelte) in the library pane's place, and Review, which
  // adds a verdict and a note to each of them (review.svelte.ts). The index
  // (routes/v) shows it `idle`,
  // its list (`children`) in place in a normal page, until a file is opened.
  //
  // Phones (under 821 px) get the VOD's title, the platform's player and a
  // note that the rest needs a bigger screen. A proper phone layout is for
  // later.
  import { onDestroy, onMount, untrack, type Snippet } from "svelte";
  import { parseAnnotations } from "desktop/lib/annotations.ts";
  import Demo from "#lib/Demo.svelte";
  import AnnotationsPane from "desktop/lib/AnnotationsPane.svelte";
  import { chapterRows, highlightRows } from "desktop/lib/annotationRows.ts";
  import { cuts } from "desktop/lib/cuts.svelte.ts";
  import ReviewControls from "#lib/ReviewControls.svelte";
  import { Review } from "#lib/review.svelte.ts";
  import { siteBar } from "#lib/siteBar.svelte.ts";
  import { annotated } from "#lib/tauri/annotations.ts";
  import type { EmbeddedPlayer } from "desktop/lib/embed.ts";
  import type { Vod } from "desktop/lib/api.ts";
  import type { PaneRow } from "desktop/lib/annotationRows.ts";
  import { pickDropped, type Source } from "#lib/tauri/annotated.ts";
  import { chatArrived, closeAnnotations, openAnnotations } from "#lib/tauri/annotations.ts";

  let {
    source = null,
    idle = false,
    children,
  }: { source?: Source | null; idle?: boolean; children?: Snippet } = $props();

  type View =
    | { kind: "idle" }
    | { kind: "loading" }
    | { kind: "missing" }
    | { kind: "error"; message: string }
    | { kind: "open"; id: number; title: string; channel: string; platform: string; from: string };
  // Whether it's idle is fixed for the page (the index's, or a VOD's).
  let view = $state<View>({ kind: untrack(() => idle) ? "idle" : "loading" });

  /** Opens the annotations in `text` (named `name`), with the chat in
   *  `chat` (null: none; undefined: still to come). The VOD's id, or null
   *  with the reason shown. */
  function open(text: string, name: string, chat: string | null | undefined, from: string): number | null {
    try {
      let value: unknown;
      try {
        value = JSON.parse(text);
      } catch {
        throw new Error(`${name} isn't a JSON file.`);
      }
      const annotations = parseAnnotations(value);
      let chatValue: unknown;
      if (typeof chat === "string")
        try {
          chatValue = JSON.parse(chat);
        } catch {
          throw new Error("The chat file isn't a JSON file.");
        }
      const id = openAnnotations(annotations, chat === undefined ? undefined : chat === null ? null : chatValue);
      const s = annotations.source;
      // The rows exported are the pane's: the marks open, once they're this VOD's.
      review = new Review(s.platform, s.id, () =>
        cuts.vodId === id ? [...chapterRows(cuts.all), ...highlightRows(cuts.all)] : [],
      );
      view = { kind: "open", id, title: s.title || s.id, channel: s.channel ?? "", platform: s.platform, from };
      return id;
    } catch (e) {
      view = { kind: "error", message: (e as Error).message };
      return null;
    }
  }

  async function fromUrls(source: Source) {
    view = { kind: "loading" };
    const where = source.custom ? source.annotations : "VODscribe";
    let res: Response;
    try {
      res = await fetch(source.annotations);
    } catch {
      // A failed request, or one the browser kept from the page: the page's
      // policy reaches only this site and the download host, and a host's
      // answer without CORS headers (a missing file's, say) can't be read.
      view = {
        kind: "error",
        message: source.custom
          ? `Couldn't load ${source.annotations}. This page can load files from this site and dl.vodscribe.com; drop others on it instead.`
          : "Couldn't load this VOD's annotations: they may not be on VODscribe, or the connection failed.",
      };
      return;
    }
    if (res.status === 404 || res.status === 403) return void (view = { kind: "missing" });
    if (!res.ok) return void (view = { kind: "error", message: `Couldn't load the annotations (${res.status}).` });
    const id = open(await res.text(), "The annotations file", undefined, where);
    if (id === null) return;
    // The chat comes after: the transcript and player are usable meanwhile.
    try {
      const chat = await fetch(source.chat);
      if (chat.ok && !chat.headers.get("content-type")?.startsWith("text/html")) {
        let value: unknown;
        try {
          value = await chat.json();
        } catch {
          return chatArrived(id, null, "The chat file isn't a JSON file.");
        }
        chatArrived(id, value);
      } else
        chatArrived(id, null, chat.status === 404 || chat.ok ? undefined : `Couldn't load the chat (${chat.status}).`);
    } catch {
      chatArrived(id, null, "Couldn't reach the chat file.");
    }
  }

  async function fromFiles(list: FileList | File[]) {
    const files = [...list];
    if (!files.length) return;
    const picked = pickDropped(files);
    if ("error" in picked) return void (view = { kind: "error", message: picked.error });
    const [text, chat] = await Promise.all([picked.annotations.text(), picked.chat?.text() ?? null]);
    open(text, picked.annotations.name, chat, picked.annotations.name);
  }

  let review = $state<Review | null>(null);
  // Review shows in the bar on a VOD's own page, once it's open.
  $effect(() => {
    siteBar.review = !idle && view.kind === "open" ? review : null;
  });

  // The same width the demo hides itself under (Demo.svelte).
  let wide = $state(true);
  onMount(() => {
    const query = matchMedia("(min-width: 821px)");
    const sync = () => (wide = query.matches);
    sync();
    query.addEventListener("change", sync);
    if (source) fromUrls(source);
    else if (!idle) view = { kind: "missing" };
    if (!idle) siteBar.pick = () => picker?.click();
    return () => query.removeEventListener("change", sync);
  });
  onDestroy(() => {
    closeAnnotations();
    siteBar.pick = null;
    siteBar.note = "";
    siteBar.review = null;
  });

  $effect(() => {
    siteBar.note =
      idle || view.kind !== "open"
        ? ""
        : `From ${view.from}. Transcribing and clipping VODs of your own needs the app.`;
  });

  // The platform's player on a phone: the app's own embed, made as its player
  // panel makes it, in the element this action is given.
  function embed(host: HTMLElement) {
    let player: EmbeddedPlayer | undefined;
    let gone = false;
    const vod = annotated()?.vod;
    if (vod)
      import("desktop/lib/embed.ts").then(({ createPlayer }) =>
        createPlayer(host, vod, 0, false, (message) => (embedError = message))?.then((p) =>
          gone ? p.destroy() : (player = p),
        ),
      );
    return {
      destroy() {
        gone = true;
        player?.destroy();
      },
    };
  }
  let embedError = $state<string | null>(null);

  // Files dropped anywhere on the page open here (the demo's own drop handling is off).
  let dragging = $state(false);
  const carriesFiles = (e: DragEvent) => !!e.dataTransfer?.types.includes("Files");
  function onDragOver(e: DragEvent) {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    dragging = true;
  }
  function onDrop(e: DragEvent) {
    if (!carriesFiles(e)) return;
    e.preventDefault();
    dragging = false;
    if (e.dataTransfer) fromFiles(e.dataTransfer.files);
  }

  let picker = $state<HTMLInputElement>();
</script>

<!-- The VOD page's left pane: its annotations, and their review when it's on. -->
{#snippet pane({ vod, onplay }: { vod: Vod | undefined; onplay: (time: number) => void })}
  <AnnotationsPane
    {vod}
    {onplay}
    review={review?.on ? reviewRow : undefined}
    reviewVod={review?.on ? reviewVod : undefined}
  />
{/snippet}

{#snippet reviewRow(row: PaneRow)}
  {#if review}<ReviewControls {review} {row} />{/if}
{/snippet}

{#snippet reviewVod()}
  {#if review}
    {@const r = review}
    <label class="vod-note">
      <span class="eyebrow">Review</span>
      <span class="muted small">{r.count} judged. Notes on the whole VOD:</span>
      <textarea rows="3" value={r.note} oninput={(e) => (r.note = e.currentTarget.value)}></textarea>
    </label>
  {/if}
{/snippet}

<!-- What a tab and a bookmark read. A shared link's preview (og: tags) needs
     them in the HTML as served, which this page, made in the browser, can't
     give: that's for the server to add later. -->
<svelte:head>
  {#if view.kind === "open"}
    <title>{view.title}{view.channel ? ` · ${view.channel}` : ""} · VODscribe</title>
    <meta
      name="description"
      content="{view.title}{view.channel
        ? `, from ${view.channel}`
        : ''}: the VOD with its transcript, chat and chapters, on VODscribe."
    />
  {:else if !idle}
    <title>A VOD with annotations · VODscribe</title>
  {/if}
</svelte:head>

<svelte:window
  ondragover={onDragOver}
  ondragleave={(e) => e.relatedTarget === null && (dragging = false)}
  ondrop={onDrop}
/>

<main class:dragging class:page={idle}>
  {#if idle}
    <div class="bar">
      <p class="muted small">
        {#if view.kind === "open"}
          A VOD with annotations, from {view.from}. Transcribing and clipping VODs of your own needs the app.
        {:else}
          VODs with annotations
        {/if}
      </p>
      <button class="small" onclick={() => picker?.click()}>Open a file…</button>
    </div>
  {/if}
  <input
    bind:this={picker}
    type="file"
    accept=".json,application/json"
    multiple
    hidden
    onchange={(e) => {
      const input = e.currentTarget;
      if (input.files) fromFiles(input.files).finally(() => (input.value = ""));
    }}
  />

  {#if view.kind === "open"}
    {#if idle || wide}
      {#key view.id}
        <Demo openVod={view.id} fill={!idle} leftPane={idle ? undefined : pane} />
      {/key}
    {:else}
      <div class="phone">
        <h1>{view.title}</h1>
        {#if view.channel}<p class="muted">{view.channel}</p>{/if}
        {#key view.id}
          <div class="embed" use:embed></div>
        {/key}
        {#if embedError}<p class="error-text small">{embedError}</p>{/if}
        <p class="caption">
          <strong>This page needs a bigger screen for the transcript and chapters.</strong>
          <span class="muted">Open it on a computer to read and search the VOD.</span>
        </p>
      </div>
    {/if}
  {:else if view.kind === "idle"}
    {@render children?.()}
  {:else}
    <div class="message">
      {#if view.kind === "loading"}
        <p class="muted">Loading the VOD…</p>
      {:else if view.kind === "missing"}
        <h1>No VOD here</h1>
        <p class="muted">
          There are no annotations for this VOD on VODscribe. Check the link, or open an annotations file from your
          computer: it's read here, in your browser, and isn't uploaded.
        </p>
      {:else}
        <h1>This VOD can't be shown</h1>
        <p class="error-text">{view.message}</p>
      {/if}
      {#if view.kind !== "loading"}
        <p class="muted small">
          Drop an annotations file anywhere on this page (with its .chat.json if you have it), or
          <button class="link" onclick={() => picker?.click()}>pick one</button>.
        </p>
      {/if}
    </div>
  {/if}
</main>

<style>
  /* A VOD's page: all that is left under the site's bar. */
  main {
    flex: 1;
    min-height: 0;
    overflow: hidden;
  }

  main.page {
    flex: none;
    overflow: visible;
    max-width: 1600px;
    margin: 0 auto 32px;
    padding: 0 24px;
  }

  main.dragging {
    outline: 2px dashed var(--accent);
    outline-offset: -4px;
  }

  main.page.dragging {
    outline-offset: 4px;
    border-radius: 12px;
  }

  .phone {
    height: 100%;
    overflow: auto;
    padding: 16px;
    box-sizing: border-box;
  }

  .phone h1 {
    font-size: 20px;
    line-height: 1.25;
    margin: 0 0 4px;
  }

  .phone p {
    margin: 0 0 12px;
  }

  .embed {
    aspect-ratio: 16 / 9;
    margin: 12px 0;
    background: #000;
    border-radius: 8px;
    overflow: hidden;
  }

  .caption {
    text-align: center;
    font-size: 14px;
  }

  .caption strong {
    display: block;
  }

  .bar {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 12px;
    margin-bottom: 10px;
  }

  .bar p {
    margin: 0;
  }

  .small {
    font-size: 13px;
  }

  .vod-note {
    display: flex;
    flex-direction: column;
    gap: 4px;
  }

  .vod-note textarea {
    resize: vertical;
    padding: 5px 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: inherit;
    font: inherit;
    font-size: 12px;
  }

  .message {
    max-width: 560px;
    padding: 0 16px;
    margin: 12vh auto;
    text-align: center;
  }

  .message h1 {
    font-size: 28px;
    letter-spacing: -0.02em;
    margin: 0 0 12px;
  }

  .link {
    padding: 0;
    border: 0;
    background: none;
    color: var(--accent);
    font: inherit;
    cursor: pointer;
    text-decoration: underline;
  }
</style>
