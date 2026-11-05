<script lang="ts">
  import { onMount } from "svelte";
  import changelog from "../../../../../CHANGELOG.md?raw";
  import { countsText, isFixes, longDate, plural, releases, runs, sentences, type Release } from "#lib/changelog.js";
  import { DOWNLOADS, onHost, versionedUrl } from "#lib/downloads.js";

  // Every released version's changes, newest first (CHANGELOG.md, read when the
  // site is built). Each folds to its lead and how many changes and fixes it
  // has, the newest starting open, and each one's fixes fold to their count. The app's version number links to its own entry by its id
  // (/releases#0.2.1), which opens it, its fixes too, and is highlighted.
  const list = releases(changelog);

  /** The version the address names, marked in the list of versions. */
  let current = $state("");

  /** Open the release the address names (/releases#0.6.0), and bring it into view. */
  function openTarget() {
    const id = decodeURIComponent(location.hash.slice(1));
    const section = document.getElementById(id);
    if (!section?.classList.contains("release")) return;
    current = id;
    for (const details of section.querySelectorAll("details")) details.open = true;
    section.scrollIntoView();
  }

  onMount(() => {
    openTarget();
    window.addEventListener("hashchange", openTarget);
    return () => window.removeEventListener("hashchange", openTarget);
  });
</script>

{#snippet text(s: string)}{#each runs(s) as run, k (k)}{#if run.code}<code>{run.text}</code
      >{:else}{run.text}{/if}{/each}{/snippet}

<!-- A change: its first sentence plainly, the rest a step quieter. -->
{#snippet change(s: string)}
  {@const { first, rest } = sentences(s)}
  {@render text(first)}{#if rest}{" "}<span class="rest">{@render text(rest)}</span>{/if}
{/snippet}

{#snippet heading(release: Release, latest: boolean)}
  <h2>
    VODscribe {release.version}
    {#if latest}<span class="latest">Latest</span>{/if}
  </h2>
  <p class="muted date">{longDate(release.date)}</p>
  {#each release.intro as lead, j (j)}
    <p class="lead">{@render text(lead)}</p>
  {/each}
{/snippet}

{#snippet notes(release: Release)}
  {@const news = release.changes.filter((c) => !isFixes(c))}
  {@const fixes = release.changes.filter(isFixes).flatMap((c) => c.items)}
  {#if news.length}
    <ul class="changes">
      {#each news as c, j (j)}
        <li>{@render change(c.text)}</li>
      {/each}
    </ul>
  {/if}
  {#if fixes.length}
    <details class="fixes">
      <summary>{plural(fixes.length, "fix", "fixes")}</summary>
      <ul class="changes">
        {#each fixes as fix, k (k)}
          <li>{@render change(fix)}</li>
        {/each}
      </ul>
    </details>
  {/if}
  {#if onHost(release.version)}
    <p class="muted downloads">
      Downloads:
      {#each DOWNLOADS as d, k (d.url)}{#if k}{" · "}{/if}<a href={versionedUrl(d, release.version)}>{d.short}</a
        >{/each}
    </p>
  {/if}
{/snippet}

<svelte:head>
  <title>Releases · VODscribe</title>
  <link rel="canonical" href="https://vodscribe.com/releases" />
  <meta property="og:title" content="Releases · VODscribe" />
  <meta property="og:image" content="https://vodscribe.com/og.jpg" />
  <meta name="description" content="What's new in each version of VODscribe." />
</svelte:head>

<article class="prose">
  <h1>Releases</h1>
  <p class="muted">
    What changed in each version, newest first. The app updates itself, but older versions are here if you need one.
  </p>
  <!-- The versions: a row under the intro, or beside the notes where the window is wide (.side). -->
  <div class="side">
    <nav class="versions" aria-label="Versions">
      {#each list as release (release.version)}
        <a href="#{release.version}" class:current={release.version === current}>{release.version}</a>
      {/each}
    </nav>
  </div>

  {#each list as release, i (release.version)}
    <section class="release" id={release.version}>
      <!-- Each release folds to its heading, lead and counts; the latest starts open. -->
      <details class="entry" open={i === 0}>
        <summary>
          {@render heading(release, i === 0)}
          {#if countsText(release)}<p class="muted counts">{countsText(release)}</p>{/if}
        </summary>
        {@render notes(release)}
      </details>
    </section>
  {/each}
</article>

<style>
  .prose {
    position: relative;
  }

  .versions {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin: 12px 0 8px;
    font-size: 14px;
  }

  .versions .current {
    font-weight: 600;
    color: var(--accent);
  }

  /* Room beside the notes: the versions stand in a column to their right
     and stay in view as the page scrolls (.side runs the article's height,
     so the sticky list has that far to go). */
  @media (min-width: 1100px) {
    .side {
      position: absolute;
      top: 0;
      bottom: 0;
      left: 100%;
      width: 140px;
      margin-left: 8px;
    }

    .versions {
      position: sticky;
      top: 24px;
      flex-direction: column;
      gap: 6px;
      margin: 40px 0 0;
      padding-left: 14px;
      border-left: 1px solid var(--border);
    }
  }

  .release {
    margin: 0 -16px 8px;
    padding: 4px 16px 8px;
    border-left: 3px solid transparent;
    border-radius: var(--radius);
    /* Linked to from the app: clear of the top of the window. */
    scroll-margin-top: 24px;
  }

  /* The version linked to (/releases#0.2.1). */
  .release:target {
    border-left-color: var(--accent);
    background: color-mix(in srgb, var(--accent) 8%, transparent);
  }

  .release h2 {
    margin-top: 12px;
    display: flex;
    align-items: center;
    gap: 10px;
  }

  .latest {
    padding: 1px 8px;
    border-radius: 10px;
    font-size: 12px;
    font-weight: 600;
    color: var(--accent);
    background: var(--accent-soft);
  }

  .date {
    margin: -8px 0 0;
  }

  .lead,
  .changes,
  .downloads,
  .fixes {
    max-width: 65ch;
  }

  .lead {
    margin: 10px 0 0;
  }

  .changes li + li {
    margin-top: 10px;
  }

  .rest {
    color: var(--muted);
  }

  .fixes {
    margin: 4px 0 0;
  }

  .fixes > summary,
  .entry > summary {
    cursor: pointer;
  }

  .fixes > summary {
    color: var(--muted);
    font-weight: 600;
  }

  .fixes > summary:hover,
  .entry > summary:hover h2 {
    color: var(--accent);
  }

  /* A release's summary is its heading, date, lead and counts: the
     marker sits in the counts line, not beside the heading. */
  .entry > summary {
    list-style: none;
  }

  .entry > summary::-webkit-details-marker {
    display: none;
  }

  .counts {
    margin: 8px 0 0;
    font-size: 14px;
  }

  .counts::before {
    content: "▸ ";
  }

  .entry[open] .counts::before {
    content: "▾ ";
  }

  .downloads {
    margin: 12px 0 0;
    font-size: 14px;
  }
</style>
