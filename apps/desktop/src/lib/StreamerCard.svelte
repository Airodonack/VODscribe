<script lang="ts">
  // A person's streamer card, at the pointer where their name or picture was
  // clicked (streamers.svelte.ts `showCard`): their picture and name, other
  // names they go by, their own stream at that moment when it ran alongside
  // (castLink.svelte.ts), their channels and other pages, and what they play
  // in each context this VOD is part of (NoPixel's characters, with their
  // wiki pages). It's the one place their links are. What it shows comes from
  // their card on the file host (streamers.ts) where there is one, else from
  // the VOD's cast; whatever is missing isn't drawn, heading and all, so
  // offline, or with no card, it is their name, picture and channel as the
  // VOD knows them. One for the page (App.svelte); a Popover, so a click
  // outside it or Escape closes it.
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { annotated } from "./annotated.svelte";
  import { channelLabel, channelUrl } from "./accounts";
  import { castLink } from "./castLink.svelte";
  import CastDisc from "./CastDisc.svelte";
  import Icon from "./Icon.svelte";
  import { formatTime } from "./platform";
  import Popover from "./Popover.svelte";
  import { cardView, type Streamer } from "./streamers";
  import { cards, openCard } from "./streamers.svelte";

  const of = $derived(openCard.of);
  /** Their card from the file host: undefined while it's asked for, null when there's none to be had. */
  let streamer = $state.raw<Streamer | null | undefined>(undefined);
  $effect(() => {
    const asked = of;
    streamer = undefined;
    if (!asked) return;
    let current = true;
    void cards.of(asked.member, asked.member.account ?? asked.account).then((got) => {
      if (current) streamer = got;
    });
    return () => {
      current = false;
    };
  });
  const contexts = $derived(of?.vod ? (annotated.of(of.vod.id)?.contexts ?? []) : []);
  const view = $derived(of ? cardView(of.member, of.name, streamer ?? null, contexts) : null);
  const to = $derived(of ? castLink(of.member, of.vod, of.moment) : null);
  /** Their own stream at the moment, when it ran alongside: first, as the name's link was. */
  const pov = $derived(to?.pov ? to : null);
  const face = $derived(
    view ? { name: view.name, aliases: [], role: "streamer", account: view.avatar ?? undefined } : undefined,
  );

  function follow(e: MouseEvent, go: () => void) {
    e.preventDefault();
    openCard.open = false;
    go();
  }
</script>

<Popover
  bind:open={openCard.open}
  anchor={openCard.anchor}
  at={openCard.at}
  label={view ? `${view.name}'s card` : "Streamer card"}
  width={260}
>
  {#if view && of}
    <div class="card" data-card={streamer?.id ?? ""} data-settled={streamer !== undefined}>
      <header>
        <CastDisc name={view.name} color={of.color} member={face} size={40} link={false} />
        <div class="who">
          <strong class="name">{view.name}</strong>
          {#if view.aliases.length}<span class="aliases muted">also {view.aliases.join(", ")}</span>{/if}
        </div>
      </header>
      {#if pov || view.channels.length || view.socials.length}
        <ul class="links">
          {#if pov}
            <li>
              <a class="pov" href={pov.href} title={pov.title} onclick={(e) => follow(e, pov.follow)}
                ><Icon name="play" size={12} />Their stream at {formatTime(pov.pov!.offset)}{#if pov.outside}<span
                    class="out"><Icon name="external" size={10} /></span
                  >{/if}</a
              >
            </li>
          {/if}
          {#each view.channels as channel (channel.platform + channel.login)}
            {@const href = channelUrl(channel)}
            <li>
              <a class="channel" {href} title={href} onclick={(e) => follow(e, () => void openUrl(href))}
                >{channelLabel(channel)}<span class="out"><Icon name="external" size={10} /></span></a
              >
            </li>
          {/each}
          {#each view.socials as social (social.href)}
            <li>
              <a
                class="social"
                href={social.href}
                title={social.href}
                onclick={(e) => follow(e, () => void openUrl(social.href))}
                >{social.label}<span class="out"><Icon name="external" size={10} /></span></a
              >
            </li>
          {/each}
        </ul>
      {/if}
      {#each view.sections as section (section.id)}
        <section class="context" data-context={section.id}>
          <h3>{section.title}</h3>
          <ul>
            {#each section.characters as character (character.name)}
              <li>
                {#if character.wiki}
                  {@const wiki = character.wiki}
                  <a href={wiki} title={wiki} onclick={(e) => follow(e, () => void openUrl(wiki))}
                    >{character.name}<span class="out"><Icon name="external" size={10} /></span></a
                  >
                {:else}
                  {character.name}
                {/if}
              </li>
            {/each}
          </ul>
        </section>
      {/each}
    </div>
  {/if}
</Popover>

<style>
  .card {
    display: grid;
    gap: 10px;
    font-size: 13px;
  }

  header {
    display: flex;
    align-items: center;
    gap: 10px;
    min-width: 0;
  }

  .who {
    display: flex;
    flex-direction: column;
    min-width: 0;
  }

  .name {
    font-size: 15px;
    overflow-wrap: anywhere;
  }

  .aliases {
    font-size: 12px;
    overflow-wrap: anywhere;
  }

  /* Each group under a rule: no rule, and no room, for one that isn't there. */
  .links,
  .context {
    border-top: 1px solid var(--border);
    padding-top: 8px;
  }

  ul {
    list-style: none;
    margin: 0;
    padding: 0;
    display: grid;
    gap: 4px;
  }

  a {
    color: var(--accent);
    text-decoration: none;
    overflow-wrap: anywhere;
  }

  a:hover {
    text-decoration: underline;
  }

  .pov {
    display: inline-flex;
    align-items: center;
    gap: 5px;
  }

  /* The sign that the link leaves the app. */
  .out {
    margin-left: 3px;
    opacity: 0.7;
  }

  h3 {
    margin: 0 0 4px;
    font-size: 11px;
    font-weight: 600;
    letter-spacing: 0.04em;
    text-transform: uppercase;
    color: var(--muted);
  }
</style>
