<script module lang="ts">
  /** Mutes or unmutes `player`; unmuting at no volume goes back to full, or it would stay silent. */
  export function toggleMute(player: EmbeddedPlayer | null | undefined) {
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
  // sound, speed, captions and full screen, where the player has them.

  import type { Snippet } from "svelte";
  import { DRAG_SLOP } from "./dragout";
  import type { EmbeddedPlayer } from "./embed";
  import Icon from "./Icon.svelte";
  import { frameIndex } from "./media";
  import { formatTime } from "./platform";
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
    captionsOn,
    fullscreen,
    canFullscreen,
    ontoggleplay,
    onfullscreen,
    actions,
  }: {
    player: EmbeddedPlayer | null;
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
    captionsOn: boolean;
    fullscreen: boolean;
    canFullscreen: boolean;
    ontoggleplay: () => void;
    onfullscreen: (on: boolean) => void;
    /** The panel's own buttons and line of context, after the time. */
    actions?: Snippet;
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

  // The speed button pressed and held opens a menu of the speeds above it;
  // let go over one, it's picked, and elsewhere the menu just closes. A
  // plain click still steps to the next speed. A press that moves first is
  // a drag, not a hold.
  const HOLD_MS = 400;
  let speedButton = $state<HTMLElement>();
  let speedMenu = $state(false);
  let speedPress: { x: number; y: number; timer: ReturnType<typeof setTimeout> } | null = null;
  // The menu was opened by this press: its let-go isn't a click on the button.
  let held = false;
  function onSpeedDown(e: PointerEvent) {
    if (e.button !== 0) return;
    held = false;
    const timer = setTimeout(() => {
      // Let go (or moved away) meanwhile: speedPress is gone, no menu.
      if (!speedPress) return;
      speedPress = null;
      held = true;
      speedMenu = true;
    }, HOLD_MS);
    speedPress = { x: e.clientX, y: e.clientY, timer };
  }
  function letGoOfPress() {
    if (speedPress) clearTimeout(speedPress.timer);
    speedPress = null;
  }
  function onWindowPointerMove(e: PointerEvent) {
    if (speedPress && (!(e.buttons & 1) || Math.hypot(e.clientX - speedPress.x, e.clientY - speedPress.y) >= DRAG_SLOP))
      letGoOfPress();
    // Its let-go missed (it happened outside the window): closed.
    else if (speedMenu && held && !(e.buttons & 1)) speedMenu = false;
  }
  function onWindowPointerUp(e: PointerEvent) {
    letGoOfPress();
    if (!speedMenu || !held) return;
    speedMenu = false;
    // By where it's let go: the press began on the button, so the button may be the event's target.
    const item = document.elementFromPoint(e.clientX, e.clientY)?.closest<HTMLElement>("[data-speed]");
    if (item) player?.rate?.set(Number(item.dataset.speed));
    // The click that may follow (let go on the button) comes before this.
    setTimeout(() => (held = false));
  }
  function onSpeedClick() {
    if (!held) nextSpeed();
  }
</script>

<svelte:window onpointermove={onWindowPointerMove} onpointerup={onWindowPointerUp} onpointercancel={letGoOfPress} />

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
  {#if player?.volume && !player.ownControls}
    <button
      class="control"
      onclick={() => toggleMute(player)}
      aria-label={muted ? "Unmute" : "Mute"}
      title={muted ? "Unmute (Shift+M)" : "Mute (Shift+M)"}
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
  {#if player?.rate && !player.ownControls}
    <button
      class="control speed"
      bind:this={speedButton}
      onpointerdown={onSpeedDown}
      onclick={onSpeedClick}
      title="Playback speed (hold for a list)">{rate}×</button
    >
    <Popover bind:open={speedMenu} anchor={speedButton} label="Playback speed" width={84} padding={4} above>
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
  {#if captionsAvailable}
    <button
      class="control"
      class:on={captionsOn}
      onclick={() => player?.captions?.toggle()}
      aria-pressed={captionsOn}
      aria-label="Captions"
      title={captionsOn ? "Hide captions (Shift+C)" : "Show captions (Shift+C)"}
    >
      <Icon name="captions" size={16} />
    </button>
  {/if}
  {#if canFullscreen}
    <button
      class="control"
      onclick={() => onfullscreen(!fullscreen)}
      aria-label={fullscreen ? "Exit full screen" : "Full screen"}
      title={fullscreen ? "Exit full screen (Esc)" : "Full screen (F)"}
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

  .control:hover {
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
