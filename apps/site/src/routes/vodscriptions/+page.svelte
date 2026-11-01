<script lang="ts">
  // VODscriptions: the site's front for streamers and whoever pays for a
  // channel (the home page is the app's, for viewers, clippers and editors).
  // The live demo (OfferDemo.svelte) beside the offer, and the questions a
  // streamer asks. Sign-up isn't open yet: where it will be, a "Coming soon"
  // button and an email link (ComingSoon.svelte). Everything about the page
  // is in this folder, and its address is PREMIUM (lib/links.ts).
  import Icon from "desktop/lib/Icon.svelte";
  import Logo from "desktop/lib/Logo.svelte";
  import { CONTACT_EMAIL, PREMIUM } from "#lib/links.js";
  import ComingSoon from "./ComingSoon.svelte";
  import OfferDemo from "./OfferDemo.svelte";

  const TITLE = "VODscriptions: chapters, highlights and cast for every VOD on your channel";
  const DESCRIPTION =
    "Every VOD on your channel, with its chapters, highlights and who's speaking, usually within hours of the stream's end. Everyone sees them for free, in VODscribe and on vodscribe.com. $199 a month per channel.";

  const PARTS = [
    { icon: "layers", name: "Chapters", text: "The stream split into named parts, on the timeline and in a list." },
    { icon: "star", name: "Highlights", text: "The moments chat reacted to, each with its quote." },
    { icon: "users", name: "Cast", text: "Who spoke when, named only with their consent." },
  ] as const;

  const FAQ = [
    {
      q: "What's in a VODscription?",
      a: "The VOD's transcript, chapters, highlights with what chat said, and cast: who spoke, named where they've agreed to be. Viewers see them on the timeline and in a list beside the transcript.",
    },
    {
      q: "How soon after a stream?",
      a: "Usually within hours of the stream's end, as best effort with no fixed time. A new VODscription also covers the channel's VODs from up to a month back.",
    },
    {
      q: "Which platforms?",
      a: "Twitch, YouTube and Kick.",
    },
    {
      q: "Who can see it?",
      a: "Everyone, for free: in the VODscribe app, which needs no account, and on this site.",
    },
    {
      q: "Who gets named?",
      a: "Only people who've agreed to be. Anyone else who speaks is in the cast without a name.",
    },
    {
      q: "Can someone else pay for my channel?",
      a: "Yes, a clipper, an editor or a manager can, once you, the channel's owner, sign in and say yes.",
    },
    {
      q: "Is there a rate for small channels?",
      a: "Sometimes, on different terms from the standard VODscription. Email and say what you stream and how often; you'll get a straight answer.",
      link: { text: "Send an email", href: `mailto:${CONTACT_EMAIL}?subject=VODscriptions` },
    },
    {
      q: "What if I stop paying?",
      a: "New VODs stop getting one. Everything already published stays for everyone to see.",
    },
    {
      q: "When can I sign up?",
      a: "Soon.",
      link: { text: "Send me an email and you'll hear first.", href: `mailto:${CONTACT_EMAIL}?subject=VODscriptions` },
    },
  ];
</script>

<svelte:head>
  <title>{TITLE}</title>
  <meta name="description" content={DESCRIPTION} />
  <meta property="og:title" content={TITLE} />
  <meta property="og:description" content={DESCRIPTION} />
  <meta property="og:type" content="website" />
  <meta property="og:url" content="https://vodscribe.com{PREMIUM}" />
  <meta property="og:image" content="https://vodscribe.com/og-vodscriptions.jpg" />
  <meta property="og:image:width" content="1200" />
  <meta property="og:image:height" content="630" />
  <link rel="canonical" href="https://vodscribe.com{PREMIUM}" />
  <meta name="twitter:card" content="summary_large_image" />
  <meta name="twitter:image" content="https://vodscribe.com/og-vodscriptions.jpg" />
</svelte:head>

