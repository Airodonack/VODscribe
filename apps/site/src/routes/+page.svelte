<script lang="ts">
  import { onMount } from "svelte";
  import Icon from "$desktop/Icon.svelte";
  import Demo from "$lib/Demo.svelte";
  import { DOWNLOADS, VERSION, detectOs, type Os } from "$lib/downloads";
  import { SUPPORT } from "$lib/links";

  // The main button offers this visitor's OS; every installer is listed below it.
  let os = $state<Os | null>(null);
  onMount(() => (os = detectOs()));
  let primary = $derived(DOWNLOADS.find((d) => d.os === os));

  const FAQ = [
    {
      q: "Is it free?",
      a: "Yes. No account, no ads, no trial. If it saves you time, you can chip in on Ko-fi to help cover hosting and code signing.",
    },
    {
      q: "Do my VODs or transcripts get uploaded anywhere?",
      a: "No. VODscribe downloads a VOD's audio to your computer and transcribes it there. Transcripts and searches never leave your computer, and VODscribe collects nothing about how you use it.",
    },
    {
      q: "Which VODs can it transcribe?",
      a: "Kick, Twitch and YouTube links: past broadcasts, highlights and uploads. It also takes video and audio files from your computer.",
    },
    {
      q: "Which languages does it understand?",
      a: "English and 24 other European languages, using NVIDIA's Parakeet speech model. It's very good with clear speech; names and game terms sometimes come out spelled differently.",
    },
    {
      q: "How fast is it?",
      a: "It depends on your computer. A recent processor transcribes an hour of VOD in minutes, and an NVIDIA graphics card with the free GPU add-on is several times faster.",
    },
    {
      q: "Why does Windows or macOS warn me before opening it?",
      a: "VODscribe isn't code-signed yet, which costs a yearly fee. On Windows, choose More info, then Run anyway. On a Mac, open it once, then go to System Settings › Privacy & Security and choose Open Anyway.",
    },
  ];
</script>

<svelte:head>
  <title>VODscribe: search everything said in your VODs</title>
  <meta
    name="description"
    content="VODscribe transcribes Kick, Twitch and YouTube VODs on your computer, so you can find any moment by what was said. Free for Windows, macOS and Linux."
  />
  <meta property="og:title" content="VODscribe: search everything said in your VODs" />
  <meta
    property="og:description"
    content="Transcribe Kick, Twitch and YouTube VODs on your computer and find any moment by what was said."
  />
  <meta property="og:type" content="website" />
  <meta property="og:url" content="https://vodscribe.com/" />
  <meta property="og:image" content="https://vodscribe.com/og.jpg" />
  <meta name="twitter:card" content="summary_large_image" />
</svelte:head>

