<script lang="ts">
  // A cast member's disc over their lines in the transcript: their initial in
  // their cast colour, and over it their picture when their account has one
  // (accounts.ts). The picture shows only once it has loaded, so the disc is
  // never empty: before, or when there's none, the initial is what shows. It
  // is shown whole as the site has it, only clipped round as chats draw one,
  // and the disc links to their channel as their name beside it does, unless
  // it sits inside something that is a control of its own (`link`). The
  // Cast popover and the chapters and highlights pane draw it smaller (`size`).
  import { openUrl } from "@tauri-apps/plugin-opener";
  import { avatarUrl, channelLabel, channelUrl } from "./accounts";
  import { annotated } from "./annotated.svelte";
  import type { CastMember } from "./api";
  import { initial } from "./transcriptGroups";

  let {
    name,
    color,
    member,
    size = 22,
    link = true,
  }: {
    /** Who it's for, as the head names them: the initial is its first letter, the character's where a streamer plays them. */
    name: string;
    /** Their cast colour; null for someone not in the cast. */
    color: string | null;
    member?: CastMember;
    /** Its width and height, in pixels; the initial is drawn to match. */
    size?: number;
    /** Whether it links to their channel; false inside a button, which can't hold a link. */
    link?: boolean;
  } = $props();

  const account = $derived(member?.account);
  const base = $derived(account ? annotated.avatarBase() : null);
  // TODO: a YouTube picture must show YouTube's own icon beside it, linking to
  // the channel (YouTube's developer policies). Until that icon is in the app,
  // as YouTube publishes it, a YouTube account gets its link and no picture.
  const picture = $derived(account && base && account.platform !== "youtube" ? avatarUrl(base, account) : null);
  let loaded = $state(false);
  $effect(() => {
    void picture;
    loaded = false;
  });
</script>

{#snippet face()}
  {initial(name)}
  {#if picture}
    <img
      src={picture}
      alt=""
      referrerpolicy="no-referrer"
      loading="lazy"
      class:loaded
      onload={() => (loaded = true)}
      onerror={() => (loaded = false)}
    />
  {/if}
{/snippet}

{#if account && link}
  {@const url = channelUrl(account)}
  <!-- The name beside it is the same link, for the keyboard and screen readers. -->
  <a
    class="avatar"
    class:unknown={!color}
    class:pictured={loaded}
    style:background={color}
    style:--size="{size}px"
    href={url}
    title={channelLabel(account)}
    tabindex="-1"
    aria-hidden="true"
    onclick={(e) => (e.preventDefault(), openUrl(url))}>{@render face()}</a
  >
{:else}
  <span
    class="avatar"
    class:unknown={!color}
    class:pictured={loaded}
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
