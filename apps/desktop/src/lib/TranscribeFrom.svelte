<script lang="ts">
  // Transcribing a stretch of a VOD from a moment, chosen from a menu: the
  // player bar's Transcribe pressed and held (from the playhead; let go over
  // a choice, hold.svelte.ts, picks it), and the transcript's "Transcribe
  // from here" (from a line, or the start of a gap).
  // The choice is asked for at once (`transcribe_stretch`), after the requests
  // already made, and kept for next time, ticked.

  import { api, type Vod } from "./api";
  import Icon from "./Icon.svelte";
  import { formatSpan, formatTime } from "./platform";
  import Popover from "./Popover.svelte";
  import { load, save } from "./stored";

  let {
    open = $bindable(),
    anchor,
    vod,
    from,
    above = false,
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
    /** The request was refused (the VOD gone, say): why. */
    onerror?: (message: string) => void;
  } = $props();

  // A length in seconds, or on to the end, or all of it from the start.
  type Choice = { key: string; label: string; length?: number; whole?: boolean };
  const CHOICES: Choice[] = [
    { key: "300", label: "5 min", length: 300 },
    { key: "900", label: "15 min", length: 900 },
    { key: "3600", label: "1 hour", length: 3600 },
    { key: "end", label: "to the end" },
    { key: "whole", label: "the whole VOD", whole: true },
  ];
  const LAST = "vodscribe.transcribeFor";
  let last = $state(load(LAST));

  /** The stretch a choice asks for: its end null when it runs on to the VOD's end. */
  function stretchOf(choice: Choice): { start: number; end: number | null } {
    if (choice.whole) return { start: 0, end: null };
    const end = choice.length === undefined ? null : from + choice.length;
    return { start: from, end: end !== null && vod.duration && end >= vod.duration ? null : end };
  }

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

<Popover bind:open {anchor} label="Transcribe from {formatTime(from)}" width={220} padding={4} {above}>
  <p class="muted head">From {formatTime(from)} for:</p>
  <div class="choices">
    {#each CHOICES as choice (choice.key)}
      <button data-choice={choice.key} class:current={choice.key === last} onclick={() => pick(choice)}>
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

  .choices button:hover {
    background: var(--surface-2);
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
