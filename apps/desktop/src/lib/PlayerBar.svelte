<script module lang="ts">
  import type { EmbeddedPlayer } from "./embed";

  /** What the bar sets: the player's sound and speed, where it has them
   *  (or, before it's made, what it will start with: Player.svelte). */
  export type BarControls = Pick<EmbeddedPlayer, "volume" | "rate">;

  /** Mutes or unmutes `player`; unmuting at no volume goes back to full, or it would stay silent. */
  export function toggleMute(player: BarControls | null | undefined) {
    const sound = player?.volume;
    if (!sound) return;
    if (sound.muted() && sound.get() === 0) sound.set(1);
    else sound.setMuted(!sound.muted());
  }
</script>

<script lang="ts">
  // The row of controls under every player's timeline (the VOD's, Player.svelte;
  // an edit's, EditView.svelte): play, the time, the panel's own actions (Split
  // and Clip, or what an edit has instead) with a line of context, then
  // sound, speed, the panel's own at the right (a VOD's Transcribe), captions
  // and full screen, where the player has them.

  import type { Snippet } from "svelte";
  import Icon from "./Icon.svelte";
  import { Hold } from "./hold.svelte";
  import { frameIndex } from "./media";
  import { keyHint } from "./keys.svelte";
  import { formatTime } from "./platform";
  import { prefs } from "./playerPrefs.svelte";
  import Popover from "./Popover.svelte";
  import { load, save } from "./stored";

  let {
    player,
    playing,
    now,
    duration,
    volume,
    muted,
    rate,
    fps,
    captionsAvailable,
    captionsNone = false,
    captionsOn,
    fullscreen,
    canFullscreen,
    ontoggleplay,
    onfullscreen,
    actions,
    beside,
  }: {
    player: BarControls | null;
    playing: boolean;
    /** Where the picture is (PlayerReading.actual): the time shown, by frame and as a clock. */
    now: number;
    duration: number;
    volume: number;
    muted: boolean;
    rate: number;
    /** The player's frame rate, for the time by frame. */
    fps: number;
    captionsAvailable: boolean;
    /** The player would show captions, but there are none: nothing is transcribed yet. */
    captionsNone?: boolean;
    captionsOn: boolean;
    fullscreen: boolean;
    canFullscreen: boolean;
    ontoggleplay: () => void;
    onfullscreen: (on: boolean) => void;
    /** The panel's own buttons and line of context, after the time. */
    actions?: Snippet;
    /** The panel's own controls at the right, before the captions button (a VOD's Transcribe). */
    beside?: Snippet;
  } = $props();

  // The time by frame (the clock small under it), or as a clock: clicking it
  // switches, and the choice is kept.
  const BY_FRAME = "vodscribe.timeByFrame";
  let byFrame = $state(load(BY_FRAME) === "1");
  function toggleByFrame() {
    byFrame = !byFrame;
    save(BY_FRAME, byFrame ? "1" : "0");
  }
  const frame = (seconds: number) => String(frameIndex(seconds, fps));

  const SPEEDS = [1, 1.25, 1.5, 2, 0.5, 0.75];
  function nextSpeed() {
    player?.rate?.set(SPEEDS[(SPEEDS.indexOf(rate) + 1) % SPEEDS.length]);
  }

  // The speed button pressed and held opens a menu of the speeds above it
  // (hold.svelte.ts); a plain click still steps to the next speed.
  let speedButton = $state<HTMLElement>();
  const speedHold = new Hold("[data-speed]", (item) => player?.rate?.set(Number(item.dataset.speed)));
  function onSpeedClick() {
    if (!speedHold.held) nextSpeed();
  }
</script>

