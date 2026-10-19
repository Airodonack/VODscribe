<script lang="ts">
  import "desktop/app.css";
  import "../site.css";
  import Logo from "desktop/lib/Logo.svelte";
  import { AUTHOR, SOURCE, SUPPORT } from "#lib/links.js";

  let { children } = $props();
</script>

<header class="top">
  <a class="brand" href="/">
    <Logo size={28} tile />
    <span>VODscribe</span>
  </a>
  <nav>
    <a class="download-link" href="/#download">Download</a>
    <a href={SUPPORT} target="_blank" rel="noopener">Support on Ko-fi</a>
  </nav>
</header>

{@render children()}

<footer>
  <span>Made with <span class="heart">❤︎</span> by {AUTHOR}</span>
  <a href={SUPPORT} target="_blank" rel="noopener">Ko-fi</a>
  <a href="/releases">Releases</a>
  <a href="/privacy">Privacy</a>
  <a href="/licenses">Licenses</a>
  <a href={SOURCE} target="_blank" rel="noopener">Source</a>
</footer>

<style>
  .top {
    display: flex;
    align-items: center;
    justify-content: space-between;
    gap: 16px;
    max-width: 1180px;
    margin: 0 auto;
    padding: 18px 24px;
  }

  .brand {
    display: flex;
    align-items: center;
    gap: 10px;
    color: var(--text);
    font-weight: 700;
    font-size: 17px;
    text-decoration: none;
  }

  nav {
    display: flex;
    gap: 20px;
    font-size: 14px;
  }

  nav a {
    color: var(--muted);
    text-decoration: none;
    white-space: nowrap;
  }

  @media (max-width: 480px) {
    .top {
      padding: 16px;
    }

    nav {
      gap: 14px;
    }
  }

  @media (max-width: 360px) {
    .download-link {
      display: none;
    }
  }

  nav a:hover {
    color: var(--text);
  }

  footer {
    display: flex;
    flex-wrap: wrap;
    justify-content: center;
    gap: 8px 22px;
    padding: 40px 24px 48px;
    border-top: 1px solid var(--border);
    color: var(--muted);
    font-size: 13px;
  }

  footer a {
    color: var(--muted);
  }

  footer a:hover {
    color: var(--text);
  }

  .heart {
    color: #e5484d;
  }
</style>
