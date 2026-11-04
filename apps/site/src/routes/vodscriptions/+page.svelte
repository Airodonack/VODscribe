<script lang="ts">
  // VODscriptions: the site's front for creators and whoever pays for a
  // channel (the home page is the app's, for viewers, clippers and editors).
  // The live demo (OfferDemo.svelte), what a VODscription holds, the offer
  // (the hero's "See pricing" jumps to it), then privacy and the
  // smaller-channel note, each on its own row. Sign-up isn't open yet: where it
  // will be, a "Coming soon" button and an email link (ComingSoon.svelte).
  // Everything about the page is in this folder, and its address is PREMIUM
  // (lib/links.ts).
  import Icon from "desktop/lib/Icon.svelte";
  import Logo from "desktop/lib/Logo.svelte";
  import { CONTACT_MAILTO, PREMIUM, SUPPORT_MAILTO } from "#lib/links.js";
  import ComingSoon from "./ComingSoon.svelte";
  import OfferDemo from "./OfferDemo.svelte";
  import PlatformMark from "./PlatformMark.svelte";

  const TITLE = "VODscriptions: chapters, highlights and cast for every VOD on your channel";
  const DESCRIPTION =
    "Chapters, highlights and who's speaking for every VOD on your channel, usually within hours of the stream's end. Free to see in VODscribe and on vodscribe.com. $99 a month per channel.";

  const PARTS = [
    {
      icon: "layers",
      name: "Chapters",
      text: "Your stream in named parts, on the timeline and in a list beside the transcript.",
    },
    { icon: "star", name: "Highlights", text: "The moments chat reacted to, each with its quote." },
    { icon: "users", name: "Cast", text: "Who spoke when. Anyone who hasn't agreed to be named stays nameless." },
  ] as const;

  /** The platforms, each shown with its own mark (PlatformMark.svelte). */
  const PLATFORMS = [
    { id: "twitch", name: "Twitch" },
    { id: "youtube", name: "YouTube" },
    { id: "kick", name: "Kick" },
  ] as const;

  // The FAQ is kept, not shown, for when there are more questions than these.
  /* const FAQ = [
    {
      q: "Which platforms?",
      a: "Twitch, YouTube and Kick.",
    },
    {
      q: "Is there a rate for small channels?",
      a: "Sometimes, on different terms from the standard VODscription. Email and say what you stream and how often; you'll get a straight answer.",
      link: { text: "Send an email", href: CONTACT_MAILTO },
    },
  ]; */
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
    <!-- A quiet way down to the price, below the demo and the features. -->
    <a class="to-pricing" href="#pricing">See pricing <Icon name="chevron-down" size={14} /></a>
  </section>

  <section class="demo">
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
  </section>

  <section class="features">
    <ul class="parts">
      {#each PARTS as part (part.name)}
        <li>
          <h2><Icon name={part.icon} size={20} />{part.name}</h2>
          <p class="muted">{part.text}</p>
        </li>
      {/each}
      <li>
        <h2><Icon name="play" size={20} />Platforms</h2>
        <ul class="platforms">
          {#each PLATFORMS as platform (platform.id)}
            <li><PlatformMark platform={platform.id} size={40} /><span>{platform.name}</span></li>
          {/each}
        </ul>
      </li>
    </ul>
  </section>

  <!-- The price card is the focus; the facts that go with it sit beside it,
       one above the other, and under it on a phone. -->
  <section class="offer" id="pricing">
    <div class="price" id="coming-soon">
      <p class="amount"><strong>$99</strong> a month, per channel</p>
      <p class="muted">
        Every new VOD while you subscribe, plus your VODs from up to a month back. Cancel any time; what's published
        stays as long as you want.
      </p>
      <ComingSoon />
    </div>

    <ul class="facts">
      <li>
        <h2>Supercharge any channel</h2>
        <p class="muted">
          Your VODs come with chapters, highlights and cast, so the best moments are easy to find and clip. Pay for your
          own channel or one you clip, edit or manage.
        </p>
      </li>
      <li>
        <h2>We wait for your consent</h2>
        <p class="muted">
          Nothing is charged or made until the creator signs in and says yes, even when someone else pays.
        </p>
      </li>
      <li>
        <h2>See annotations within hours</h2>
        <p class="muted">
          Once your stream ends, they're made automatically and show up in VODscribe and on this site. If anything looks
          wrong, <a href={SUPPORT_MAILTO}>let us know right away</a>.
        </p>
      </li>
    </ul>
  </section>

  <!-- Privacy, and beside it the note for smaller channels, set as the features are. -->
  <section class="more">
    <div>
      <h2><Icon name="shield" size={20} />You choose who sees them</h2>
      <p class="muted">
        Annotations are open to everyone by default. If bad actors follow your channel, whitelist who sees them; you or
        your managers handle requests for access.
      </p>
    </div>
    <div>
      <h2><Icon name="user" size={20} />Smaller channel?</h2>
      <p class="muted">
        A lower rate on different terms may be available. <a href={CONTACT_MAILTO}>Send an email</a> and tell us what you
        stream.
      </p>
    </div>
  </section>

  <!-- The FAQ is kept, not shown, for when there are more questions than these.
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
  -->
</main>

<style>
  /* One gap between the sections below the hero, and one between the items
     inside a section. */
  main {
    --gap-section: 72px;
    --gap-inner: 40px;
    max-width: 1560px;
    margin: 0 auto;
    padding: 0 24px;
  }

  section {
    margin: 0 auto var(--gap-section);
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

  /* A phone: the title stacked. */
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

  .to-pricing {
    display: inline-flex;
    align-items: center;
    gap: 4px;
    margin-top: 14px;
    font-size: 14px;
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

  /* What a VODscription holds, side by side on a wide window. */
  .features {
    max-width: 1040px;
  }

  .parts {
    display: grid;
    grid-template-columns: repeat(auto-fit, minmax(220px, 1fr));
    gap: var(--gap-inner);
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* An item's icon starts its heading's line, and the text under it starts
     at the icon's left edge, so nothing hangs outside the column. */
  h2 > :global(svg) {
    flex: none;
    color: var(--accent);
  }

  /* The features', privacy's and the smaller channel's headings; the price
     row's facts are a step larger, so the two groups don't read as one list. */
  h2 {
    display: flex;
    align-items: center;
    gap: 8px;
    margin: 0 0 4px;
    font-size: 15px;
  }

  .facts h2 {
    font-size: 17px;
  }

  .parts p,
  .facts p,
  .more p {
    margin: 0;
  }

  /* The marks side by side, each name a caption under its mark. */
  .platforms {
    display: flex;
    gap: 20px;
    list-style: none;
    margin: 8px 0 0;
    padding: 0;
  }

  .platforms li {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 6px;
    /* The page's smallest text: a little darker than --muted, to stay easy to read. */
    font-size: 13px;
    color: color-mix(in srgb, var(--text) 80%, transparent);
  }

  /* The price card, and beside it the facts that go with it, their tops
     level; one above the other on a narrow window. */
  .offer {
    /* The header doesn't stick, so a little room above is enough. */
    scroll-margin-top: 24px;
    display: grid;
    grid-template-columns: minmax(0, 400px) minmax(0, 1fr);
    gap: var(--gap-inner);
    align-items: start;
    max-width: 1040px;
  }

  @media (max-width: 760px) {
    .offer {
      grid-template-columns: 1fr;
    }
  }

  .price {
    /* Sign in in the header lands here (+layout.svelte): the card whole, not under the window's edge. */
    scroll-margin-top: 24px;
    display: flex;
    flex-direction: column;
    align-items: center;
    padding: 20px;
    /* The row's focus: an accent rule on top and an accent-tinted edge. */
    border: 1px solid color-mix(in srgb, var(--accent) 35%, var(--border));
    border-top: 3px solid var(--accent);
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

  .facts {
    display: grid;
    gap: var(--gap-inner);
    list-style: none;
    margin: 0;
    padding: 0;
  }

  /* Privacy and the smaller-channel note side by side, the note starting
     where Cast does (the same gap as the features); one above the other on a
     narrow window. */
  .more {
    display: grid;
    grid-template-columns: repeat(2, minmax(0, 1fr));
    gap: var(--gap-inner);
    align-items: start;
    max-width: 1040px;
  }

  @media (max-width: 760px) {
    .more {
      grid-template-columns: 1fr;
    }
  }

  /* A phone: the lead and the price a step smaller, so the price doesn't
     match the title. After the rules they change, to win over them. */
  @media (max-width: 560px) {
    .lead {
      font-size: 17px;
    }

    .amount strong {
      font-size: 28px;
    }
  }
</style>
