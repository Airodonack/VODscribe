<script lang="ts" module>
  /** What a search looks through: the audio (what was said) and chat. */
  export interface Sources {
    audio: boolean;
    chat: boolean;
  }

  /** The window when neither the filter nor the text sets another (crates/core query.rs). */
  export const DEFAULT_WITHIN = 20;
</script>

<script lang="ts">
  import { untrack } from "svelte";
  import { api, type AuthorSuggestion, type SearchQuery, type Vod } from "./api";
  import Icon from "./Icon.svelte";
  import Popover from "./Popover.svelte";
  import { lengthText, splitLength, UNITS, type Unit } from "./queryChips";

  // A button that sums up the search's filters, opening a panel to change them.
  // New kinds of filter get a section of the panel and a part of the summary.
  // Everything here can also be typed (in:, within:, from:); what's typed
  // wins, and its control shows the typed value, set by the search text,
  // until the text changes.
  let {
    scopeAll = $bindable(),
    sources = $bindable(),
    within = $bindable(),
    from = $bindable(),
    selected,
    vods,
    typed,
  }: {
    /** Search every VOD rather than just the selected one. Set by choosing All
     *  VODs, it stays set as other VODs are picked (shown with a lock), until
     *  This VOD is chosen again. */
    scopeAll: boolean;
    sources: Sources;
    /** Seconds a match's pieces must fit in. */
    within: number;
    /** Who said or wrote it; empty for anyone. */
    from: string;
    /** The selected VOD, which a search can be limited to. */
    selected: Vod | undefined;
    vods: Map<number, Vod>;
    /** What the search text set, from the last search's answer. */
    typed: SearchQuery | null;
  } = $props();

  let open = $state(false);
  let anchor = $state<HTMLElement>();

  // What applies: typed values over the filter's.
  const shownSources = $derived(typed?.sources ?? sources);
  const shownWithin = $derived(typed?.within ?? within);
  const shownFrom = $derived(typed?.from ?? from.trim());

  let scope = $derived(selected && !scopeAll ? "This VOD" : "All VODs");
  let looking = $derived(
    shownSources.audio && shownSources.chat ? "Audio & Chat" : shownSources.audio ? "Audio only" : "Chat only",
  );
  // The window's left out while it's the usual one.
  let more = $derived(
    (shownFrom ? `, from ${shownFrom}` : "") +
      (shownWithin !== DEFAULT_WITHIN ? `, within ${lengthText(shownWithin)}` : ""),
  );

  // Turning off the only source left switches to the other, rather than
  // searching nothing or ignoring the click.
  function toggle(key: keyof Sources) {
    const other: keyof Sources = key === "audio" ? "chat" : "audio";
    if (sources[key] && !sources[other]) sources[other] = true;
    sources[key] = !sources[key];
  }

  // The window as a number and a unit; the unit stays as chosen while the
  // number changes (120 s stays 120 s rather than turning into 2 min).
  let unit = $state<Unit>(splitLength(untrack(() => within)).unit);
  const shownLength = $derived.by(() => {
    if (typed?.within != null) return splitLength(typed.within);
    const n = within / UNITS[unit];
    return Number.isInteger(n) ? { n, unit } : splitLength(within);
  });
  function setLength(n: number, u: Unit) {
    unit = u;
    if (Number.isFinite(n) && n > 0) within = n * UNITS[u];
  }

  // Names for From whom as they're typed: channels first, then chatters, as
  // the library has them (in the VOD searched, if it's one).
  let suggestions = $state<AuthorSuggestion[]>([]);
  let naming = $state(false);
  let asked = 0;
  let timer: ReturnType<typeof setTimeout> | undefined;
  $effect(() => {
    const prefix = from.trim();
    const vodId = selected && !scopeAll ? selected.id : null;
    if (!naming) return;
    clearTimeout(timer);
    const mine = ++asked;
    timer = setTimeout(() => {
      api.authorSuggestions(prefix, vodId).then(
        (s) => mine === asked && (suggestions = s),
        () => mine === asked && (suggestions = []),
      );
    }, 150);
    return () => clearTimeout(timer);
  });
  // A channel with a VOD from YouTube, whose chat may show people by other names.
  const onYouTube = (name: string) =>
    [...vods.values()].some((v) => v.platform === "youtube" && v.channel?.toLowerCase() === name.toLowerCase());
  function choose(name: string) {
    from = name;
    naming = false;
  }
</script>

