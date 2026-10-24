# Changelog

What changed in each release, for people who use VODscribe. The entry for the version being released is also the release notes in `latest.json` (`scripts/release.py` reads it), so write it for them, not for developers. The newest release is first; a version still being worked on is marked "(unreleased)".

## 0.6.1 (unreleased)

- The keyboard button is now at the right of the player's controls, beside the captions and full screen buttons, just above the cheat sheet it opens.
- A VOD from a site whose player VODscribe can't show says just "Press Play to download the video and watch it here." under its thumbnail; the note before downloading still explains why.
- Ctrl+A (⌘A) with nothing to pick, such as over the library page's main side, no longer selects all the text on the page.
- Where the timeline has no waveform (a VOD not transcribed there yet, a stretch of an edit without one), it shows a thin line instead of a thick bar, so the waveform stands out from what isn't there yet.
- A video on this computer (a file or a kept video) shows its picture as soon as it opens: the first frame, or the one where you left it, instead of its library picture. Play then starts at once, without the moment of black while the video got going.
- Fixes:
  - A VOD from a site whose player VODscribe can't show, played while its video downloads, no longer loses its picture when the download finishes and refuses to play on: it carries on from the video kept, where it was.

## 0.6.0 — 2026-10-24

- Adding a VOD or a video file no longer transcribes it by itself: it opens, ready to watch and clip, and its row says "Not transcribed". Click Transcribe on the row when you want its transcript. One partly transcribed says how much ("Transcribed 24:00 of 3:04:00"), and Transcribe the rest does what's left.
- Transcribe, at the right under the player beside the captions button (or T), transcribes the selection, if there is one, or else the whole VOD: what isn't transcribed yet, or, once it all is, all of it again after asking. Pointing at it lights up on the timeline what it will transcribe, as Clip does. Press and hold it to transcribe from the playhead instead, for 5 minutes, 15 minutes, an hour, to the end, or the whole VOD; it remembers your last choice. With a selection, it also transcribes that stretch again, replacing its lines.
- Where part of a VOD isn't transcribed, its transcript says so in place, from when to when, with Transcribe for just that part; a part waiting or being transcribed says how far it is, with Stop. A VOD with nothing transcribed yet offers Transcribe the whole VOD or Transcribe from here, and while a part is transcribed, the top of the transcript says which.
- A search that finds nothing in a VOD transcribed only in part says that only chat and the transcribed parts were searched, with Transcribe the rest. Across all VODs, it says how many aren't transcribed, so only their chat was searched.
- With nothing transcribed, the captions button is grayed out and says to transcribe for captions. A video file's captions come as soon as a part of it is transcribed, without opening it again.
- The timeline of a VOD from a site shows its waveform only over the parts transcribed so far; elsewhere it's a flat line.
- A video on this computer (a file, a kept or downloaded video) shows its sound, speed and captions buttons under the player as soon as it opens, before you press Play: set them first and it starts that way. The speed you pick is now remembered, like the volume, for every video and after VODscribe restarts.
- A Twitch VOD's player now has the volume and mute controls under it too, kept in step with Twitch's own. Twitch's player has no speed setting, so that one stays out.
- YouTube, Twitch and Kick VODs get captions from their transcript: in a strip under the player, since nothing may be drawn over a site's player, turned on and off with the same captions button. With captions on, the strip is there as soon as the VOD opens, so the timeline and buttons below don't jump when you press Play.
- Pointing at the timeline of a video on this computer (a file, a kept video, an edit's pieces) shows a small picture of that moment beside the time, and so does dragging the playhead. It's slightly see-through, so the waveform under it still shows, and it sits in the timeline's waveform beside the line, the time beside it, staying in place as you move. Between two of a video's keyframes (far apart in a screen recording), it shows the one before the pointer, the frame the picture there is made from. Dragging a selection's edge shows the frame at that edge: for the end, the last frame inside it, which the player now shows too. The pictures are made in the background from when a video is added, so a long one takes a moment. Once the video has played, rest the pointer and the picture becomes exactly the frame there, the one Play would start on; click there and play, and the video starts at once, without the wait while it finds the frame.
- A file's picture in the library is now a frame an eighth of the way in (it was a tenth), taken from the small frames VODscribe now keeps of each video on this computer for the timeline.
- A VOD from a site whose player VODscribe can't show (anything but YouTube, Twitch and Kick) now shows its thumbnail: press Play to download its video, after the usual note about downloading, and it plays in VODscribe as it comes in (waiting where it hasn't yet), then stays on this computer. A site whose video can't come in parts plays it once it's all in. Keep video on the row does the same.
- Settings opens as a window of its own, with a section list, instead of a dropdown. In its Help section, Reset settings and Repair VODscribe are set apart under their own heading, Reset and repair, marked in red as the other buttons that delete things are, each with a line on what it does.
- Every keyboard control can be changed, in Settings › Controls: several keys per action, modifiers allowed, reset one or all.
- The keyboard button above the player now opens a cheat sheet under the player's controls, so the picture no longer moves down when it opens: the keys in four short columns, Playing, Cutting, Marks and More (two by two in a narrow window, scrolling in a short one), each with a few words on what it does (an edit's own keys are under Cutting), and All controls › to open Settings › Controls. Left open, it stays open for every VOD and edit, and after VODscribe restarts.
- Search finds a phrase even when it runs from one line of the transcript into the next. Words in quotes are found in any order, as long as they're all said within 20 seconds of each other: `"all" "words" "any" "order"`. Unquoted words are still a phrase, in order, and you can mix the two: `nice "clutch"` finds "nice" with "clutch" anywhere near it. A result that runs over several lines shows them all, each part found marked.
- Put a minus before a word or a quoted phrase to leave out matches with it nearby: `i did it -frick`.
- OR finds either side: `well that was OR damn`. AND binds more tightly than OR, and brackets group: `(from:alice "hi") OR (from:bob "gg")`. Under the search box, a search with more than one part shows how it was read, as chips: OR between the alternatives, brackets round groups, a crossed-out chip for a minus. A search VODscribe can't read, such as one with a quote left open, says what to fix there instead.
- New words to type in a search: `from:name` for what a channel said and wrote or what a chatter wrote (`from:"Some One"` for a name with a space, `from:ada,marlowe` for several; on its own it lists everything they said and wrote), `in:chat` or `in:audio`, `within:2m` for how close together the words must be (seconds, minutes or hours; 20 seconds unless you say), and `after:1:20:00` or `before:2h` for where in the VOD. Each one shows as a chip with an × to take it out of the search.
- The search filter has Within and From whom too, with names suggested as you type: channels first, then the people in chat. From whom takes several names separated by commas, and on its own, with nothing typed, lists everything they said and wrote. For a YouTube channel, it says that their name in chat can differ from the channel's. What you type wins over the filter, and the filter then shows the typed value, grayed out, set by the search text.
- Pick several things at once: Ctrl-click (⌘-click on a Mac) clips in the clip bar, stretches between cuts on the timeline, or VODs and edits in the library, to add each or take it out again, and Shift-click to pick the whole run from there to here. Or drag a box over them, starting from empty space in the clip bar or below the library's list. With two or more picked, each shows its number in the order you picked them, and the clip bar or the top of the list says how many, with what you can do with them. A plain click still plays, opens or moves the playhead as before. Escape, or a click on empty space, lets them go; Ctrl+A (⌘A) picks everything on the side of the window the pointer is on: over the library, its whole list; over the player, timeline, transcript or clip bar, all the clips. Only one kind of thing is picked at a time: picking clips or stretches lets go of library rows and the other way round, and selecting a cut, chapter or highlight lets go of both, so Delete always acts on just what's selected.
- With stretches picked on the timeline, C (or Clip) makes a clip of each, in the order you picked them. Delete takes the clips picked out of the clip bar, and in an edit the pieces picked out of it too, all in one go that a single undo brings back.
- VODs and edits picked in the library are removed together with Delete, or Remove… above the list: it asks once, saying how many of each.
- Drag what you picked to put it all in an edit, in the order you picked it, as one step a single undo takes back: drag one of the clips or rows picked (or Alt-drag a stretch picked, or pull a picked piece off an edit's timeline). Let go on an edit in the library and they go at the end of its timeline; on "Make new edit", they start a new one. With the edit open, let go on its timeline and they go in where the marker shows, or on its clip bar and each becomes a clip there. A VOD picked in the library goes in whole, as one piece. Several picked clips dragged out of the window, into a video editor or a folder, take all their files in one drag, once each is made.
- Without a mouse: Add to edit… above the clip bar's picked clips, or above the library's picked rows, lists your edits, then puts them on the edit's timeline (at the end) or on its clip bar.
- With several VODs picked in the library, pointing at Transcribe on one of them lights up all the picked ones it would transcribe, and a click asks for them all, in the order you picked them; they're transcribed one after another.
- Fixes:
  - Stopping a transcription keeps the lines transcribed so far, instead of throwing them away.
  - Moving the playhead in an edit or a clip just opened in the editor, before it is ready to play, no longer puts it back where it was a moment later: a cut (B) goes where the time shows, and Delete after it takes out only the stretch at the playhead, not the whole piece.
  - A selection started with I (or O) now stays when you click the timeline to move the playhead, so O (or I) there ends it where you clicked. Escape still lets it go.
  - On a Mac, the first time you play a file from your Downloads, Desktop or Documents folder, the player says it's waiting for permission to read that folder while macOS asks, rather than sitting black.
  - The window no longer sometimes stays empty for about two minutes when VODscribe starts.
  - The desktop's media controls show a Twitch VOD's own title and artwork again, as before 0.5.0. Turning WebKit's Media Session off then, for a desktop freeze seen once on GNOME, turned out not to reduce what WebKit tells the desktop, and it made the desktop controls fall back to a bare title.
  - A clip bar with more clips than fit, so that it scrolls sideways, takes clicks on its clips again: on some Linux systems, a click on the lower half of a clip, its name or its buttons did nothing.
  - A chat that stops downloading partway, as a long Twitch VOD's sometimes does when Twitch answers "service error", keeps what came in: the chat column and the strip beside the transcript show it, and the header says the chat stopped partway and why, with Try again, which fetches the rest from where it stopped. Before, the chat that had come in disappeared, and came back only while Try again ran. A part of the chat that Twitch or Kick refuses for a moment is now asked for again, for about half a minute, before the download stops. A Twitch VOD's chat no longer ends with "service error" at the VOD's end: its last messages can run a second or two past the video, and asking Twitch for anything past the end is what it refused every time; the download now stops where the video does.
  - In an MPEG-TS video file (.ts), moving the playhead or stepping a frame now shows the frame at that time. It showed one from up to a few seconds later, the next keyframe, and stepping there stood still on it.
  - Pointing at an edit's timeline now shows the small picture of the moment over pieces of a VOD from a site too, once that part of its video is downloaded, not only over pieces of video files.

## 0.5.1 — 2026-10-23

- I and O set where the selection starts and ends, as in video editors: I at the playhead is its start, O its end. With nothing selected, I selects from the playhead to the next cut (or the end), and O back to the cut before (or the start). Put past the other end, the two swap, so the selection runs between them. B, C and (in an edit) Delete then act on it as on lines selected in the transcript.
- Press and hold the speed button under the player for a menu of the speeds, the current one ticked: let go over one to play at that speed, or anywhere else to leave it as it was. A click still steps to the next speed.
- Double-click a search result to see it where it was said: it plays, as a click does, and the VOD opens in the results' place with its transcript scrolled to that line (or, for a chat match, to that message in the chat), lit for a moment. Your words stay in the search box: click in it, or change them, to see the results again.
- Back and forward know your searches: leave a search's results (double-click a result, open a VOD, clear the search box) and Back brings them back as they were, with the same words and filter, scrolled where you left them. A VOD removed since has no matches there.
- The transcript follows the video as it plays: the line being said is lit up and kept in view, the page moving on once the line nears the bottom rather than with every line. Scroll the transcript yourself and it stops following; Follow along, at the bottom of it, goes back to the line playing and follows again, as does a click on any line.
- Pointing at the timeline marks the same moment with a thin line on the strip beside the transcript, and pointing at a line of the transcript or a chat message marks its time on the timeline.
- Fixes:
  - After a click on the timeline, the time by the play button no longer flicks back to where the video was while the picture catches up: it shows the new time at once and stays on it.
  - Moving around a video while it plays (a click on the timeline, the arrow keys) no longer flashes the Play button for a moment: it goes on saying Pause.
  - The time and frame count under a Twitch video no longer jump back and forth about once a second while it plays.

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
