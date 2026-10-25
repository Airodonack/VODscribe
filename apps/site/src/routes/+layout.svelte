<script lang="ts">
  import "desktop/app.css";
  import "../site.css";
  import Logo from "desktop/lib/Logo.svelte";
  import { page } from "$app/state";
  import { AUTHOR, SOURCE, SUPPORT } from "#lib/links.js";
  import { siteBar } from "#lib/siteBar.svelte.ts";

  let { children } = $props();
  // A VOD's page is the player itself: a one-line bar, the view filling the
  // rest of the window, no footer and no page scroll (VodView.svelte).
  const full = $derived(page.route.id === "/v/[platform]/[id]");
</script>

<div class="shell" class:full>
  <header class="top">
    <a class="brand" href="/">
      <Logo size={28} tile />
      <span>VODscribe</span>
    </a>
    {#if full && siteBar.note}
      <p class="note muted">{siteBar.note}</p>
    {/if}
    <nav>
      {#if full && siteBar.review}
        {@const review = siteBar.review}
        <!-- A verdict and a note on each chapter and highlight, kept in this browser (review.svelte.ts). -->
        <span class="review-tools">
          <button
            class="open"
            class:on={review.on}
            aria-pressed={review.on}
            onclick={() => (review.on = !review.on)}
            title="Say whether each chapter and highlight is right, with notes, to export">Review</button
          >
          {#if review.on}
            <button class="open" onclick={() => review.export()} title="Save what's said as a JSONL file"
              >Export verdicts</button
            >
            <button
              class="open"
              onclick={() => confirm("Clear every verdict and note on this VOD?") && review.clear()}
              title="Forget every verdict and note on this VOD">Clear</button
            >
          {/if}
        </span>
      {/if}
      {#if full && siteBar.pick}
        <button class="open" onclick={siteBar.pick}>Open a file…</button>
      {/if}
      <a class="download-link" href="/#download">Download</a>
      {#if !full}
        <a href={SUPPORT} target="_blank" rel="noopener">Support on Ko-fi</a>
      {/if}
    </nav>
  </header>

  {@render children()}

  {#if !full}
    <footer>
      <span>Made with <span class="heart">❤︎</span> by {AUTHOR}</span>
      <a href={SUPPORT} target="_blank" rel="noopener">Ko-fi</a>
      <a href="/releases">Releases</a>
      <a href="/privacy">Privacy</a>
      <a href="/licenses">Licenses</a>
      <a href={SOURCE} target="_blank" rel="noopener">Source</a>
    </footer>
  {/if}
</div>

<style>
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    max-width: 1180px;
    margin: 0 auto;
    padding: 18px 24px;
  }

  .shell.full {
    display: flex;
    flex-direction: column;
    height: 100dvh;
    overflow: hidden;
    /* The bar's height, which the view under it subtracts from the window's. */
    --bar-h: 48px;
  }

  .shell.full .top {
    flex: none;
    box-sizing: border-box;
    height: var(--bar-h);
    max-width: none;
    margin: 0;
    padding: 0 16px;
    border-bottom: 1px solid var(--border);
  }

  .note {
    flex: 1;
    min-width: 0;
    margin: 0;
    overflow: hidden;
    font-size: 13px;
    text-overflow: ellipsis;
    white-space: nowrap;
  }

  .open {
    font-size: 13px;
    padding: 3px 10px;
    white-space: nowrap;
  }

  .review-tools {
    display: flex;
    gap: 6px;
  }

  .open.on {
    background: var(--accent-soft);
    border-color: var(--accent);
  }

  nav {
    align-items: center;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text);
    font-weight: 700;
    font-size: 17px;
    text-decoration: none;
  }

  nav {
    display: flex;
    gap: 20px;
    font-size: 14px;
  }

  nav a {
    color: var(--muted);
    text-decoration: none;
    white-space: nowrap;
  }

  @media (max-width: 480px) {
    .top {
      padding: 16px;
    }

    nav {
      gap: 14px;
    }

    .note {
      display: none;
    }
  }

  @media (max-width: 360px) {
    .download-link {
      display: none;
    }
  }

  nav a:hover {
    color: var(--text);
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px 22px;
    padding: 40px 24px 48px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: 13px;
  }

  footer a {
    color: var(--muted);
  }

  footer a:hover {
    color: var(--text);
  }

  .heart {
    color: #e5484d;
  }
</style>
