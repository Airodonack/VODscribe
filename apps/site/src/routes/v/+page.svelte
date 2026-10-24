<script lang="ts">
  // The VODs with annotations on the download host (annotations/index.json),
  // each linking to its page; and, for a file without a page, the same
  // drop and open-a-file controls as a VOD's page (VodView.svelte).
  import { onMount } from "svelte";
  import { formatTime, platformName } from "desktop/lib/platform.ts";
  import VodView from "#lib/VodView.svelte";
  import { DOWNLOAD_HOST } from "#lib/downloads.ts";
  import { indexEntries, pageOf, type IndexEntry } from "#lib/tauri/annotated.ts";

  let entries = $state<IndexEntry[] | null>(null);
  let error = $state<string | null>(null);

  onMount(async () => {
    try {
      const res = await fetch(`${DOWNLOAD_HOST}/annotations/index.json`);
      if (res.status === 404 || res.status === 403) return void (entries = []);
      if (!res.ok) throw new Error(`The list of VODs couldn't be loaded (${res.status}).`);
      entries = indexEntries(await res.json());
    } catch (e) {
      // A failed request, or an answer the browser kept from the page (see VodView.svelte).
      error = e instanceof TypeError ? "The list of VODs couldn't be loaded." : (e as Error).message;
    }
  });
</script>

<svelte:head>
  <title>VODs with annotations · VODscribe</title>
  <meta name="description" content="VODs with their transcripts, chat and chapters, to read and play on VODscribe." />
</svelte:head>

<VodView idle>
  <div class="index">
    <h1>VODs with annotations</h1>
    {#if error}
      <p class="error-text">{error}</p>
    {:else if entries === null}
      <p class="muted">Loading…</p>
    {:else if entries.length === 0}
      <p class="muted">There are no VODs here yet.</p>
    {:else}
      <ul>
        {#each entries as e (`${e.platform}/${e.id}`)}
          <li>
            <a href={pageOf(e.platform, e.id)}>
              <span class="title">{e.title || e.id}</span>
              <span class="muted">
                {platformName[e.platform] ?? e.platform}{#if e.channel}<span class="sep">·</span
                  >{e.channel}{/if}{#if e.duration}<span class="sep">·</span>{formatTime(e.duration)}{/if}
              </span>
            </a>
          </li>
        {/each}
      </ul>
    {/if}
    <p class="muted small">
      Have an annotations file? Drop it anywhere on this page (with its .chat.json if you have it), or use Open a file.
      It's read here, in your browser, and isn't uploaded.
    </p>
  </div>
</VodView>

<style>
  .index {
    max-width: 720px;
    margin: 32px auto;
  }

  h1 {
    font-size: 28px;
    letter-spacing: -0.02em;
    margin: 0 0 16px;
  }

  ul {
    list-style: none;
    margin: 0 0 20px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
  }

  li + li {
    border-top: 1px solid var(--border);
  }

  a {
    display: flex;
    flex-direction: column;
    gap: 2px;
    padding: 12px 16px;
    color: var(--text);
    text-decoration: none;
  }

  a:hover {
    background: var(--surface-2);
  }

  .title {
    font-weight: 600;
  }

  .muted {
    font-size: 13px;
  }

  .sep {
    margin: 0 6px;
  }

  .small {
    font-size: 13px;
  }
</style>
