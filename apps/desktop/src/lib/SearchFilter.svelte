<script lang="ts" module>
  /** What a search looks through: the audio (what was said) and chat. */
  export interface Sources {
    audio: boolean;
    chat: boolean;
  }
</script>

<script lang="ts">
  import type { Vod } from "./api";
  import Icon from "./Icon.svelte";

  // A button that sums up the search's filters, opening a panel to change them.
  // New kinds of filter get a section of the panel and a part of the summary.
  let {
    scopeAll = $bindable(),
    sources = $bindable(),
    selected,
  }: {
    /** Search every VOD rather than just the selected one. Set by choosing All
     *  VODs, it stays set as other VODs are picked (shown with a lock), until
     *  This VOD is chosen again. */
    scopeAll: boolean;
    sources: Sources;
    /** The selected VOD, which a search can be limited to. */
    selected: Vod | undefined;
  } = $props();

  let open = $state(false);
  let anchor = $state<HTMLElement>();

  let scope = $derived(selected && !scopeAll ? "This VOD" : "All VODs");
  let looking = $derived(sources.audio && sources.chat ? "Audio & Chat" : sources.audio ? "Audio only" : "Chat only");

  // Turning off the only source left switches to the other, rather than
  // searching nothing or ignoring the click.
  function toggle(key: keyof Sources) {
    const other: keyof Sources = key === "audio" ? "chat" : "audio";
    if (sources[key] && !sources[other]) sources[other] = true;
    sources[key] = !sources[key];
  }

  function onWindowClick(e: MouseEvent) {
    if (open && anchor && !anchor.contains(e.target as Node)) open = false;
  }
</script>

<svelte:window onclick={onWindowClick} onkeydown={(e) => e.key === "Escape" && (open = false)} />

<div class="anchor" bind:this={anchor}>
  <button
    class="filter"
    class:on={open}
    onclick={() => (open = !open)}
    aria-expanded={open}
    title="Change what this searches"
  >
    <Icon name="filter" size={14} /> From <span class="bar"></span><span class="summary"
      >{scope}{#if scopeAll}<span class="lock" title="Stays on All VODs when you pick a VOD"
          ><Icon name="lock" size={11} /></span
        >{/if}, {looking}</span
    >
  </button>
  {#if open}
    <div class="popover" role="dialog" aria-label="Search filter">
      <p class="eyebrow">Search in</p>
      <div class="choices">
        <button
          class:on={selected && !scopeAll}
          aria-pressed={!!selected && !scopeAll}
          disabled={!selected}
          title={selected ? selected.title : "Pick a VOD in the library to search only it"}
          onclick={() => (scopeAll = false)}
        >
          <span class="label">This VOD</span>
          {#if selected}<span class="muted detail">{selected.title}</span>{/if}
        </button>
        <button class:on={!selected || scopeAll} aria-pressed={!selected || scopeAll} onclick={() => (scopeAll = true)}>
          <span class="label"
            >All VODs{#if scopeAll}<span class="lock"><Icon name="lock" size={11} /></span>{/if}</span
          >
          <span class="muted detail wrap">
            {scopeAll ? "Locked: stays on when you pick a VOD" : "Choose it to keep it on when you pick a VOD"}
          </span>
        </button>
      </div>
      <p class="eyebrow">Look through</p>
      <div class="choices">
        <button class:on={sources.audio} aria-pressed={sources.audio} onclick={() => toggle("audio")}>
          <span class="label">Audio</span><span class="muted detail">What was said</span>
        </button>
        <button class:on={sources.chat} aria-pressed={sources.chat} onclick={() => toggle("chat")}>
          <span class="label">Chat</span><span class="muted detail">What chat wrote</span>
        </button>
      </div>
    </div>
  {/if}
</div>

<style>
  .anchor {
    position: relative;
    flex: none;
  }

  .filter {
    display: inline-flex;
    align-items: center;
    gap: 6px;
    height: 32px;
    padding: 0 12px;
    font-size: 12px;
    white-space: nowrap;
  }

  .filter.on {
    border-color: var(--accent);
  }

  .bar {
    width: 1px;
    align-self: stretch;
    margin: 7px 2px;
    background: var(--border);
  }

  .summary {
    color: var(--muted);
  }

  .popover {
    position: absolute;
    top: calc(100% + 8px);
    right: 0;
    z-index: 10;
    width: 280px;
    padding: 12px;
    background: var(--surface);
    border: 1px solid var(--border);
    border-radius: 10px;
    box-shadow: var(--shadow);
  }

  .eyebrow {
    margin: 0 0 6px;
  }

  .choices {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 12px;
  }

  .choices:last-child {
    margin-bottom: 0;
  }

  .choices button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    width: 100%;
    padding: 6px 10px;
    text-align: left;
    border: 1px solid var(--border);
    background: none;
  }

  .choices button.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .label {
    font-size: 13px;
    font-weight: 600;
  }

  .lock {
    margin-left: 4px;
    color: var(--accent);
  }

  .detail.wrap {
    white-space: normal;
  }

  .detail {
    font-size: 12px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
