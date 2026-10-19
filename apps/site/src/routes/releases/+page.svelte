<script lang="ts">
  import changelog from "../../../../../CHANGELOG.md?raw";
  import { longDate, releases, runs } from "#lib/changelog.js";

  // Every released version's changes, newest first (CHANGELOG.md, read when the
  // site is built). The app's version number links to its own, by its id
  // (/releases#0.2.1), which is highlighted when linked to.
  const list = releases(changelog);
</script>

{#snippet text(s: string)}{#each runs(s) as run, k (k)}{#if run.code}<code>{run.text}</code
      >{:else}{run.text}{/if}{/each}{/snippet}

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
    What changed in each version of VODscribe, newest first. The app updates itself; the version number in its sidebar
    links to its entry here.
  </p>

  {#each list as release, i (release.version)}
    <section class="release" id={release.version}>
      <h2>
        <a href="#{release.version}">VODscribe {release.version}</a>
        {#if i === 0}<span class="latest">Latest</span>{/if}
      </h2>
      <p class="muted date">{longDate(release.date)}</p>
      {#each release.intro as text, j (j)}
        <p>{text}</p>
      {/each}
      {#if release.changes.length}
        <ul>
          {#each release.changes as change, j (j)}
            <li>
              {@render text(change.text)}
              {#if change.items.length}
                <!-- The fixes, under "Fixes:": a step quieter than what's new. -->
                <ul class="items">
                  {#each change.items as item, k (k)}
                    <li>{@render text(item)}</li>
                  {/each}
                </ul>
              {/if}
            </li>
          {/each}
        </ul>
      {/if}
    </section>
  {/each}
</article>

<style>
  .items {
    margin: 4px 0 0;
    color: var(--muted);
    font-size: 0.95em;
  }

  .items li {
    margin: 2px 0;
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

  h2 a {
    color: inherit;
    text-decoration: none;
  }

  h2 a:hover {
    text-decoration: underline;
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
    margin-top: -8px;
  }
</style>
