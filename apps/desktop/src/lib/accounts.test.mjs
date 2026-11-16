// Where a cast member's account leads (accounts.ts): node --test apps/desktop/src/lib/accounts.test.mjs
// Sources: accounts.ts's doc comments; CHANGELOG.md 0.7.0 ("Characters link to their streamer."), 0.7.1 ("The two
// privacy settings are renamed.": "Enable streamer pictures from img.vodscribe.com").
import assert from "node:assert/strict";
import { test } from "node:test";
import { AVATAR_BASE, avatarUrl, channelLabel, channelUrl } from "./accounts.ts";

/** accounts.ts `channelUrl` ("The channel of `account`, on its site"). */
test("a Twitch or Kick account leads to its channel on its site", () => {
  assert.equal(channelUrl({ platform: "twitch", login: "marlowe" }), "https://www.twitch.tv/marlowe");
  assert.equal(channelUrl({ platform: "kick", login: "ada_plays" }), "https://kick.com/ada_plays");
});

/** accounts.ts `channelUrl` ("A handle (\"@name\") has a page of its own; anything else is a channel id"). */
test("a YouTube handle has a page of its own, and anything else is a channel id", () => {
  assert.equal(channelUrl({ platform: "youtube", login: "@ada.makes" }), "https://www.youtube.com/@ada.makes");
  assert.equal(channelUrl({ platform: "youtube", login: "UCabc-123" }), "https://www.youtube.com/channel/UCabc-123");
});

/** accounts.ts `part` ("A login as one part of a path: escaped, but a YouTube handle's \"@\" kept"). */
test("a login is one part of the address, escaped, a handle's @ kept", () => {
  assert.equal(channelUrl({ platform: "kick", login: "a/../b" }), "https://kick.com/a%2F..%2Fb");
  assert.equal(
    channelUrl({ platform: "youtube", login: "@마를로" }),
    "https://www.youtube.com/@%EB%A7%88%EB%A5%BC%EB%A1%9C",
  );
  assert.equal(channelUrl({ platform: "youtube", login: "UC a" }), "https://www.youtube.com/channel/UC%20a");
});

/** accounts.ts `channelLabel` ("The channel's address as it's read out: \"twitch.tv/marlowe\""). */
test("a channel's address is read out without its scheme or www", () => {
  assert.equal(channelLabel({ platform: "twitch", login: "marlowe" }), "twitch.tv/marlowe");
  assert.equal(channelLabel({ platform: "kick", login: "ada" }), "kick.com/ada");
  assert.equal(channelLabel({ platform: "youtube", login: "@ada" }), "youtube.com/@ada");
});

/** accounts.ts `AVATAR_BASE` ("The image server's address"); CHANGELOG 0.7.1 "Enable streamer pictures from img.vodscribe.com". */
test("the pictures are asked of img.vodscribe.com", () => {
  assert.equal(AVATAR_BASE, "https://img.vodscribe.com");
});

/** accounts.ts `avatarUrl` ("Where `account`'s picture is asked for, on image server `base`"). */
test("an account's picture is asked for by its site and login on the image server", () => {
  assert.equal(
    avatarUrl(AVATAR_BASE, { platform: "twitch", login: "marlowe" }),
    "https://img.vodscribe.com/avatar/twitch/marlowe",
  );
  assert.equal(avatarUrl(AVATAR_BASE, { platform: "kick", login: "ada" }), "https://img.vodscribe.com/avatar/kick/ada");
  assert.equal(
    avatarUrl("http://127.0.0.1:9", { platform: "youtube", login: "@ada" }),
    "http://127.0.0.1:9/avatar/youtube/@ada",
  );
});

/** accounts.ts `avatarUrl` (`base.replace(/\/+$/, "")`: a base's trailing slashes don't double). */
test("a server address's trailing slashes don't double in a picture's address", () => {
  assert.equal(
    avatarUrl("http://127.0.0.1:9///", { platform: "twitch", login: "ada" }),
    "http://127.0.0.1:9/avatar/twitch/ada",
  );
});

/** accounts.ts `part` (escaped all the same, should a login that can't be one reach it). */
test("a login that can't be one is escaped in a picture's address", () => {
  assert.equal(avatarUrl("http://h", { platform: "kick", login: "a/../b" }), "http://h/avatar/kick/a%2F..%2Fb");
});
