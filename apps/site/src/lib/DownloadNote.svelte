<script lang="ts">
  // Shown once a Windows or Mac download has started: the warning the system
  // is about to show (VODscribe isn't code-signed yet), drawn as a sketch with
  // the buttons to press highlighted, and where to check the app first.
  import Icon from "desktop/lib/Icon.svelte";
  import type { Os } from "./downloads";

  let { os = $bindable() }: { os: Os | null } = $props();

  let dialog = $state<HTMLDialogElement>();
  $effect(() => {
    if (!dialog) return;
    if ((os === "windows" || os === "mac") && !dialog.open) {
      dialog.showModal();
      // The dialog, not its close button, so it opens without a focus ring on ×.
      dialog.focus();
    } else if (!os && dialog.open) dialog.close();
  });

  const SOURCE = "https://github.com/Airodonack/VODscribe";
  const BUILDS = "https://github.com/Airodonack/VODscribe/actions/workflows/build.yml";
</script>

<dialog
  bind:this={dialog}
  aria-labelledby="note-title"
  tabindex="-1"
  onclose={() => (os = null)}
  onclick={(e) => e.target === dialog && dialog.close()}
>
  <div class="body">
    <button class="icon close" aria-label="Close" onclick={() => dialog?.close()}><Icon name="x" size={18} /></button>
    <div class="started">
      <span class="badge"
        ><Icon name="download" size={18} /><span class="tick"><Icon name="check" size={11} /></span></span
      >
      <div>
        <h2 id="note-title">Your download has started</h2>
        <p class="muted">One heads-up before you open it.</p>
      </div>
    </div>

    {#if os === "windows"}
      <p>
        Windows will show a blue “Windows protected your PC” warning. That's because VODscribe isn't code-signed yet:
        signing needs a registered company, and I'm setting one up now. Once that's done, the warning goes away.
      </p>
      <p><strong>To open it:</strong> click <strong>More info</strong>, then <strong>Run anyway</strong>.</p>
      <div class="sketches" aria-hidden="true">
        <figure>
          <span class="step">1</span>
          <div class="win">
            <b>Windows protected your PC</b>
            <i></i><i class="short"></i>
            <span class="target link">More info</span>
            <div class="buttons"><span>Don't run</span></div>
          </div>
        </figure>
        <figure>
          <span class="step">2</span>
          <div class="win">
            <b>Windows protected your PC</b>
            <i></i><i class="short"></i>
            <i class="short"></i>
            <div class="buttons"><span class="target">Run anyway</span><span>Don't run</span></div>
          </div>
        </figure>
      </div>
    {:else if os === "mac"}
      <p>
        macOS will say it “could not verify VODscribe is free of malware” and won't open it. That's because VODscribe
        isn't code-signed yet: signing needs a registered company, and I'm setting one up now. Once that's done, the
        warning goes away.
      </p>
      <p>
        <strong>To open it:</strong> click <strong>Done</strong>, then open
        <strong>System Settings › Privacy & Security</strong>, scroll down, and click <strong>Open Anyway</strong>. You
        only have to do this once.
      </p>
      <div class="sketches" aria-hidden="true">
        <figure>
          <span class="step">1</span>
          <div class="mac-alert">
            <span class="app"></span>
            <b>“VODscribe” Not Opened</b>
            <i></i><i class="short"></i>
            <span class="target wide">Done</span>
            <span class="wide">Move to Trash</span>
          </div>
        </figure>
        <figure>
          <span class="step">2</span>
          <div class="mac-settings">
            <div class="side"><i></i><i></i><span class="on">Privacy & Security</span><i></i></div>
            <div class="pane">
              <b>Security</b>
              <small>“VODscribe” was blocked to protect your Mac.</small>
              <span class="target">Open Anyway</span>
            </div>
          </div>
        </figure>
      </div>
    {/if}

    <div class="check">
      <p class="muted"><strong>Want to check it first?</strong></p>
      <p class="muted">
        <Icon name="search" size={14} />
        <span>
          The <a href={SOURCE} target="_blank" rel="noopener">full source code</a> is on GitHub, and every installer is
          <a href={BUILDS} target="_blank" rel="noopener">built from it on GitHub's servers</a>, never on my computer.
          Read it yourself, or ask an AI assistant like ChatGPT to look it over.
        </span>
      </p>
      <p class="muted">
        <Icon name="shield" size={14} />
        <span>
          The <a href="/privacy">privacy policy</a> lists everything VODscribe connects to. Your VODs and transcripts never
          leave your computer.
        </span>
      </p>
    </div>
  </div>
</dialog>

<style>
  dialog {
    width: min(560px, calc(100% - 32px));
    max-height: calc(100% - 32px);
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 14px;
    background: var(--surface);
    color: var(--text);
    box-shadow: var(--shadow);
  }

  dialog:focus {
    outline: none;
  }

  dialog::backdrop {
    background: rgb(0 0 0 / 0.45);
  }

  .body {
    position: relative;
    padding: 24px;
    line-height: 1.55;
  }

  .close {
    position: absolute;
    top: 14px;
    right: 14px;
  }

  .started {
    display: flex;
    align-items: center;
    gap: 14px;
    margin-bottom: 16px;
  }

  .badge {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
    width: 42px;
    height: 42px;
    border-radius: 50%;
    background: var(--accent-soft);
    color: var(--accent);
  }

  .tick {
    position: absolute;
    right: -2px;
    bottom: -2px;
    display: grid;
    place-items: center;
    width: 18px;
    height: 18px;
    border: 2px solid var(--surface);
    border-radius: 50%;
    background: var(--accent);
    color: var(--accent-text);
  }

  h2 {
    margin: 0;
    font-size: 19px;
  }

  .started p {
    margin: 0;
  }

  p {
    margin: 0 0 12px;
  }

  /* The sketches: the system's warning, simplified, with what to press in the accent color. */
  .sketches {
    display: grid;
    grid-template-columns: 1fr 1fr;
    gap: 12px;
    margin: 16px 0 20px;
  }

  @media (max-width: 480px) {
    .sketches {
      grid-template-columns: 1fr;
    }
  }

  figure {
    position: relative;
    margin: 0;
  }

  .step {
    position: absolute;
    top: -8px;
    left: -8px;
    z-index: 1;
    display: grid;
    place-items: center;
    width: 22px;
    height: 22px;
    border-radius: 50%;
    background: var(--text);
    color: var(--surface);
    font-size: 12px;
    font-weight: 700;
  }

  .sketches i {
    display: block;
    height: 5px;
    margin: 5px 0;
    border-radius: 3px;
    background: currentColor;
    opacity: 0.35;
  }

  .sketches i.short {
    width: 60%;
  }

  .target {
    outline: 2px solid var(--accent);
    outline-offset: 2px;
    border-radius: 3px;
  }

  /* Windows: SmartScreen's blue panel. */
  .win {
    height: 100%;
    box-sizing: border-box;
    padding: 12px 12px 10px;
    border-radius: 6px;
    background: #1f5fa8;
    color: #fff;
    font-size: 11px;
    display: flex;
    flex-direction: column;
  }

  .win b {
    font-size: 13px;
    font-weight: 400;
    margin-bottom: 4px;
  }

  .win .link {
    align-self: flex-start;
    margin-top: 4px;
    padding: 0 2px;
    text-decoration: underline;
  }

  .win .buttons {
    display: flex;
    justify-content: flex-end;
    gap: 6px;
    margin-top: auto;
    padding-top: 10px;
  }

  .win .buttons span {
    padding: 3px 8px;
    border: 1px solid rgb(255 255 255 / 0.7);
  }

  .win .buttons .target {
    background: #fff;
    color: #1f5fa8;
  }

  /* macOS: the alert, then System Settings. */
  .mac-alert,
  .mac-settings {
    height: 100%;
    box-sizing: border-box;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface-2);
    font-size: 11px;
  }

  .mac-alert {
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 12px;
    text-align: center;
  }

  .mac-alert .app {
    width: 22px;
    height: 22px;
    margin-bottom: 6px;
    border-radius: 6px;
    background: var(--border);
  }

  .mac-alert i {
    align-self: stretch;
  }

  .mac-alert i.short {
    align-self: center;
  }

  .mac-alert .wide {
    align-self: stretch;
    margin-top: 6px;
    padding: 3px;
    border-radius: 5px;
    background: var(--surface);
  }

  .mac-alert .wide.target {
    background: var(--accent);
    color: var(--accent-text);
  }

  .mac-settings {
    display: flex;
    overflow: hidden;
  }

  .mac-settings .side {
    flex: none;
    width: 44%;
    padding: 10px 6px;
    border-right: 1px solid var(--border);
  }

  .mac-settings .side .on {
    display: block;
    margin: 4px 0;
    padding: 2px 4px;
    border-radius: 4px;
    background: var(--muted);
    color: var(--surface);
    font-size: 9px;
    white-space: nowrap;
    overflow: hidden;
    text-overflow: ellipsis;
  }

  .mac-settings .pane {
    display: flex;
    flex-direction: column;
    align-items: flex-start;
    gap: 6px;
    padding: 10px;
  }

  .mac-settings small {
    font-size: 10px;
    line-height: 1.35;
  }

  .mac-settings .pane .target {
    padding: 2px 8px;
    border-radius: 5px;
    background: var(--accent);
    color: var(--accent-text);
  }

  /* For the cautious: quieter, set apart. */
  .check {
    padding-top: 14px;
    border-top: 1px solid var(--border);
    font-size: 13px;
  }

  .check p {
    display: flex;
    gap: 8px;
    margin: 0 0 8px;
  }

  .check p:last-child {
    margin: 0;
  }

  .check :global(svg) {
    flex: none;
    margin-top: 3px;
  }
</style>
