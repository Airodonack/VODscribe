# The page's components and modules

Svelte components (`.svelte`), modules (`.ts`) and stores (`.svelte.ts`). The website builds the same files with stand-ins for the shell.

- `*.test.mjs`: unit tests of the module of the same name, run with `node --test`; `modules.test.mjs` checks that no two module names differ only in case.
- [`logos`](logos): the emote providers' logos.

## The window

- `App.svelte`: the whole window: the library, the player or an edit, the transcript, search and the side columns.
- `api.ts`: the shell's commands and events, typed.
- `fanout.ts`: one listener per event, told to subscribers in order.
- `nav.svelte.ts`: back and forward between what was open.
- `shortMode.svelte.ts`: a short window shows the player alone.
- `EdgeStrip.svelte`: a side column's edge, which hides and shows it.
- `LibraryProblem.svelte`: the recovery screen when the library won't open.
- `Setup.svelte`: first-run setup.
- `UpdateBanner.svelte`: the note that a newer version is out.
- `updates.svelte.ts`: checks for updates and installs one when asked.
- `theme.svelte.ts`: light, dark or the system's colours.
- `platform.ts`: this computer's OS.
- `demo.ts`: what the website's demo turns off; nothing in the app.
- `links.ts`: the website's addresses and the source's.

## Library

- `Library.svelte`: the library's list of VODs and edits.
- `VodRow.svelte`: a VOD's row: thumbnail, title, status and actions.
- `EditRow.svelte`: an edit's row, which takes dropped clips.
- `Thumb.svelte`: a VOD's local thumbnail.
- `ChannelDisc.svelte`: the streamer's picture beside the channel's name.
- `adder.svelte.ts`: adds pasted links and dropped or picked files, one after another.
- `actions.ts`: what can be done with a VOD or an edit, wherever it shows.
- `refresh.svelte.ts`: a site's VOD's Refresh details, which asks its site and the file host again.
- `removal.svelte.ts`: removing a VOD.
- `reorder.svelte.ts`: drag to reorder a list, with Alt+↑ and Alt+↓.
- `readAhead.ts`: reads a VOD before it's opened, so it opens at once.
- `kept.ts`: what's been read of recent VODs, kept for the session.
- `Jobs.svelte`: the jobs under way at the bottom of the library.
- `jobs.ts`: what the jobs list says.
- `videos.svelte.ts`: whole videos downloaded on request.
- `playable.ts`: what this computer's web view plays, asked at each start.
- `ResolveNote.svelte`, `resolveDrop.svelte.ts`: the note at a drop on DaVinci Resolve on Linux, which can't read MP4.

## The player

- `Player.svelte`: a VOD's player, in the site's player or the app's own.
- `PlayerPanel.svelte`: the player's panel, shared by a VOD and an edit.
- `PlayerBar.svelte`: the row of controls under a player's timeline.
- `Controls.svelte`: Settings › Controls.
- `NativePlayer.svelte`: a file in the app's own player.
- `LocalPlayer.svelte`: a file in the web view's `<video>`, where the app's player can't draw.
- `Screen.svelte`: tells the shell where the native screen goes.
- `screenOverlay.ts`: captions and notes drawn over the native picture.
- `textWrap.ts`: text wrapped to a width, for that overlay.
- `nativeDriver.ts`: the page's side of the app's own player.
- `ownPlayer.svelte.ts`: whether the app's own player draws here.
- `ownStatus.ts`: what the app's own player last said of itself.
- `embed.ts`: drives YouTube's and Twitch's players over postMessage, and plays Kick's stream.
- `settle.ts`: a site player's play or pause, passed on once it settles.
- `heldMute.ts`: a mute set before Twitch's player takes one, held till it does.
- `startWatch.ts`: whether Twitch's player refused to start on Play.
- `drivePlayer.svelte.ts`: the loop that reads a player every frame.
- `playerReading.svelte.ts`: what a player's controls show.
- `playerClock.ts`: a player's position, run on between reports.
- `playerPrefs.svelte.ts`: the volume, speed and captions every player shares.
- `playerKeys.ts`: the keys both players share, and their legend.
- `mediaSeek.ts`: seeks that land on WebKitGTK.
- `media.ts`: the arithmetic of captions, waveforms, frame steps and cuts.
- `hold.svelte.ts`: a bar button right-clicked or held opens its menu.
- `TranscribeFrom.svelte`: the Transcribe menu.
- `Shortcuts.svelte`: a player's keyboard legend.
- `PermissionNote.svelte`, `permission.ts`: the note before downloading from a site.

## The timeline

