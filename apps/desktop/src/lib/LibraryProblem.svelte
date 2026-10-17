<!--
  Shown instead of the app when its library won't open (recovery.rs): what
  went wrong, and what can be done. Nothing here deletes anything: starting
  a new library sets the old file aside, kept for recovering or for a bug
  report.
-->
<script lang="ts">
  import { onMount } from "svelte";
  import { getCurrentWindow } from "@tauri-apps/api/window";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { relaunch } from "@tauri-apps/plugin-process";
  import { api } from "./api";
  import Icon from "./Icon.svelte";
  import Logo from "./Logo.svelte";
  import { ISSUES } from "./links";
  import { theme } from "./theme.svelte";

  let { error }: { error: string } = $props();

  let busy = $state(false);
  let failed = $state<string | null>(null);

  onMount(() => {
    // The settings live beside the library, so the system's theme.
    theme.apply("system");
    void getCurrentWindow()
      .show()
      .catch(() => {});
  });

  async function startNew() {
    busy = true;
    failed = null;
    try {
      await api.setLibraryAside();
      await relaunch();
    } catch (e) {
      failed = String(e);
      busy = false;
    }
  }
</script>

<div class="problem">
  <div class="brand">
    <Logo size={26} tile />
    <span class="name">VODscribe</span>
  </div>
  <h1>Your library couldn't be opened</h1>
  <p class="muted lead">
    VODscribe can't read the file that holds your VODs, transcripts, clips and edits. Nothing has been deleted.
  </p>
  <pre class="error">{error}</pre>

  <div class="choices">
    <div class="choice">
      <button onclick={() => relaunch()} disabled={busy}>Try again</button>
      <p class="muted">If something else had the file open, or the drive it's on was away, it may open now.</p>
    </div>
    <div class="choice">
      <button class="primary" onclick={startNew} disabled={busy}>Start a new library</button>
      <p class="muted">
        Starts empty. The old file is kept beside it, renamed <code>library-broken-</code> with today's date, so it can be
        recovered or sent with a bug report.
      </p>
    </div>
  </div>

  {#if failed}
    <p class="error-text"><Icon name="error" size={14} /> {failed}</p>
  {/if}

  <div class="links">
    <button class="link" onclick={() => api.showLibrary()}>Show the file</button>
    <button class="link" onclick={() => openUrl(ISSUES)}>Report the problem</button>
  </div>
</div>

<style>
  .problem {
    max-width: 540px;
    margin: 9vh auto;
    padding: 0 24px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 8px;
    margin-bottom: 22px;
  }

  .name {
    font-weight: 700;
    font-size: 15px;
  }

  h1 {
    margin: 0 0 6px;
    font-size: 22px;
    line-height: 1.25;
    letter-spacing: -0.01em;
  }

  .lead {
    margin: 0 0 14px;
  }

  .error {
    margin: 0 0 20px;
    padding: 10px 12px;
    border: 1px solid var(--border);
    border-radius: 8px;
    background: var(--surface);
    font-size: 12px;
    white-space: pre-wrap;
    word-break: break-word;
    user-select: text;
  }

  .choices {
    display: flex;
    flex-direction: column;
    gap: 14px;
    margin-bottom: 20px;
  }

  .choice {
    display: flex;
    align-items: flex-start;
    gap: 14px;
  }

  .choice button {
    flex: none;
    min-width: 160px;
  }

  .choice p {
    margin: 4px 0 0;
    font-size: 13px;
  }

  .links {
    display: flex;
    gap: 16px;
  }
</style>
