# Changelog

What changed in each release, for people who use VODscribe. The entry for the version being released is also the release notes in `latest.json` (`scripts/release.py` reads it), so write it for them, not for developers. The newest release is first; a version still being worked on is marked "(unreleased)".

## 0.4.0 (unreleased)

- Edits: put clips and stretches of your VODs together into one video. An edit sits in the library's list beside your VODs; drop a clip, a stretch dragged from a timeline, or the Clip button onto it, or use a clip's "Add to an edit". In an edit, drag pieces to reorder them and their edges to make them shorter or longer, split at the playhead (B), click lines of the transcript to strike them out, and render it to one video (E). What's put in an edit is its own copy.
- Clips: Clip (E) makes the lit-up stretch a clip (with no cuts, the whole video), a pill in the clip bar under the player: play it, rename it, save it, add it to an edit, or drag it into your video editor. Drag the Clip button, or Alt-drag (Option on a Mac) a stretch of the timeline, to take it straight out.
- Clips and edits can be made as MOV (DNxHR video, PCM sound), the format video editors take best: Settings → Clip format. DaVinci Resolve on Linux needs it, and says so once.
- Chapters and highlights on the timeline: N starts a chapter and M marks a highlight (M, as in video editors), each with a title to write. Chapters head the transcript and copy as YouTube chapters. Pressed by mistake, Escape takes it back. Muting is now the speaker button's alone.
- Undo and redo (Ctrl+Z and Ctrl+Shift+Z, or ⌘Z and ⇧⌘Z) for edits, marks and the clip bar.
- Back and forward (Alt+← and Alt+→, or ⌘[ and ⌘]) between the VODs and edits you've opened, each as you left it.
- VODscribe's own player plays your files, downloaded videos and edits, on Windows, macOS and Linux: smooth, frame-exact (, and . step a frame), with captions over the picture. Videos without sound can be added.
- YouTube's and Twitch's players keep the playhead in step: at other speeds, after seeks, and while Twitch reloads a long VOD.
- A copy of your library is kept before each upgrade of its format (for 30 days), and if the library won't open, VODscribe says so and offers a way out.
- Files you made and no longer use don't pile up in Videos/VODscribe: a clip's earlier file goes when it's resized, and an edit's when it's renamed and rendered again, unless you dragged that file into another app.
- Fixes: Twitch's playhead jumped back while playing or after a seek. Removing a VOD, or deleting its downloaded video, didn't stop what was still using it. Adding several files at once could fail with "database is locked". Installing a new version over an old one on Windows could offer to delete your library. Quitting left downloads and transcription running. Error messages showed your home folder's full path. A file ffmpeg can't read passed as silent instead of failing.

## 0.3.0 — 2026-10-15

- VODs from other sites: paste a link from any site the video downloader knows (Dailymotion, SoundCloud, Bandcamp and well over a thousand more), not only Kick, Twitch and YouTube. Their video, or audio, is kept on your computer to play in VODscribe, in a format your computer plays, up to 720p. Settings → Storage chooses 480p to 1080p and shows the space they take; the crossed-out camera in its row deletes the video and keeps the transcript. Sites that need you to sign in aren't supported.
- Twitch clips play, and show the chat from the broadcast they were cut from, while Twitch still has it.
- Kick and Twitch chats download 3 to 5 times faster.
- YouTube chat shows how far it has got and appears as it downloads, and removing the VOD stops it.
- Transcripts fill in steadily while they're made, and pausing or quitting loses at most half a minute of work.
- Remove, Cancel, Transcribe again, Free up space and Remove GPU add-on ask first again. They had stopped asking, and went ahead on the first click.
- Audio alone (a podcast, a song, an audio file) plays in a slim bar instead of an empty video frame.
- Links and files being added show in the library right away, with a spinner, and ✕ cancels one.
- 7TV, BetterTTV and FrankerFaceZ emotes load faster, and a provider that can't be reached isn't waited on again for a few minutes.
- VODscribe won't follow links to this computer or your local network.
- Needs macOS 12 or later, as it already did in practice; older Macs are told so at install.
- Fixes: the first second or two of FLV files was left out of the transcript. Chat or a thumbnail still arriving for a removed VOD could land on the next one added. A YouTube chat could fail when its transcription finished first. Stop, Pause and Remove could miss a VOD that had just started transcribing. Switching VODs could briefly show the previous one's transcript, with its chat timeline at the wrong scale. Two quick changes in Settings could be saved out of order. Timeline layers that failed to load could be overwritten. On Linux, a link dropped onto a player opened in the player instead of being added, and the drop overlay could stay on screen. Stopping or removing a download could leave partial files behind. A rare crash on an unusual upload date.

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
