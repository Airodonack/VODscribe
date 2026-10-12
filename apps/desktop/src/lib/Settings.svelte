<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type Device, type GpuStatus, type UnlistenFn } from "./api";
  import { openUrl } from "@tauri-apps/plugin-opener";
  import Icon from "./Icon.svelte";
  import { PRIVACY } from "./links";
  import Progress from "./Progress.svelte";

  let { oncleared }: { oncleared: () => void } = $props();

  let device = $state<Device>("cpu");
  let cacheBytes = $state<number | null>(null);
  let clearing = $state(false);
  let gpu = $state<GpuStatus | null>(null);
  let step = $state<{ label: string; done: number; total: number } | null>(null);
  let error = $state<string | null>(null);
  const unlisten: UnlistenFn[] = [];
  let timer: ReturnType<typeof setInterval> | undefined;

  async function refresh() {
    gpu = await api.gpuStatus();
    cacheBytes = await api.cacheSize();
  }

  const size = (bytes: number) =>
    bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`;

  async function clearCache() {
    const amount = cacheBytes ? ` ${size(cacheBytes)}` : "";
    const ok = confirm(
      `Free up${amount}? This deletes the parts VODscribe downloaded to run, and any unfinished downloads. ` +
        "Anything in progress is paused. Your library and transcripts are kept, and VODscribe downloads what it needs again.",
    );
    if (!ok) return;
    clearing = true;
    error = null;
    try {
      await api.clearCache();
      oncleared();
    } catch (e) {
      error = String(e);
    } finally {
      clearing = false;
      await refresh();
    }
  }

  // The popover can close before this async setup finishes; anything set up
  // after that is torn down straight away.
  let destroyed = false;
  const keep = (u: UnlistenFn) => (destroyed ? u() : unlisten.push(u));

  onMount(async () => {
    device = (await api.getSettings()).device;
    keep(await events.gpuProgress((label, done, total) => (step = { label, done, total })));
    keep(await events.ytDlpProgress((label, done) => (ytDlpStep = { label, done })));
    keep(
      await events.gpuDone((e) => {
        step = null;
        error = e;
        refresh();
      }),
    );
    await refresh();
    // Free VRAM changes as other programs use the GPU.
    if (!destroyed) timer = setInterval(refresh, 5000);
  });
  onDestroy(() => {
    destroyed = true;
    clearInterval(timer);
    unlisten.forEach((u) => u());
  });

  const mb = (bytes: number) => Math.round(bytes / 1e6).toLocaleString();

  let dataDir = $state<string | null>(null);
  api.dataDir().then(
    (d) => (dataDir = d),
    () => {},
  );

  async function choose(d: Device) {
    device = d;
    await api.setSettings({ device: d }).catch((e) => (error = String(e)));
  }

  // Platforms change often enough that an old yt-dlp stops working.
  let updating = $state(false);
  let ytDlpStatus = $state<string | null>(null);
  // The step under way, named like the GPU add-on's: signature check, download, install.
  let ytDlpStep = $state<{ label: string; done: number } | null>(null);
  async function updateYtDlp() {
    updating = true;
    ytDlpStatus = null;
    try {
      ytDlpStatus = await api.updateYtDlp();
    } catch (e) {
      ytDlpStatus = String(e);
    } finally {
      updating = false;
      ytDlpStep = null;
    }
  }

  async function install() {
    error = null;
    await api.installGpuPack().catch((e) => (error = String(e)));
    await refresh();
  }

  async function remove() {
    if (!confirm("Remove the GPU add-on? VODs will be transcribed with the processor.")) return;
    if (device === "gpu") await choose("cpu");
    await api.removeGpuPack().catch((e) => (error = String(e)));
    await refresh();
  }

  // Why GPU can't be chosen right now, if it can't.
  let blocker = $derived.by(() => {
    if (!gpu) return "Checking…";
    if (!gpu.supported) return "Not available on this kind of computer.";
    if (!gpu.worker) return "This version of VODscribe can't use graphics cards.";
    if (!gpu.gpu) return "No NVIDIA graphics card found.";
    if (!gpu.installed) return "Needs the GPU add-on, a one-time download of about 3 GB.";
    return null;
  });
  let busy = $derived(gpu?.gpu && gpu.installed && gpu.gpu.free_mb < gpu.min_free_mb);
  let offerPack = $derived(gpu?.supported && gpu.worker && gpu.gpu);
</script>

<div class="settings">
  <section>
    <span class="eyebrow">Transcribe with</span>
    <div class="options" role="radiogroup" aria-label="Transcribe with">
      <label class="option" class:checked={device === "cpu"}>
        <input type="radio" name="device" checked={device === "cpu"} onchange={() => choose("cpu")} />
        <span class="option-text">
          <span class="option-title">Processor <span class="muted">(CPU)</span></span>
          <span class="muted">Works on any computer.</span>
        </span>
      </label>
      <label class="option" class:checked={device === "gpu"} class:disabled={blocker !== null}>
        <input
          type="radio"
          name="device"
          checked={device === "gpu"}
          disabled={blocker !== null}
          onchange={() => choose("gpu")}
        />
        <span class="option-text">
          <span class="option-title">
            Graphics card <span class="muted">(GPU)</span>
            {#if gpu?.gpu}<span class="muted detail">{gpu.gpu.name}</span>{/if}
          </span>
          <span class="muted">
            {#if blocker}{blocker}{:else}NVIDIA only. Several times faster.{/if}
          </span>
        </span>
      </label>
    </div>
    {#if busy && gpu}
      <p class="hint warn-text">
        <Icon name="warning" size={13} /> Your graphics card is busy with something else, so VODs use the processor for now.
      </p>
    {/if}

    {#if offerPack}
      {#if step || gpu?.installing}
        <div class="progress">
          <!-- The exact component, for the people who chose the GPU add-on. -->
          <span class="muted step">
            {step?.label ?? "Downloading the GPU add-on"}…
            {#if step?.label.startsWith("Downloading") && step.total}
              <span class="mb">{mb(step.done)} / {mb(step.total)} MB</span>
            {/if}
          </span>
          <Progress value={step && step.total ? step.done / step.total : 0} />
        </div>
      {:else if !gpu?.installed}
        <button onclick={install}><Icon name="download" size={14} /> Download GPU add-on (about 3 GB)</button>
      {:else}
        <p
          class="verified"
          title="VODscribe checked that every file is exactly what was published, so nothing was damaged or tampered with."
        >
          <Icon name="shield" size={12} /> GPU add-on installed and checked
        </p>
        <button class="link" onclick={remove}>Remove GPU add-on</button>
      {/if}
    {/if}
    {#if error}<p class="error-text hint">{error}</p>{/if}
    <p class="muted hint">Changes apply from the next VOD.</p>
  </section>

  <section>
    <span class="eyebrow">Video downloader</span>
    <p class="muted hint">If VODs from a site stop downloading, updating usually fixes it.</p>
    <button onclick={updateYtDlp} disabled={updating}>{updating ? "Updating…" : "Update video downloader"}</button>
    {#if updating && ytDlpStep}
      <span class="muted step">
        {ytDlpStep.label}…
        {#if ytDlpStep.label.startsWith("Downloading") && ytDlpStep.done}
          <span class="mb">{mb(ytDlpStep.done)} MB</span>
        {/if}
      </span>
    {:else if ytDlpStatus}
      <p class="muted hint step">{ytDlpStatus}</p>
    {/if}
  </section>

  <section>
    <span class="eyebrow">Privacy & security</span>
    <p class="hint">
      Your transcripts, audio and searches stay on this computer. There's no account, and VODscribe doesn't collect
      anything about how you use it.
    </p>
    <p class="muted hint">
      VODscribe only goes online to get the VODs you add, to play them, and to download the parts it needs to run. Those
      parts are checked before they're used, to make sure they haven't been tampered with. Video files from your
      computer never leave it.
    </p>
    {#if dataDir}
      <p class="muted hint">Your library is in:</p>
      <code class="path">{dataDir}</code>
    {/if}
    <div class="row-buttons">
      <button onclick={() => api.openDataDir().catch((e) => (error = String(e)))}>Open folder</button>
      <button class="link" onclick={() => openUrl(PRIVACY)}
        >More about privacy <Icon name="external" size={12} /></button
      >
    </div>
  </section>

  <section>
    <span class="eyebrow">Storage</span>
    <p class="muted hint">
      Downloaded parts and unfinished downloads{cacheBytes !== null ? ` use ${size(cacheBytes)}` : ""}. Your library and
      transcripts aren't counted.
    </p>
    <button class="danger-outline" onclick={clearCache} disabled={clearing || step !== null || gpu?.installing}>
      {clearing ? "Clearing…" : "Free up space…"}
    </button>
  </section>
</div>

<style>
  .settings {
    display: grid;
  }

  section {
    display: grid;
    gap: 8px;
    padding: 14px 16px;
    justify-items: start;
  }

  .step {
    font-size: 12px;
  }

  .mb {
    font-family: var(--mono);
    font-variant-numeric: tabular-nums;
  }

  .verified {
    display: flex;
    align-items: center;
    gap: 5px;
    margin: 0;
    font-size: 12px;
    color: var(--ok);
  }

  .path {
    font-size: 11px;
    word-break: break-all;
  }

  .row-buttons {
    display: flex;
    align-items: center;
    gap: 14px;
  }

  section + section {
    border-top: 1px solid var(--border);
  }

  .options {
    display: grid;
    gap: 6px;
    width: 100%;
  }

  .option {
    display: flex;
    gap: 10px;
    align-items: flex-start;
    padding: 8px 10px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    cursor: pointer;
    font-size: 12px;
  }

  .option:hover:not(.disabled) {
    background: var(--surface-2);
  }

  .option.checked {
    border-color: var(--accent);
    background: var(--accent-soft);
  }

  .option.disabled {
    cursor: default;
    opacity: 0.65;
  }

  .option input {
    margin: 2px 0 0;
  }

  .option-text {
    display: grid;
    gap: 1px;
    min-width: 0;
  }

  .option-title {
    font-size: 13px;
    font-weight: 600;
  }

  .detail {
    display: block;
    font-weight: 400;
  }

  .hint {
    margin: 0;
    font-size: 12px;
    display: flex;
    gap: 5px;
    align-items: center;
  }

  .progress {
    display: grid;
    gap: 4px;
    font-size: 12px;
    width: 100%;
  }
</style>
