<script lang="ts">
  // The streamer's picture beside their channel's name, in the VOD's heading
  // and its library row, as the Cast list shows them (CastDisc.svelte), for
  // a VOD with a VODscription: the cast member the channel is (annotated.ts
  // `channelMember`), or when the cast gives none an account, the channel
  // itself, by its own login (annotated.ts `channelAccount`). There's no
  // room there for an initial, so it's the picture or nothing: with pictures
  // off (Settings › Privacy), nothing is read or asked.
  import { annotated } from "./annotated.svelte";
  import { channelAccount, channelMember, speakerColor } from "./annotated";
  import type { CastMember, Vod } from "./api";
  import CastDisc from "./CastDisc.svelte";
  import { displayName } from "./castNames";

  let { vod, size = 14 }: { vod: Vod; size?: number } = $props();

  const on = $derived(!!vod.channel && annotated.avatarBase() !== null);
  $effect(() => {
    if (on) annotated.load(vod.id);
  });
  const annotations = $derived(on ? annotated.of(vod.id) : null);
  const cast = $derived(annotations?.cast ?? []);
  const member = $derived.by((): CastMember | undefined => {
    if (!annotations || !vod.channel) return undefined;
    const named = channelMember(cast, vod.channel);
    if (named) return named;
    const account = channelAccount(vod);
    return account && { name: vod.channel, aliases: [], role: "streamer", account };
  });
</script>

{#if member}
  <span class="chip"
    ><CastDisc
      name={displayName(member)}
      color={speakerColor(cast, member.name)}
      {member}
      {size}
      link={false}
      pictureOnly
    /></span
  >
{/if}

<style>
  /* On the middle of the name's line. An empty character in the name's font
     comes first and gives the box the name's baseline; the picture is centred
     on that character's line, which is the name's. (The hidden initial's
     baseline put it low; the x-height's middle still left it 1 px or so low.) */
  .chip {
    position: relative;
    display: inline-flex;
    align-items: center;
  }

  .chip::before {
    content: "\200b";
  }

  /* A gap before the name only once there's a picture in it. */
  .chip:has(:global(.avatar.pictured)) {
    margin-right: 4px;
  }
</style>
