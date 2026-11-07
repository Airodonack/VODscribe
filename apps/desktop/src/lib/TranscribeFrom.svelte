<script lang="ts">
  // Transcribing a stretch of a VOD from a moment, chosen from a menu: the
  // player bar's captions button right-clicked, or pressed and held (from the
  // playhead; let go over a choice, hold.svelte.ts, picks it), and the transcript's "Transcribe
  // from here" (from a line, or the start of a gap). The captions button's
  // menu has a first entry of its own (`lead`): what T does.
  // The choice is asked for at once (`transcribe_stretch`), after the requests
  // already made, and kept for next time, ticked.

  import { api, type Vod } from "./api";
  import Icon from "./Icon.svelte";
  import { formatSpan, formatTime } from "./platform";
  import Popover from "./Popover.svelte";
  import { load, save } from "./stored";
  import { transcribeChoices, transcribeStretchOf, type TranscribeChoice as Choice } from "./stretches";

  let {
    open = $bindable(),
    anchor,
    vod,
    from,
    above = false,
    lead,
    pointed = null,
    onhover,
    onerror,
  }: {
    open: boolean;
    /** The button that opens it (Popover.svelte's anchor). */
    anchor: HTMLElement | undefined;
    vod: Vod;
    /** Where the stretch starts, in seconds. */
    from: number;
    /** Above the button, as from the player's bar. */
    above?: boolean;
    /** An entry before the choices, its key shown (the player bar's: the selection, or the VOD). */
    lead?: { label: string; title: string; key?: string; run: () => void };
    /** The entry under the pointer while the button is held (hold.svelte.ts `pointed`), shaded as hovered. */
    pointed?: string | null;
    /** The entry pointed at: "lead", a choice's key, or null for none. */
    onhover?: (key: string | null) => void;
    /** The request was refused (the VOD gone, say): why. */
    onerror?: (message: string) => void;
  } = $props();

  const choices = $derived(transcribeChoices(from, vod.duration));
  const LAST = "vodscribe.transcribeFor";
  let last = $state(load(LAST));

  const stretchOf = (choice: Choice) => transcribeStretchOf(choice, from, vod.duration);

  function describe(choice: Choice): string {
    const { start, end } = stretchOf(choice);
    const until = end ?? vod.duration;
    return until ? formatSpan(start, until) : `from ${formatTime(start)}`;
  }

  function pick(choice: Choice) {
    last = choice.key;
    save(LAST, choice.key);
    open = false;
    const { start, end } = stretchOf(choice);
    api.transcribeStretch(vod.id, start, end).catch((e) => onerror?.(String(e)));
  }
</script>

<Popover
  bind:open
  {anchor}
  label={lead ? "Transcribe" : `Transcribe from ${formatTime(from)}`}
  width={220}
  padding={4}
  {above}
>
  {#if lead}
    <div class="choices lead">
      <button
        data-choice="lead"
        class:pointed={pointed === "lead"}
        title={lead.title}
        onclick={() => {
          open = false;
          lead.run();
        }}
        onmouseenter={() => onhover?.("lead")}
        onmouseleave={() => onhover?.(null)}
      >
        <span>{lead.label}</span>{#if lead.key}<kbd class="range">{lead.key}</kbd>{/if}
      </button>
    </div>
  {/if}
  <p class="muted head">From {formatTime(from)} for:</p>
  <div class="choices">
    {#each choices as choice (choice.key)}
      <button
        data-choice={choice.key}
        class:current={choice.key === last}
        class:pointed={pointed === choice.key}
        onclick={() => pick(choice)}
        onmouseenter={() => onhover?.(choice.key)}
        onmouseleave={() => onhover?.(null)}
      >
        {#if choice.key === last}<Icon name="check" size={12} />{/if}<span>{choice.label}</span><span
          class="muted range">{describe(choice)}</span
        >
      </button>
    {/each}
  </div>
</Popover>

<style>
  .head {
    margin: 0;
    padding: 5px 8px 3px;
    font-size: 12px;
  }

  /* As the speeds menu (PlayerBar.svelte): the one picked last ticked. */
  .choices {
    display: flex;
    flex-direction: column;
  }

  .choices button {
    display: grid;
    grid-template-columns: 14px auto 1fr;
    align-items: center;
    padding: 5px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    font-size: 12px;
    text-align: left;
  }

  .choices button span:first-of-type {
    grid-column: 2;
  }

  .choices button:hover,
  .choices button.pointed {
    background: var(--surface-2);
  }

  /* The first entry, set off from the choices from here under it. */
  .lead {
    padding-bottom: 4px;
    border-bottom: 1px solid var(--border);
  }

  .lead kbd {
    font-size: 10px;
  }

  .choices .current {
    color: var(--accent);
  }

  .range {
    justify-self: end;
    padding-left: 12px;
    font-variant-numeric: tabular-nums;
  }
</style>