{#snippet setByText(key: string)}
  <span class="muted note set">Set by the search text: change {key} there.</span>
{/snippet}

<div class="anchor" bind:this={anchor}>
  <button
    class="filter"
    class:on={open}
    onclick={() => (open = !open)}
    aria-expanded={open}
    title="Change what this searches"
  >
    <Icon name="filter" size={14} /> From <span class="bar"></span><span class="summary"
      >{scope}{#if scopeAll}<span class="lock" title="Stays on All VODs when you switch VODs"
          ><Icon name="lock" size={11} /></span
        >{/if}, {looking}{more}</span
    >
  </button>
  <Popover bind:open {anchor} label="Search filter" width={300}>
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
        {#if scopeAll}<span class="muted detail">Stays on when you switch VODs</span>{/if}
      </button>
    </div>
    <p class="eyebrow">Look through</p>
    <div class="choices">
      <button
        class:on={shownSources.audio}
        aria-pressed={shownSources.audio}
        disabled={!!typed?.sources}
        onclick={() => toggle("audio")}
      >
        <span class="label">Audio</span><span class="muted detail">What was said</span>
      </button>
      <button
        class:on={shownSources.chat}
        aria-pressed={shownSources.chat}
        disabled={!!typed?.sources}
        onclick={() => toggle("chat")}
      >
        <span class="label">Chat</span><span class="muted detail">What chat wrote</span>
      </button>
      {#if typed?.sources}{@render setByText("in:")}{/if}
    </div>
    <p class="eyebrow">Within</p>
    <div class="section" role="group" aria-label="Within">
      <div class="length">
        <input
          type="number"
          min="1"
          step="1"
          aria-label="Within"
          value={shownLength.n}
          disabled={typed?.within != null}
          oninput={(e) => setLength(e.currentTarget.valueAsNumber, unit)}
        />
        <div class="segmented" role="radiogroup" aria-label="Unit">
          {#each [["s", "s"], ["m", "min"], ["h", "h"]] as const as [u, name] (u)}
            <button
              role="radio"
              aria-checked={shownLength.unit === u}
              class:on={shownLength.unit === u}
              disabled={typed?.within != null}
              onclick={() => setLength(shownLength.n, u)}>{name}</button
            >
          {/each}
        </div>
      </div>
      {#if typed?.within != null}{@render setByText("within:")}{:else}<span class="muted note"
          >A match's words all fall in this much of the VOD.</span
        >{/if}
    </div>
    <p class="eyebrow">From whom</p>
    <div class="section" role="group" aria-label="From whom">
      <input
        type="text"
        aria-label="From whom"
        placeholder="Anyone"
        autocomplete="off"
        spellcheck="false"
        value={typed?.from ?? from}
        disabled={typed?.from != null}
        onfocus={() => (naming = true)}
        onblur={() => (naming = false)}
        oninput={(e) => {
          from = e.currentTarget.value;
          naming = true;
        }}
      />
      {#if naming && suggestions.length}
        <ul class="names" role="listbox" aria-label="Names">
          {#each suggestions as s (s.name)}
            <li role="option" aria-selected={s.name.toLowerCase() === from.trim().toLowerCase()}>
              <!-- Pressed, not clicked: the field keeps its focus until the name is in. -->
              <button onmousedown={(e) => (e.preventDefault(), choose(s.name))}>
                <span class="label">{s.name}</span>
                <span class="muted detail"
                  >{s.streamer ? "Channel: what they said and wrote" : `${s.messages} in chat`}</span
                >
                {#if s.streamer && onYouTube(s.name)}
                  <span class="muted note">On YouTube, their name in chat can differ from the channel's.</span>
                {/if}
              </button>
            </li>
          {/each}
        </ul>
      {/if}
      {#if typed?.from != null}{@render setByText("from:")}{/if}
    </div>
  </Popover>
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

  .choices button:disabled {
    opacity: 0.7;
  }

  .choices button.on {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .section {
    display: flex;
    flex-direction: column;
    gap: 4px;
    margin-bottom: 12px;
  }

  .section:last-child {
    margin-bottom: 0;
  }

  .length {
    display: flex;
    gap: 6px;
  }

  /* As the app's text fields (app.css). */
  .length input {
    width: 80px;
    padding: 5px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
  }

  .segmented {
    display: inline-flex;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    overflow: hidden;
  }

  .segmented button {
    padding: 4px 10px;
    font-size: 12px;
    border: none;
    border-radius: 0;
    background: none;
    color: var(--muted);
  }

  .segmented button + button {
    border-left: 1px solid var(--border);
  }

  .segmented button.on {
    color: var(--text);
    background: var(--accent-soft);
  }

  .names button {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 1px;
    width: 100%;
    padding: 5px 10px;
    text-align: left;
    border: none;
    background: none;
  }

  .names button:hover {
    background: var(--surface-2);
  }

  .note {
    font-size: 12px;
  }

  .set {
    color: var(--accent);
  }

  .names {
    display: flex;
    flex-direction: column;
    gap: 2px;
    margin: 0;
    padding: 0;
    list-style: none;
  }

  .label {
    font-size: 13px;
    font-weight: 600;
  }

  .lock {
    margin-left: 4px;
    color: var(--accent);
  }

  .detail {
    font-size: 12px;
    max-width: 100%;
    overflow: hidden;
    text-overflow: ellipsis;
    white-space: nowrap;
  }
</style>