- `Scrubber.svelte`: the timeline under every player, with its waveform, marks and clips.
- `ScrubFrame.svelte`, `scrubFrame.ts`: the thumbnail of the moment under the pointer.
- `exactFrame.svelte.ts`: the exact frame for that thumbnail.
- `latest.ts`: one request at a time, the newest winning.
- `resting.ts`: when the pointer comes to rest.
- `Minimap.svelte`: the whole VOD beside the transcript, with how busy it was.
- `Timeline.svelte`: the Timeline panel: whose marks show, and the layers.
- `layers.svelte.ts`: saved chat searches drawn on the timeline.
- `events.ts`: subs, gifts and tips as a timeline layer.
- `markSources.ts`: whether your own marks show, or imported ones, or both.
- `cuts.svelte.ts`: a VOD's or an edit's cuts, chapters and highlights.
- `MarkEditor.svelte`: a chapter's title or a highlight's note.
- `markHover.svelte.ts`: the mark under the pointer, lit in both places.
- `stretches.ts`: what of a VOD is transcribed.

## Clips and edits

- `ClipStrip.svelte`: the clip bar.
- `clips.svelte.ts`: the open VOD's clips.
- `EditView.svelte`: an edit, opened in the player's place.
- `editPlayer.ts`: an edit's player.
- `edits.svelte.ts`: edits.
- `AddToEdit.svelte`: Add to edit, without dragging.
- `dragout.ts`: carrying a stretch or a clip into an edit or out of the window.
- `pressToDrag.ts`: a press that becomes a drag.
- `selection.svelte.ts`: several things picked at once.
- `pick.ts`: the picking rule, as in a file manager.
- `insideDrag.ts`: which native drops come from a drag that started inside the page.
- `bulk.ts`: what's done with everything picked.
- `where.ts`: where Ctrl+A selects and Ctrl+V pastes.
- `copyPaste.ts`: Ctrl+C, Ctrl+X and Ctrl+V.
- `clipboardText.ts`: the text a copy puts on the system's clipboard.
- `undo.ts`: undo and redo.
- `writes.ts`: changes sent and not answered yet.

## Transcript, chat and search

- `Transcript.svelte`: the transcript with its chat, or an edit's.
- `TranscriptHead.svelte`: the switch between transcript and chapters on a narrow window.
- `rows.ts`: the transcript's rows, each line with its chat.
- `transcriptGroups.ts`: a speaker's lines grouped under one head.
- `transcriptReads.ts`: the transcript read whole or by range, one read at a time.
- `pinned.ts`: headings pinned at the top while their lines show.
- `follow.ts`: the transcript following the playhead.
- `ChatMessage.svelte`: one chat message.
- `chatParts.ts`: a chat message's text and emotes.
- `Emote.svelte`: one emote image.
- `EmoteProviders.svelte`: the 7TV, BetterTTV and FrankerFaceZ switches.
- `marks.svelte.ts`: what the transcript marks in chat.
- `Results.svelte`: search results, speech and chat side by side.
- `SearchFilter.svelte`: whether a search looks through speech, chat or both.
- `SearchGrouping.svelte`, `queryChips.ts`: how the search was read, as chips.
- `textFocus.ts`: text fields and the timeline's keys taking turns.

## Annotations and VODscriptions

- `annotations.ts`: the annotations file's shape.
- `annotated.ts`, `annotated.svelte.ts`: the open VOD's cast and activity.
- `annotationRows.ts`: the chapters and highlights pane's rows.
- `AnnotationsPane.svelte`: chapters and highlights beside the transcript.
- `AnnotationsOffer.svelte`, `offers.svelte.ts`: the offer of a VOD's VODscription.
- `CastDisc.svelte`: a cast member's disc.
- `accounts.ts`: where a cast member's account and picture come from.
- `castLink.svelte.ts`: where a cast member's link leads, their own stream at the moment first.
- `streamers.ts`: the streamer card's file, its index, and reading them.
- `streamers.svelte.ts`, `StreamerCard.svelte`: the streamer card a name or picture opens.
- `castNames.ts`: what a cast member is called on screen.
- `povs.ts`: another streamer's view of the same moment.

## Settings and small parts

- `Settings.svelte`: Settings, a section at a time.
- `settingsWindow.svelte.ts`: whether Settings is open, and at which section.
- `codecs.ts`: the clip formats Settings offers, greyed out where ffmpeg can't make them.
- `appSettings.svelte.ts`: the settings, loaded once and saved whole.
- `asks.ts`: the notes that can be turned off, and what's done with a VODscription found.
- `keymap.ts`: every keyboard control and its default keys.
- `keys.svelte.ts`: the keys in force, with the user's own over the defaults.
- `stored.ts`: small choices kept in the data folder.
- `DownloadStep.svelte`: a download's current step.
- `Modal.svelte`: a window over the app.
- `Popover.svelte`: a panel opened from a button, or at the pointer.
- `swallowClick.ts`: the press that closed a menu, kept from the page.
- `portal.ts`: moves an element to the end of `<body>`.
- `Menu.svelte`: a menu's choice.
- `Tip.svelte`: a tooltip.
- `Toast.svelte`, `toasts.svelte.ts`: short messages at the bottom of the window.
- `ask.ts`: asks before something that can't be undone.
- `Progress.svelte`: a progress bar.
- `Spinner.svelte`: a turning ring.
- `Icon.svelte`: inline icons.
- `Logo.svelte`: VODscribe's mark.
