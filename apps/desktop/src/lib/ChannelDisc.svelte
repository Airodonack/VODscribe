<script lang="ts">
  // The streamer's picture beside their channel's name, in the VOD's heading
  // and its library row, as the Cast list shows them (CastDisc.svelte): the
  // cast member the channel is (annotated.ts `channelMember`), from the VOD's
  // annotations. There's no room there for an initial, so it's the picture or
  // nothing: with pictures off (Settings › Privacy), nothing is read or asked.
  import { annotated } from "./annotated.svelte";
  import { channelMember, speakerColor } from "./annotated";
  import CastDisc from "./CastDisc.svelte";
  import { displayName } from "./castNames";

  let { vodId, channel, size = 14 }: { vodId: number; channel: string | null; size?: number } = $props();

  const on = $derived(!!channel && annotated.avatarBase() !== null);
  $effect(() => {
    if (on) annotated.load(vodId);
  });
  const cast = $derived(on ? (annotated.of(vodId)?.cast ?? []) : []);
  const member = $derived(channelMember(cast, channel));
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
  .chip {
    position: relative;
    display: inline-flex;
    vertical-align: -2px;
  }

  /* A gap before the name only once there's a picture in it. */
  .chip:has(:global(.avatar.pictured)) {
    margin-right: 4px;
  }
</style>
