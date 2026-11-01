<script lang="ts">
  import { onMount } from "svelte";
  import changelog from "../../../../../CHANGELOG.md?raw";
  import { countsText, isFixes, longDate, plural, releases, runs, sentences, type Release } from "#lib/changelog.js";
  import { DOWNLOADS, onHost, versionedUrl } from "#lib/downloads.js";

  // Every released version's changes, newest first (CHANGELOG.md, read when the
  // site is built). The newest is open; the ones before it fold to their lead
  // and how many changes and fixes they have, and each one's fixes fold to
  // their count. The app's version number links to its own entry by its id
  // (/releases#0.2.1), which opens it, its fixes too, and is highlighted.
  const list = releases(changelog);

  /** Open the release the address names (/releases#0.6.0), and bring it into view. */
  function openTarget() {
    const section = document.getElementById(decodeURIComponent(location.hash.slice(1)));
    if (!section?.classList.contains("release")) return;
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

{#snippet notes(release: Release, latest: boolean)}
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
  {#if latest}
    <p class="muted downloads">This is the <a href="/#download">current version</a>.</p>
  {:else if onHost(release.version)}
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
    What changed in each version of VODscribe, newest first. Older versions are here if you need one; the app updates
    itself to the newest.
  </p>
  <nav class="versions" aria-label="Versions">
    {#each list as release (release.version)}
      <a href="#{release.version}">{release.version}</a>
    {/each}
  </nav>

  {#each list as release, i (release.version)}
    <section class="release" id={release.version}>
      {#if i === 0}
        {@render heading(release, true)}
        {@render notes(release, true)}
      {:else}
        <details class="older">
          <summary>
            {@render heading(release, false)}
            {#if countsText(release)}<p class="muted counts">{countsText(release)}</p>{/if}
          </summary>
          {@render notes(release, false)}
        </details>
      {/if}
    </section>
  {/each}
</article>

<style>
  .versions {
    display: flex;
    flex-wrap: wrap;
    gap: 4px 14px;
    margin: 12px 0 8px;
    font-size: 14px;
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
  .older > summary {
    cursor: pointer;
  }

  .fixes > summary {
    color: var(--muted);
    font-weight: 600;
  }

  .fixes > summary:hover,
  .older > summary:hover h2 {
    color: var(--accent);
  }

  /* An older release's summary is its heading, date, lead and counts: the
     marker sits in the counts line, not beside the heading. */
  .older > summary {
    list-style: none;
  }

  .older > summary::-webkit-details-marker {
    display: none;
  }

  .counts {
    margin: 8px 0 0;
    font-size: 14px;
  }

  .counts::before {
    content: "▸ ";
  }

  .older[open] .counts::before {
    content: "▾ ";
  }

  .downloads {
    margin: 12px 0 0;
    font-size: 14px;
  }
</style>
