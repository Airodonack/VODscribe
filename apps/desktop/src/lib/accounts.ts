// Where a cast member's account (annotations.ts `Account`) leads: their
// channel, and their picture. The picture is asked of VODscribe's image
// server at display time, which answers with a redirect to the site's own
// picture or 404; the web view loads and caches it as it does any image, and
// nothing of it is kept by the app. The channel link beside it is the way to
// where the picture comes from.

import type { Account } from "./annotations";

/** The image server's address. The app can be pointed at another (crates/core annotations.rs `AVATAR_ENV`). */
export const AVATAR_BASE = "https://img.vodscribe.com";

/** A login as one part of a path: escaped, but a YouTube handle's "@" kept. */
const part = (login: string) =>
  login.startsWith("@") ? `@${encodeURIComponent(login.slice(1))}` : encodeURIComponent(login);

/** The channel of `account`, on its site. */
export function channelUrl(account: Account): string {
  switch (account.platform) {
    case "twitch":
      return `https://www.twitch.tv/${part(account.login)}`;
    case "kick":
      return `https://kick.com/${part(account.login)}`;
    case "youtube":
      // A handle ("@name") has a page of its own; anything else is a channel id.
      return account.login.startsWith("@")
        ? `https://www.youtube.com/${part(account.login)}`
        : `https://www.youtube.com/channel/${part(account.login)}`;
  }
}

/** The channel's address as it's read out: "twitch.tv/marlowe". */
export const channelLabel = (account: Account): string => channelUrl(account).replace(/^https:\/\/(www\.)?/, "");

/** Where `account`'s picture is asked for, on image server `base`. */
export const avatarUrl = (base: string, account: Account): string =>
  `${base.replace(/\/+$/, "")}/avatar/${account.platform}/${part(account.login)}`;
