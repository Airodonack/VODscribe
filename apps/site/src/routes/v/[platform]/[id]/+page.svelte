<script lang="ts">
  // A VOD with annotations, in the app's own view (Demo.svelte): its
  // transcript, chat and player. Its files come from the download host
  // (annotations/<platform>/<id>.json and .chat.json beside it), from
  // ?src=<url>, or from files dropped on the page or picked, which are read
  // here in the browser and never uploaded.
  import { onDestroy, onMount } from "svelte";
  import { page } from "$app/state";
  import { parseAnnotations } from "desktop/lib/annotations.ts";
  import Demo from "#lib/Demo.svelte";
  import { pickDropped, pickSource, type Source } from "#lib/tauri/annotated.ts";
  import { chatArrived, closeAnnotations, openAnnotations } from "#lib/tauri/annotations.ts";

  type View =
    | { kind: "loading" }
    | { kind: "missing" }
    | { kind: "error"; message: string }
    | { kind: "open"; id: number; title: string; from: string };
  let view = $state<View>({ kind: "loading" });

  const platform = $derived(page.params.platform ?? "");
  const vodId = $derived(page.params.id ?? "");

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
      view = { kind: "open", id, title: annotations.source.title || annotations.source.id, from };
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

  onMount(() => {
    const source = pickSource(platform, vodId, page.url.searchParams, page.url.href);
    if (source) fromUrls(source);
    else view = { kind: "missing" };
  });
  onDestroy(closeAnnotations);

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

<svelte:head>
  <title>{view.kind === "open" ? `${view.title} · VODscribe` : "A VOD with annotations · VODscribe"}</title>
</svelte:head>

<svelte:window
  ondragover={onDragOver}
  ondragleave={(e) => e.relatedTarget === null && (dragging = false)}
  ondrop={onDrop}
/>

<main class:dragging>
  <div class="bar">
    <p class="muted small">
      {#if view.kind === "open"}
        A VOD with annotations, from {view.from}. Transcribing and clipping VODs of your own needs the app.
      {:else}
        A VOD with annotations
      {/if}
    </p>
    <button class="small" onclick={() => picker?.click()}>Open a file…</button>
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
  </div>

  {#if view.kind === "open"}
    {#key view.id}
      <Demo openVod={view.id} fill />
    {/key}
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
  main {
    max-width: 1600px;
    margin: 0 auto 32px;
    padding: 0 24px;
  }

  main.dragging {
    outline: 2px dashed var(--accent);
    outline-offset: 4px;
    border-radius: 12px;
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

  .message {
    max-width: 560px;
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
