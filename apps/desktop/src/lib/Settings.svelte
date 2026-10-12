<script lang="ts">
  import { onDestroy, onMount } from "svelte";
  import { api, events, type Device, type GpuStatus, type UnlistenFn } from "./api";
  import Icon from "./Icon.svelte";

  let { oncleared }: { oncleared: () => void } = $props();

  let device = $state<Device>("cpu");
  let cacheBytes = $state<number | null>(null);
  let clearing = $state(false);
  let gpu = $state<GpuStatus | null>(null);
  let step = $state<{ label: string; done: number; total: number } | null>(null);
  let error = $state<string | null>(null);
  const unlisten: UnlistenFn[] = [];
  let timer: ReturnType<typeof setInterval> | undefined;

  const gb = (mb: number) => `${(mb / 1024).toFixed(1)} GB`;

  async function refresh() {
    gpu = await api.gpuStatus();
    cacheBytes = await api.cacheSize();
  }

  const size = (bytes: number) =>
    bytes >= 1e9 ? `${(bytes / 1e9).toFixed(1)} GB` : `${Math.max(1, Math.round(bytes / 1e6))} MB`;

  async function clearCache() {
    const amount = cacheBytes ? `${size(cacheBytes)} of ` : "";
    const ok = confirm(
      `Delete ${amount}downloaded files? This removes the speech models, the GPU pack, yt-dlp and unfinished downloads. ` +
        "Running jobs are paused. Your library and transcripts are kept, and setup downloads what's needed again.",
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

  async function choose(d: Device) {
    device = d;
    await api.setSettings({ device: d }).catch((e) => (error = String(e)));
  }

  // Platforms change often enough that an old yt-dlp stops working.
  let updating = $state(false);
  let ytDlpStatus = $state<string | null>(null);
  async function updateYtDlp() {
    updating = true;
    ytDlpStatus = null;
    try {
      ytDlpStatus = await api.updateYtDlp();
    } catch (e) {
      ytDlpStatus = String(e);
    } finally {
      updating = false;
    }
  }

  async function install() {
    error = null;
    await api.installGpuPack().catch((e) => (error = String(e)));
    await refresh();
  }

  async function remove() {
    if (!confirm("Remove the GPU pack? Transcription will use the CPU.")) return;
    if (device === "gpu") await choose("cpu");
    await api.removeGpuPack().catch((e) => (error = String(e)));
    await refresh();
  }

  // Why GPU can't be chosen right now, if it can't.
  let blocker = $derived.by(() => {
    if (!gpu) return "Checking…";
    if (!gpu.supported) return "Not available on this operating system.";
    if (!gpu.worker) return "This build doesn't include GPU support.";
    if (!gpu.gpu) return "No NVIDIA GPU found.";
    if (!gpu.installed) return "Needs the GPU pack, a one-time download of about 3 GB.";
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
          <span class="option-title">CPU</span>
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
            GPU
            {#if gpu?.gpu}<span class="muted detail">{gpu.gpu.name}, {gb(gpu.gpu.free_mb)} free</span>{/if}
          </span>
          <span class="muted">
            {#if blocker}{blocker}{:else}NVIDIA only; several times faster than the CPU.{/if}
          </span>
        </span>
      </label>
    </div>
    {#if busy && gpu}
      <p class="hint warn-text">
        <Icon name="warning" size={13} /> The GPU is busy. VODs use the CPU until {gb(gpu.min_free_mb)} is free.
      </p>
    {/if}

    {#if offerPack}
      {#if step || gpu?.installing}
        <div class="progress">
          <span class="muted">Downloading {step?.label ?? "GPU pack"}…</span>
          <div class="bar"><div style:width="{step && step.total ? (step.done / step.total) * 100 : 0}%"></div></div>
        </div>
      {:else if !gpu?.installed}
        <button onclick={install}><Icon name="download" size={14} /> Download GPU pack</button>
      {:else}
        <button class="link" onclick={remove}>Remove GPU pack</button>
      {/if}
    {/if}
    {#if error}<p class="error-text hint">{error}</p>{/if}
    <p class="muted hint">Changes apply from the next VOD.</p>
  </section>

  <section>
    <span class="eyebrow">Downloader</span>
    <p class="muted hint">If VODs from a site start failing to download, a newer yt-dlp usually fixes it.</p>
    <button onclick={updateYtDlp} disabled={updating}>{updating ? "Checking…" : "Update yt-dlp"}</button>
    {#if ytDlpStatus}<p class="muted hint">{ytDlpStatus}</p>{/if}
  </section>

  <section>
    <span class="eyebrow">Storage</span>
    <p class="muted hint">
      Downloaded files{cacheBytes !== null ? `: ${size(cacheBytes)}` : ""}. Your library and transcripts aren't
      included.
    </p>
    <button class="danger-outline" onclick={clearCache} disabled={clearing || step !== null || gpu?.installing}>
      {clearing ? "Clearing…" : "Clear downloads…"}
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

  .bar {
    height: 4px;
    border-radius: 2px;
    background: var(--surface-3);
    overflow: hidden;
  }

  .bar > div {
    height: 100%;
    background: var(--accent);
    transition: width 0.2s;
  }
</style>
