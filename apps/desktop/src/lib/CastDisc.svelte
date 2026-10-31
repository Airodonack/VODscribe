<script lang="ts">
  // A cast member's disc over their lines in the transcript: their initial in
  // their cast colour, and over it their picture when their account has one
  // (accounts.ts). The picture shows only once it has loaded, so the disc is
  // never empty: before, or when there's none, the initial is what shows. It
  // is shown whole as the site has it, only clipped round as chats draw one,
  // and the disc links where their name beside it does (`to`, castLink.svelte.ts:
  // their channel, or their stream at the moment), unless it sits inside
  // something that is a control of its own (`link`). The Cast popover and the
  // chapters and highlights pane draw it smaller (`size`). A text-to-speech
  // voice is no one: a voice's sign in place of an initial, and no picture;
  // nor is a speaker no one names (`anonymous`): a person's outline. Where
  // there's no room to spare (the VOD's heading, its library row:
  // `pictureOnly`), it is the picture or nothing: no initial while it
  // loads, and nothing at all without one (pictures off, none there).
  import { avatarUrl } from "./accounts";
  import { annotated } from "./annotated.svelte";
  import { isVoice } from "./annotated";
  import type { CastMember } from "./api";
  import type { CastLink } from "./castLink.svelte";
  import Icon from "./Icon.svelte";
  import { initial } from "./transcriptGroups";

  let {
    name,
    color,
    member,
    size = 22,
    link = true,
    to = null,
    anonymous = false,
    pictureOnly = false,
  }: {
    /** Who it's for, as the head names them: the initial is its first letter, the character's where a streamer plays them. */
    name: string;
    /** Their cast colour; null for someone not in the cast. */
    color: string | null;
    member?: CastMember;
    /** Its width and height, in pixels; the initial is drawn to match. */
    size?: number;
    /** Whether it's a link; false inside a button, which can't hold one. */
    link?: boolean;
    /** Where it links, as their name beside it does; null for nowhere. */
    to?: CastLink | null;
    /** For lines no one is named for: an outline of a person, no one's initial. */
    anonymous?: boolean;
    /** Only the picture, once loaded; nothing until then, or without one. */
    pictureOnly?: boolean;
  } = $props();

  const voice = $derived(isVoice(member));
  const account = $derived(voice ? undefined : member?.account);
  const base = $derived(account ? annotated.avatarBase() : null);
  // TODO: a YouTube picture must show YouTube's own icon beside it, linking to
  // the channel (YouTube's developer policies). Until that icon is in the app,
  // as YouTube publishes it, a YouTube account gets its link and no picture.
  const picture = $derived(account && base && account.platform !== "youtube" ? avatarUrl(base, account) : null);
  let loaded = $state(false);
  let failed = $state(false);
  $effect(() => {
    void picture;
    loaded = false;
    failed = false;
  });
</script>

{#snippet face()}
  {#if anonymous}
    <Icon name="user" size={Math.round(size * 0.6)} />
  {:else if voice}
    <Icon name="audio-lines" size={Math.round(size * 0.6)} />
  {:else}
    {initial(name)}
  {/if}
  {#if picture}
    <img
      src={picture}
      alt=""
      referrerpolicy="no-referrer"
      loading={pictureOnly ? "eager" : "lazy"}
      class:loaded
      onload={() => (loaded = true)}
      onerror={() => ((loaded = false), (failed = true))}
    />
  {/if}
{/snippet}

{#if pictureOnly && (!picture || failed)}
  <!-- No picture to show: nothing, not an initial. -->
{:else if to && link}
  <!-- The name beside it is the same link, for the keyboard and screen readers. -->
  <a
    class="avatar"
    class:unknown={!color}
    class:pictured={loaded}
    class:waiting={pictureOnly && !loaded}
    style:background={color}
    style:--size="{size}px"
    href={to.href}
    title={to.title}
    tabindex="-1"
    aria-hidden="true"
    onclick={(e) => (e.preventDefault(), to.follow())}>{@render face()}</a
  >
{:else}
  <span
    class="avatar"
    class:unknown={!color}
    class:pictured={loaded}
    class:waiting={pictureOnly && !loaded}
    style:background={color}
    style:--size="{size}px"
    aria-hidden="true">{@render face()}</span
  >
{/if}

<style>
  .avatar {
    position: relative;
    display: grid;
    place-items: center;
    flex: none;
    width: var(--size, 22px);
    height: var(--size, 22px);
    border-radius: 50%;
    overflow: hidden;
    color: #fff;
    font-size: calc(var(--size, 22px) / 2);
    font-weight: 700;
    line-height: 1;
    text-decoration: none;
    user-select: none;
  }

  .avatar.unknown {
    background: var(--muted);
  }

  .avatar.pictured {
    color: transparent;
  }

  /* Only a picture, and that not loaded yet: kept out of the way, still loading. */
  .avatar.waiting {
    position: absolute;
    width: 1px;
    height: 1px;
    opacity: 0;
  }

  img {
    position: absolute;
    inset: 0;
    width: 100%;
    height: 100%;
    object-fit: contain;
    opacity: 0;
  }

  img.loaded {
    opacity: 1;
  }
</style>