<!-- Not "row": that's the transcript's lines, styled app-wide. -->
<div class="bar">
  <button
    class="control"
    onclick={ontoggleplay}
    aria-label={playing ? "Pause" : "Play"}
    title={playing ? "Pause" : "Play"}
  >
    <Icon name={playing ? "pause" : "play"} size={16} />
  </button>
  <!-- The time takes the room of the longest it can be (the length, under it
       unseen), so nothing beside it moves. -->
  <button class="time" onclick={toggleByFrame} title={byFrame ? "Show the time as a clock" : "Show the time by frame"}>
    {#if byFrame}
      <!-- Frame over clock, in columns: the times now right-aligned, the
           slashes over each other, the lengths left-aligned. -->
      <span class="stack">
        <span class="now"
          ><span class="widest" aria-hidden="true">{frame(duration)}</span><span>{frame(now)}</span></span
        >
        <span class="slash">/</span>
        <span>{frame(duration)}</span>
        <span class="now clock"
          ><span class="widest" aria-hidden="true">{formatTime(duration)}</span><span>{formatTime(now)}</span></span
        >
        <span class="slash clock">/</span>
        <span class="clock">{formatTime(duration)}</span>
      </span>
    {:else}
      <span
        ><span class="now"
          ><span class="widest" aria-hidden="true">{formatTime(duration)}</span><span>{formatTime(now)}</span></span
        >
        / {formatTime(duration)}</span
      >
    {/if}
  </button>
  {@render actions?.()}
  {#if player?.volume}
    <button
      class="control"
      onclick={() => toggleMute(player)}
      aria-label={muted ? "Unmute" : "Mute"}
      title={`${muted ? "Unmute" : "Mute"}${keyHint("mute.toggle")}`}
    >
      <Icon name={muted ? "volume-off" : "volume"} size={16} />
    </button>
    <input
      class="volume"
      type="range"
      min="0"
      max="1"
      step="0.05"
      value={muted ? 0 : volume}
      oninput={(e) => player?.volume?.set(Number(e.currentTarget.value))}
      aria-label="Volume"
    />
  {/if}
  {#if player?.rate}
    <button
      class="control speed"
      bind:this={speedButton}
      onpointerdown={speedHold.down}
      onclick={onSpeedClick}
      title="Playback speed (hold for a list)">{rate}×</button
    >
    <Popover bind:open={speedHold.open} anchor={speedButton} label="Playback speed" width={84} padding={4} above>
      <div class="speeds">
        {#each [...SPEEDS].sort((a, b) => b - a) as s (s)}
          <button
            data-speed={s}
            class:current={s === rate}
            aria-pressed={s === rate}
            onclick={() => player?.rate?.set(s)}
            >{#if s === rate}<Icon name="check" size={12} />{/if}<span>{s}×</span></button
          >
        {/each}
      </div>
    </Popover>
  {/if}
  {@render beside?.()}
  {#if captionsAvailable || captionsNone}
    <button
      class="control"
      class:on={captionsOn && captionsAvailable}
      onclick={() => prefs.toggleCaptions()}
      disabled={!captionsAvailable}
      aria-pressed={captionsOn && captionsAvailable}
      aria-label="Captions"
      title={!captionsAvailable
        ? "Transcribe to get captions"
        : captionsOn
          ? `Hide captions${keyHint("captions.toggle")}`
          : `Show captions${keyHint("captions.toggle")}`}
    >
      <Icon name="captions" size={16} />
    </button>
  {/if}
  {#if canFullscreen}
    <button
      class="control"
      onclick={() => onfullscreen(!fullscreen)}
      aria-label={fullscreen ? "Exit full screen" : "Full screen"}
      title={fullscreen ? "Exit full screen (Esc)" : `Full screen${keyHint("fullscreen.toggle")}`}
    >
      <Icon name={fullscreen ? "minimize" : "maximize"} size={16} />
    </button>
  {/if}
</div>

<style>
  .bar {
    display: flex;
    align-items: center;
    gap: 4px;
    height: var(--controls-height);
    margin: 0 -6px;
  }

  .control {
    flex: none;
    display: inline-flex;
    align-items: center;
    justify-content: center;
    min-width: 30px;
    height: 30px;
    padding: 0 6px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
  }

  .control:hover:not(:disabled) {
    background: var(--surface-2);
  }

  .control.on {
    color: var(--accent);
  }

  /* The panel's actions (Split, Clip…): labelled, with their keys, but
     otherwise like the other controls. */
  .bar :global(.action) {
    flex: none;
    display: inline-flex;
    align-items: center;
    gap: 5px;
    height: 30px;
    padding: 0 6px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    color: inherit;
    font-size: 12px;
  }

  .bar :global(.action:hover:not(:disabled)) {
    background: var(--surface-2);
  }

  .bar :global(.action:disabled) {
    opacity: 0.45;
  }

  .bar :global(.action kbd) {
    min-width: 0;
    padding: 0 4px;
    font-size: 10px;
    line-height: 15px;
    color: var(--muted);
  }

  /* A line of context after the actions: brief facts. */
  .bar :global(.context) {
    flex: 1;
    min-width: 0;
    padding: 0 8px;
    font-size: 12px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .speed {
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    min-width: 42px;
  }

  /* The speeds held open from the speed button, fastest at the top. */
  .speeds {
    display: flex;
    flex-direction: column;
  }

  .speeds button {
    display: grid;
    grid-template-columns: 14px auto;
    align-items: center;
    padding: 5px 8px;
    border: none;
    border-radius: var(--radius-sm);
    background: none;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    text-align: left;
  }

  .speeds button span {
    grid-column: 2;
  }

  .speeds button:hover {
    background: var(--surface-2);
  }

  .speeds .current {
    color: var(--accent);
  }

  .time {
    flex: none;
    padding: 0 6px;
    border: none;
    background: none;
    font: inherit;
    font-size: 12px;
    font-variant-numeric: tabular-nums;
    color: var(--muted);
    white-space: nowrap;
    cursor: pointer;
    gap: 0;
    line-height: 1.2;
  }

  .stack {
    display: grid;
    grid-template-columns: auto auto auto;
    align-items: baseline;
  }

  .stack .now {
    justify-self: end;
  }

  .slash {
    padding: 0 0.35em;
    text-align: center;
  }

  .time:hover {
    color: var(--text);
  }

  .clock {
    font-size: 9px;
  }

  .now {
    display: inline-grid;
    justify-items: end;
  }

  .now > span {
    grid-area: 1 / 1;
  }

  .widest {
    visibility: hidden;
  }

  .volume {
    flex: none;
    width: 64px;
    accent-color: var(--accent);
  }

  @media (max-width: 560px) {
    .volume,
    .speed {
      display: none;
    }
  }
</style>