<main>
  <section class="hero">
    <h1>Search everything said in your VODs</h1>
    <p class="lead">
      VODscribe transcribes Kick, Twitch and YouTube VODs on your computer. Find any moment by what was said, and jump
      straight to it.
    </p>
    <div class="ctas">
      <a class="button primary big" href={primary?.url ?? "#download"}>
        <Icon name="download" size={18} />
        {primary ? `Download for ${primary.label}` : "Download"}
      </a>
      <a class="button big" href={SUPPORT} target="_blank" rel="noopener">
        <span class="heart">❤︎</span> Support on Ko-fi
      </a>
    </div>
    <p class="small muted">Free · Windows, macOS and Linux · Version {VERSION}</p>
  </section>

  <section class="demo">
    <p class="demo-intro">
      <strong>This is the real app, running in your browser.</strong>
      <span class="muted">Two VODs are already transcribed: search them, or pick one to read along and play.</span>
    </p>
    <div class="wide">
      <Demo />
    </div>
    <div class="narrow">
      <img
        src="/screenshot.jpg"
        alt="VODscribe with a NASA VOD open and every mention of “heat shield” found"
        width="1132"
        height="623"
      />
      <p class="muted small">The live demo needs a bigger screen. Open this page on a computer to try it.</p>
    </div>
  </section>

  <section class="features">
    <div>
      <Icon name="search" size={22} />
      <h3>Every word, searchable</h3>
      <p class="muted">
        Type a phrase to see every time it was said, across all your VODs. Click one to play from there.
      </p>
    </div>
    <div>
      <Icon name="shield" size={22} />
      <h3>Stays on your computer</h3>
      <p class="muted">Transcribing happens on your own computer. No account, no uploads, nothing collected.</p>
    </div>
    <div>
      <Icon name="film" size={22} />
      <h3>Kick, Twitch, YouTube and files</h3>
      <p class="muted">Paste a link or drop a video file. VODs queue up and transcribe one after another.</p>
    </div>
    <div>
      <Icon name="zap" size={22} />
      <h3>Faster with a GPU</h3>
      <p class="muted">Works on any recent computer. An NVIDIA graphics card makes it several times faster.</p>
    </div>
  </section>

  <section class="download" id="download">
    <h2>Download VODscribe {VERSION}</h2>
    <ul class="files">
      {#each DOWNLOADS as d (d.url)}
        <li class:mine={d.os === os}>
          <a href={d.url}>
            <Icon name="download" size={16} />
            <span class="name">{d.label}</span>
            <span class="muted">{d.detail}</span>
          </a>
        </li>
      {/each}
    </ul>
    <p class="muted small">
      For 64-bit Windows 10 and 11, macOS (Apple Silicon and Intel) and Linux. On first run it downloads what it needs
      to transcribe (about 1 GB), checking each file before using it.
    </p>
  </section>

  <section class="faq">
    <h2>Questions</h2>
    {#each FAQ as item (item.q)}
      <details>
        <summary>{item.q}</summary>
        <p class="muted">{item.a}</p>
      </details>
    {/each}
  </section>
</main>

<style>
  main {
    max-width: 1180px;
    margin: 0 auto;
    padding: 0 24px;
  }

  section {
    margin: 0 auto 88px;
  }

  .hero {
    max-width: 760px;
    padding-top: 48px;
    text-align: center;
  }

  h1 {
    margin: 0 0 16px;
    font-size: clamp(34px, 6vw, 56px);
    line-height: 1.08;
    letter-spacing: -0.03em;
  }

  .lead {
    margin: 0 auto 28px;
    max-width: 600px;
    font-size: 19px;
    color: var(--muted);
  }

  .ctas {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 12px;
    margin-bottom: 14px;
  }

  .button {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    color: var(--text);
    text-decoration: none;
    font-weight: 600;
  }

  .button:hover {
    background: var(--surface-2);
  }

  .button.primary {
    background: var(--accent);
    border-color: var(--accent);
    color: var(--accent-text);
  }

  .button.primary:hover {
    filter: brightness(1.08);
  }

  .big {
    padding: 12px 22px;
    font-size: 16px;
  }

  .heart {
    color: #e5484d;
  }

  .small {
    font-size: 13px;
  }

  .demo {
    text-align: center;
  }

  .demo-intro {
    margin: 0 0 14px;
  }

  .narrow {
    display: none;
  }

  .narrow img {
    width: 100%;
    height: auto;
    border: 1px solid var(--border);
    border-radius: 12px;
  }

  @media (max-width: 820px) {
    .wide {
      display: none;
    }

    .narrow {
      display: block;
    }
  }

  .features {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: 28px;
  }

  .features :global(svg) {
    color: var(--accent);
  }

  .features h3 {
    margin: 10px 0 6px;
    font-size: 16px;
  }

  .features p {
    margin: 0;
  }

  .download {
    max-width: 640px;
    text-align: center;
  }

  h2 {
    font-size: 28px;
    letter-spacing: -0.02em;
    margin: 0 0 20px;
  }

  .files {
    list-style: none;
    margin: 0 0 16px;
    padding: 0;
    border: 1px solid var(--border);
    border-radius: 10px;
    background: var(--surface);
    text-align: left;
  }

  .files li + li {
    border-top: 1px solid var(--border);
  }

  .files a {
    display: flex;
    align-items: center;
    gap: 10px;
    padding: 12px 16px;
    color: var(--text);
    text-decoration: none;
  }

  .files a:hover {
    background: var(--surface-2);
  }

  .files .name {
    font-weight: 600;
  }

  .files .muted {
    margin-left: auto;
    font-size: 13px;
    text-align: right;
  }

  .files li.mine .name {
    color: var(--accent);
  }

  .faq {
    max-width: 720px;
  }

  details {
    border-bottom: 1px solid var(--border);
    padding: 14px 0;
  }

  summary {
    cursor: pointer;
    font-weight: 600;
  }

  details p {
    margin: 8px 0 0;
  }
</style>
