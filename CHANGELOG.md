# Changelog

What changed in each release, for people who use VODscribe. The entry for the version being released is also the release notes in `latest.json` (`scripts/release.py` reads it), so write it for them, not for developers. The newest release is first; a version still being worked on is marked "(unreleased)".

## 0.2.1 — 2026-10-14

- Every version's changes are on vodscribe.com/releases. The version number in the sidebar opens this version's, and the update notice has a "What's new" link to the new one's.
- Settings has a new Appearance section. Its theme chooses System (as before), Light or Dark; on Windows and macOS the title bar follows.
- Chat can show 7TV, BetterTTV and FrankerFaceZ emotes for Kick and Twitch VODs (Settings → Appearance): the channel's own and the global ones, as most viewers of big channels see them, with "zero-width" emotes (a hat, a spin) drawn over the emote before them. Each is off until you turn it on, since it means contacting that service; drag them to choose which wins when two have an emote of the same name. Chat already in your library shows them too, and the Timeline panel's pills use their images.
- The timeline strip beside the transcript appears as soon as chat is in, even before the transcript has its first line, and scrolls through the part still waiting to be transcribed.
- Clicking or dragging on the timeline strip centers that moment in the view, so a moment you aim at is in plain sight; before, it landed just under the header, and its event marks sat slightly early. Dragging the box moves it smoothly with the pointer, also across stretches without speech, where it used to jump and stall, and with chat shown such stretches scroll by minute.
- Settings and the search's From filter open above everything, like the Timeline panel, so a playing video or a short window can't cover or cut them off.
- Twitch bits cheered with newer cheermotes, such as PrideCheer, count as bits: each chat fetch asks Twitch for its current cheermotes rather than relying on a built-in list. Chats fetched before keep how they were counted.
- Faster with big chats: searching one VOD of a large library, scrolling and dragging the timeline strip through busy chat, fetching a chat again (which used to freeze the app for seconds), and removing a VOD. A one-letter search now finds that word, rather than every word starting with the letter.
- A link pasted while dropped links or files are still being added waits its turn, and the errors of both show together, instead of the Add button coming back early.

## 0.2.0 — 2026-10-13

- Chat: for Kick, Twitch and YouTube livestreams, the chat replay sits beside the transcript, each line with the messages sent while it was said, emotes and all. Hide it with the Chat button. It's fetched in the background, shows as it arrives, and carries on where it stopped if interrupted.
- Search covers chat too. The From button by the search box chooses audio, chat or both, and this VOD or all of them; choosing All VODs keeps it there as you switch VODs.
- A timeline beside the transcript shows how busy chat was over the whole VOD. Click or drag on it to go there.
- Timeline layers mark the moments you care about: laughter, hype and "clip it" to start, or any words and emotes, for every channel or one. Up to three show at once, and their messages are highlighted in chat.
- Subs, gifts and tips (bits, KICKs, Super Chats and memberships) are marked in chat, and can be shown on the timeline, stronger where more money changed hands.
- Long VODs with busy chats open, scroll and jump around quickly.
- Removing or re-transcribing a VOD shows that it's under way, and the rest of the app keeps working meanwhile.
- No white flash when the app opens.
- Fixes: the GPU add-on's progress shows when Settings is reopened mid-download.

## 0.1.3 — 2026-10-13

- Setup and the GPU add-on downloads have a Cancel button. Tools and model files that finished stay; the GPU add-on's own pack starts over.
- The GPU add-on is smaller: it now contains only ONNX Runtime and NVIDIA's libraries, since the speech-recognition library ships with the app. It downloads again once after updating.
- Settings has a Help section with where to report problems, and the website answers the same question.
- Under the hood: a weekly check that every file setup downloads is still available where it's expected.
- A YouTube download that the site refused is tried again from a fresh lookup before it's reported, since YouTube sometimes refuses the first.
- VODscribe's code is public, under the Functional Source License (source-available; Apache 2.0 two years after each release).
- Downloaded installers carry the version in their file name, like VODscribe-0.1.3-windows-x64-setup.exe.
- A copy that wasn't installed from a package (built from source, say) says so when asked to check for updates, instead of an error about platforms.

## 0.1.2 — 2026-10-12

- Downloads with several files show which one is under way, like "(3 of 5)".
- Settings no longer scrolls sideways.
- Download sizes are exact.
- Under the hood: SvelteKit 3 and more tests.

## 0.1.1 — 2026-10-12

- Fixes YouTube and Twitch playback in the installed app.
- Linux downloads are .deb and .rpm only.
- Opening VODscribe twice brings up the open window.
- Downloads work on Windows for users with non-ASCII names.
- Several smaller fixes.

## 0.1.0 — 2026-10-12

First release.