<main>
  <section class="hero">
    <!-- The service's mark and name, set as the header sets the app's, at display size. -->
    <h1>
      <Logo size={84} tile mark="vodscriptions" />
      <span>VODscriptions</span>
    </h1>
    <p class="lead">Chapters, highlights and who's speaking, for every VOD on your channel.</p>
  </section>

  <section class="offer">
    <div class="demo">
      <div class="wide">
        <OfferDemo />
      </div>
      <div class="narrow">
        <img
          src="/screenshot.jpg"
          alt="VODscribe with a NASA VOD open and every mention of “heat shield” found"
          width="1132"
          height="623"
        />
        <p class="caption">
          <strong>The live demo needs a bigger screen.</strong>
          <span class="muted">Open this page on a computer to try it.</span>
        </p>
      </div>
    </div>

    <div class="side">
      <p class="intro">
        Usually ready within hours of the stream's end. Your viewers and clippers see them for free, in VODscribe and on
        this site.
      </p>
      <ul class="parts">
        {#each PARTS as part (part.name)}
          <li>
            <Icon name={part.icon} size={20} />
            <div>
              <h2>{part.name}</h2>
              <p class="muted">{part.text}</p>
            </div>
          </li>
        {/each}
      </ul>

      <div class="price" id="coming-soon">
        <p class="amount"><strong>$199</strong> a month, per channel</p>
        <p class="muted">
          Every new VOD while it runs, and the channel's VODs from up to a month back. Cancel any time; what's published
          stays.
        </p>
        <ComingSoon />
      </div>

      <p class="small trust">
        Nothing is produced until the channel's owner signs in once and says yes, even when someone else pays.
      </p>
      <p class="small trust">
        <strong>Smaller channel?</strong> A lower rate on different terms may be available.
        <a href={`mailto:${CONTACT_EMAIL}?subject=VODscriptions`}>Send an email</a> and say what you stream.
      </p>
    </div>
  </section>

  <section class="more">
    <div>
      <h2>Paying for a channel you work with</h2>
      <p class="muted">Clippers, editors and managers can pay for a channel's VODscription.</p>
    </div>
    <div>
      <h2>Privacy</h2>
      <p class="muted">
        There's no sign-in here yet, so this page knows nothing about you, and the email link opens your own mail app.
        <a href="/privacy">How VODscribe and this site handle data</a>
      </p>
    </div>
  </section>

  <section class="faq">
    <h2>Questions</h2>
    {#each FAQ as item (item.q)}
      <details>
        <summary>{item.q}</summary>
        <p class="muted">
          {item.a}
          {#if item.link}<a href={item.link.href}>{item.link.text}</a>{/if}
        </p>
      </details>
    {/each}
  </section>
</main>

<style>
  main {
    max-width: 1560px;
    margin: 0 auto;
    padding: 0 24px;
  }

  section {
    margin: 0 auto 88px;
  }

  .hero {
    max-width: 760px;
    margin-bottom: 48px;
    padding-top: 48px;
    text-align: center;
  }

  /* The header's wordmark (+layout.svelte .brand) at display size: the mark
     beside the name, one above the other on a phone. */
  h1 {
    display: flex;
    align-items: center;
    justify-content: center;
    gap: 20px;
    margin: 0 0 20px;
    font-size: clamp(34px, 6vw, 56px);
    font-weight: 700;
    line-height: 1.08;
    letter-spacing: -0.03em;
  }

  @media (max-width: 560px) {
    h1 {
      flex-direction: column;
      gap: 14px;
    }
  }

  .lead {
    margin: 0 auto;
    max-width: 720px;
    font-size: 19px;
    color: var(--muted);
  }

  /* The demo on the left, the offer on the right; one above the other on a
     narrower window. */
  .offer {
    display: grid;
    grid-template-columns: minmax(0, 1fr) 340px;
    gap: 40px;
    align-items: start;
  }

  @media (max-width: 1100px) {
    .offer {
      grid-template-columns: minmax(0, 1fr);
    }

    .side {
      max-width: 560px;
      margin: 0 auto;
    }
  }

  .narrow {
    display: none;
  }

  /* As on the home page: the screenshot keeps its aspect box, the caption over it. */
  .narrow {
    position: relative;
    aspect-ratio: 1132 / 623;
    border: 1px solid var(--border);
    border-radius: 12px;
    overflow: hidden;
  }

  .narrow img {
    display: block;
    width: 100%;
    height: 100%;
    filter: blur(3px) brightness(0.5);
    transform: scale(1.04);
  }

  .caption {
    position: absolute;
    top: 50%;
    left: 50%;
    transform: translate(-50%, -50%);
    width: max-content;
    max-width: calc(100% - 32px);
    margin: 0;
    padding: 12px 16px;
    border: 1px solid var(--border);
    border-radius: var(--radius);
    background: var(--surface);
    box-shadow: var(--shadow);
    line-height: 1.4;
    text-align: center;
  }

  .caption strong,
  .caption span {
    display: block;
  }

  @media (max-width: 820px) {
    .wide {
      display: none;
    }

    .narrow {
      display: block;
    }
  }

  .intro {
    margin: 0 0 24px;
  }

  .parts {
    list-style: none;
    margin: 0 0 24px;
    padding: 0;
  }

  .parts li {
    display: flex;
    gap: 12px;
  }

  .parts li + li {
    margin-top: 16px;
  }

  .parts :global(svg) {
    flex: none;
    margin-top: 2px;
    color: var(--accent);
  }

  .parts h2 {
    margin: 0 0 2px;
    font-size: 16px;
  }

  .parts p {
    margin: 0;
  }

  .price {
    /* Sign in in the header lands here (+layout.svelte): the card whole, not under the window's edge. */
    scroll-margin-top: 24px;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 20px;
    border: 1px solid var(--border);
    border-radius: 12px;
    background: var(--surface);
    box-shadow: var(--shadow);
    text-align: center;
  }

  .price p {
    margin: 0 0 14px;
  }

  .amount {
    font-size: 17px;
  }

  .amount strong {
    font-size: 34px;
    letter-spacing: -0.02em;
  }

  .trust {
    margin: 16px 0 0;
  }

  .small {
    font-size: 13px;
  }

  .more {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(260px, 1fr));
    gap: 28px;
    max-width: 960px;
  }

  .more h2 {
    margin: 0 0 6px;
    font-size: 16px;
  }

  .more p {
    margin: 0;
  }

  .faq {
    max-width: 720px;
  }

  .faq h2 {
    font-size: 28px;
    letter-spacing: -0.02em;
    margin: 0 0 20px;
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
