<script lang="ts">
  // The page's demo: the app's view (Demo.svelte) opened on the demo's NASA
  // VOD, with its VODscription from the download host
  // (annotations/youtube/<NASA_ID>.json, the chat beside it if there is one,
  // as a VOD's page loads them: VodView.svelte), or from `?src=<url>` as there.
  // Until the host has the file, or when it can't be read, it opens the same
  // VOD from the demo's own transcripts (static/demo), without the chapters.
  // Phones get a screenshot instead (+page.svelte), so nothing loads until
  // the window is wide enough to show it.
  import { onDestroy, onMount } from "svelte";
  import { parseAnnotations } from "desktop/lib/annotations.ts";
  import Demo from "#lib/Demo.svelte";
  import { pickSource } from "#lib/tauri/annotated.ts";
  import { chatArrived, closeAnnotations, openAnnotations } from "#lib/tauri/annotations.ts";

  /** The NASA VOD: its YouTube id, and its id in static/demo/vods.json. */
  const NASA_ID = "_43Ei9eQVww";
  const NASA_DEMO_VOD = 2;

  let vod = $state<number | null>(null);

  async function load() {
    const source = pickSource("youtube", NASA_ID, new URLSearchParams(location.search), location.href);
    try {
      const res = source && (await fetch(source.annotations));
      if (!source || !res?.ok) throw new Error(`no VODscription (${res?.status ?? "no source"})`);
      const id = openAnnotations(parseAnnotations(await res.json()), undefined);
      vod = id;
      // The chat after, as on a VOD's page; a VOD without one has none.
      try {
        const chat = await fetch(source.chat);
        const html = chat.headers.get("content-type")?.startsWith("text/html");
        chatArrived(id, chat.ok && !html ? await chat.json() : null);
      } catch {
        chatArrived(id, null);
      }
    } catch {
      // Not on the host yet, unreachable, or not readable: the demo's own copy of the VOD.
      closeAnnotations();
      vod = NASA_DEMO_VOD;
    }
  }

  onMount(() => {
    const wide = matchMedia("(min-width: 821px)");
    let started = false;
    const start = () => {
      if (wide.matches && !started) {
        started = true;
        load();
      }
    };
    start();
    wide.addEventListener("change", start);
    return () => wide.removeEventListener("change", start);
  });
  // The home page's demo, when the visitor goes there, is the demo's VODs again.
  onDestroy(closeAnnotations);
</script>

{#if vod !== null}
  {#key vod}
    <Demo openVod={vod} library={false} />
  {/key}
{:else}
  <div class="waiting muted">Loading the demo…</div>
{/if}

<style>
  .waiting {
    display: grid;
    place-items: center;
    height: min(760px, 88vh);
    border: 1px solid var(--border);
    border-radius: 12px;
  }
</style>
