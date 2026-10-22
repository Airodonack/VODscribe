# Changelog

What changed in each release, for people who use VODscribe. The entry for the version being released is also the release notes in `latest.json` (`scripts/release.py` reads it), so write it for them, not for developers. The newest release is first; a version still being worked on is marked "(unreleased)".

## 0.5.1 (unreleased)

- After a click on the timeline, the time by the play button no longer flicks back to where the video was while the picture catches up: it shows the new time at once and stays on it.
- Moving around a video while it plays (a click on the timeline, the arrow keys) no longer flashes the Play button for a moment: it goes on saying Pause.
- The time and frame count under a Twitch video no longer jump back and forth about once a second while it plays.
- I and O set where the selection starts and ends, as in video editors: I at the playhead is its start, O its end. With nothing selected, I selects from the playhead to the next cut (or the end), and O back to the cut before (or the start). Put past the other end, the two swap, so the selection runs between them. B, C and (in an edit) Delete then act on it as on lines selected in the transcript.

## 0.5.0 — 2026-10-22

- VODs and edits open in one player: the same picture, timeline, controls, keys, transcript and clip bar, each doing what it can (an edit's pieces move and resize; a VOD stays whole). Edits gain what VODs had, below.
- New keys: C makes a clip (it was E; E now renders an edit), Shift+C turns captions on or off (it was C), and Shift+M mutes. Letters on their own act on the video; with Shift they're the player's own.
- Video fetched from Kick, Twitch and YouTube for clips and edits is kept only while an edit uses it: the rest is deleted about an hour after its last use (it was kept up to 30 days), and fetched again if a clip needs it later. Settings → Storage → Fetched video shows how much it takes, and deletes what edits don't use at once. Repair VODscribe has moved to Help, beside Reset settings.
- An edit's transcript shows under its player as a VOD's does, with only what the edit plays, and works the same: click a line to play from it, drag across lines or Shift+click to select them. In an edit, Delete then takes the selected lines' stretch out, as any stretch taken out: the piece splits in two there, each part its own band (Ctrl+Z brings it back). A click no longer strikes a line. Shift+click selects in a VOD's transcript too.
- Edits have captions: what's said in each piece, over the picture as it plays, with the CC button and Shift+C as for a VOD. An edit's player also keeps the volume you set, as a VOD's does.
- Edits have chapters and highlights of their own: N and M in an edit, as in a VOD, shown on its timeline and over its transcript. Each stays on its moment of the video, so it moves with its piece; a chapter whose start is cut away starts where its piece plays again.
- In an edit, B puts a cut, as in a VOD: a mark you can move, take away (B on it again, or Delete) or drag off. Delete with nothing selected takes out the stretch between cuts at the playhead: the piece splits there, and those cuts become a real split, drawn as a plain line; with no cuts in the piece, it takes out the piece. With lines selected, B cuts at both ends of them.
- An edit has a clip bar of its own: C makes a clip of the lines selected, or of the stretch at the playhead (between cuts, or its piece), with the edit's chapters and highlights in it. Its clips drag out into a video editor or a folder, go into other edits, open in the editor with a double-click, and leave the bar when their file is deleted outside VODscribe (Ctrl+Z brings them back), as a VOD's do.
- An edit's transcript has chat beside it: each piece's VOD's chat where that piece plays, and the minimap shows how busy it is over the edit. Its header has the line count, the Chat button to hide or show it, and the Timeline button, as a VOD's does: subs, gifts and tips, and the Timeline layers of each piece's VOD, show where they play, and the emotes its chat writes as words show as images.
- A line of the transcript split between two pieces of an edit shows, under each, only the words said there, and the edit's captions likewise.
- A rendered edit has its chapters, as chapters of the video file that players and editors show. It shows as changed since it was rendered only when its video would be different: a highlight or a cut added doesn't count, and a clip opened in the editor isn't made again for one.
- A VOD that's only sound goes full screen too (F, or the button): black, with its captions large in the middle.
- A clip added to an edit arrives whole, as one band on the timeline that moves and goes as one. Double-click it to open it, and its pieces show one by one, to move among themselves, trim, split or remove; double-click one again, or press Escape, to close it. An edit added to another arrives the same way.
- Double-click a clip's name in the clip bar to open it in the editor: trim it, split it or take lines out there, and its file is made again to match. Enter or F2 on its name still renames it (double-click used to).
- A clip, a stretch put in an edit, and a clip added to an edit bring the chapters and highlights in them along, on the same moments.
- Removing a VOD that an edit uses leaves the edit's pieces of it where they are, and asks whether its video goes too. Removed from the library, its transcript and video stay on this computer until no edit uses them, then are erased when VODscribe quits (or else the next time it starts). Removed with its video, only the stretches the edits use are kept, as videos of their own, and the rest is erased at once; if those stretches can't be kept (offline, say), it asks whether to try again, keep the video, or delete it anyway, leaving gaps in the edits.
- In an edit, a piece whose video file you deleted outside VODscribe shows striped in the timeline and plays and renders as black and silence, rather than stopping the edit from rendering.
- The time under the player, as a clock or by frame, is the frame on screen: after a jump it changes as the picture does, and the frame number is the one you see.
- Fixes:
  - A VOD opened again after an edit started at its beginning, not where you'd left it.
  - On macOS, clips of VODs from Kick, Twitch and YouTube, and edits with pieces of them, couldn't get their video ("Couldn't get this video"): the video reader didn't trust the sites' certificates.
  - Undoing a cut, chapter or highlight made before you deleted a clip's file outside VODscribe brought the clip back with it. The clip going is now its own step: undo brings it back (made again), and the next undo takes back the mark.
  - A stretch pulled out of the window with Clip and let go the moment it left could stay as a clip, with its file, when the file had already been made; it goes now.

## 0.4.1 — 2026-10-19

- Settings → Help has Reset settings: every setting in Settings back as it started, with your library, transcripts, clips, edits and videos kept.
- On macOS, if it asks to find devices on your local network, it now says why: a site's video player looking for a TV to play on. VODscribe works the same if you don't allow it.
- More is done from the keyboard: Enter renames the selected chapter or highlight, and Enter or F2 renames a clip; in an edit, Ctrl+← and Ctrl+→ (⌘← and ⌘→ on a Mac) move the piece at the playhead earlier or later.
- The keyboard button lists every shortcut, Escape, undo and redo among them, and an edit has its own list.
- In an edit, pointing at Remove lights up, in red, the piece it would take out.
- A clip whose file you delete outside VODscribe (in a file manager, say) leaves the clip bar too.
- Fixes:
  - Videos at 23.976, 29.97 or 59.94 frames a second showed one frame twice every few minutes, and stepping frame by frame drifted.
  - A chapter or highlight taken back with Escape left nothing to redo.
  - Removing a VOD while its clips waited to be made could still download them, leaving their files behind.
  - On Windows, a clip deleted while an edit was rendering from it kept its file.
  - Settings and other panels opened over a video playing in VODscribe's own player were hidden behind the picture, as was the timeline's time under the pointer.
  - The playhead could flick back to a moment already passed.
  - Sound could click while playing in VODscribe's own player.
  - Captions blurred at 125% or 150% display scale, and a very long word ran out of them.
  - The GPU add-on's progress bar could stay up after it had installed.
  - On Linux, a sound-only file played from its last second could stay "playing" for good instead of ending.
  - On Linux, seeking in a Twitch VOD could freeze the whole desktop (GNOME) while its sound played on.
  - Dragging a clip out of the window paused at its edge before the drag began, and letting go then dropped nothing.
  - A clip dragged out of the window and back in couldn't be dropped on an edit or the clip bar.

## 0.4.0 — 2026-10-18

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
- Fixes:
  - Twitch's playhead jumped back while playing or after a seek.
  - Removing a VOD, or deleting its downloaded video, didn't stop what was still using it.
  - Adding several files at once could fail with "database is locked".
  - Installing a new version over an old one on Windows could offer to delete your library.
  - Quitting left downloads and transcription running.
  - Error messages showed your home folder's full path.
  - A file ffmpeg can't read passed as silent instead of failing.

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
- Fixes:
  - The first second or two of FLV files was left out of the transcript.
  - Chat or a thumbnail still arriving for a removed VOD could land on the next one added.
  - A YouTube chat could fail when its transcription finished first.
  - Stop, Pause and Remove could miss a VOD that had just started transcribing.
  - Switching VODs could briefly show the previous one's transcript, with its chat timeline at the wrong scale.
  - Two quick changes in Settings could be saved out of order.
  - Timeline layers that failed to load could be overwritten.
  - On Linux, a link dropped onto a player opened in the player instead of being added, and the drop overlay could stay on screen.
  - Stopping or removing a download could leave partial files behind.
  - A rare crash on an unusual upload date.

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
- Fixes:
  - The GPU add-on's progress shows when Settings is reopened mid-download.

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
