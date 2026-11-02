# Changelog

What changed in each release, for people who use VODscribe. The entry for the version being released is also the release notes in `latest.json` (`scripts/release.py` reads it), so write it for them, not for developers. The newest release is first; a version still being worked on is marked "(unreleased)". An entry opens with two or three plain sentences on what's new, as you'd tell a friend, then its changes. Each change opens with a headline of at most eight words, a plain statement ending in a full stop with no colon, dash or parenthesis, then one sentence of detail (a second only for a key, a menu name or a limit). A change goes beside the others about the same part of the app (the player and its timeline, clips and the editor, the transcript and search, chat, chapters, highlights and VODscriptions, the library, settings and privacy, the website), the bigger first, and the fixes under the closing "Fixes:" are grouped the same way.

## 0.7.1 (unreleased)

The clips and edits being made now show in a list at the foot of the library, where you can follow them, cancel one or move it up the line. Copy, cut and paste work on clips, and on Linux, Windows and a Mac a clip dragged out of the window goes at once. The service that brings a VOD its annotations is called VODscription from now on.

- Ctrl+C and Ctrl+V copy and paste clips. Ctrl+C (⌘C) copies the selection or the clips picked, and Ctrl+V (⌘V) pastes them as new clips on a clip bar, or into an edit at the playhead. A paste is the clip as it was when copied, with its video if it was already made, whatever happens to the original after. With a stretch selected on an edit's timeline, the paste goes in after it and is selected, so pasting again builds a sequence.
- Cut clips with Ctrl+X. Ctrl+X (⌘X) copies the clips picked and takes them off their bar, or out of the edit.
- Copied clips go on the clipboard too. Each is a line of text, with a link to the moment for a site's VOD.
- Ctrl+A can select the whole timeline. Ctrl+A (⌘A) over the player, the timeline or the transcript selects the whole timeline, ready to copy; over the clip bar it still selects all the clips, and over the library its list.
- On Linux, clips drag out at once. The drag doesn't wait at the window's edge for the file, which lands where you drop it once it's made; a file manager waits for it (Dolphin takes each file as it's ready, in a KDE session or not), and Escape before then lands nothing.
- On Windows, clips drag out at once. File Explorer copies the clip in once it's made, showing its copy dialog meanwhile.
- On a Mac, clips drag out at once. Finder shows the file with a progress bar until it's made, and its cancel button stops it.
- Clips start as you pick them up. From a video on this computer, a stretch's clip, or several, is made from the moment you pick it up, first in line; a site's is downloaded once the drag leaves the window, and a clip of an edit follows its videos.
- Long downloads no longer ask first. A clip downloaded from a site is made however long it is.
- Some apps take finished files only. Let go on one before the clip is made, nothing lands there, VODscribe says which app it was, and the clip shows on the clip bar to drag again once it's ready.
- A clip still being made drags out. Drag it from the clip bar as you would a ready one.
- Dragged clips are listed once let go. A clip made only for a drag stays off the clip bar; once you let it go on another app it's in the list at the bottom of the library, where Cancel stops it and nothing lands.
- A drag called off makes nothing. Escape, or a drop on nothing, stops the clip's making; dropped on the clip bar, it's kept there. If the clip can't be made, VODscribe says why only when you let it go on another app.
- A carried stretch shows a dashed outline. Alt-dragging (Option-dragging on a Mac) a stretch of the timeline outlines what you picked up as soon as the drag starts.
- An unpicked stretch is carried alone. Alt-drag a stretch from outside what's picked and the others are let go, as in a file manager.
- Dragging an edge draws a guide line. On an edit's timeline (or a clip's, in the editor), a dashed line runs down the waveform where the edge is, to line it up with what you hear and see.
- The annotation service is called VODscription. The service that brings a VOD's published transcript, chapters, highlights and cast has that name throughout the app and the website: the gift button offers the VOD's VODscription, and a library row's menu has Check for a VODscription and Check all VODs for VODscriptions.
- The streamer's picture shows by their channel. When a VOD's annotations name the streamer whose channel it is, their picture shows beside the channel's name under the VOD's title and in its row in the library, as in Cast. With streamer avatars turned off in Settings › Privacy, nothing shows there, not even an initial as in the transcript and Cast.
- A list shows what's being made. At the bottom of the library a line says what's under way, as a file manager shows its copies: the clips and edits being made, how far the current one is, and the VODs being erased after you removed them. Click it for the list, with each one's name, what it's doing, how far it is, and a button to cancel it.
- Waiting clips and edits can be reordered. Drag a clip or edit that's waiting up or down the list to have it made sooner or later.
- A canceled clip or edit says so. A canceled clip says so on the clip bar (clip that stretch again to make it); a canceled edit is left not rendered.
- Removed VODs are erased at once. It's erased beside whatever is being made rather than as VODscribe quits, so quitting is as quick as with no VOD removed. Quit, or cancel it in the list, before it's done, and the rest is erased when VODscribe next starts.
- The donation links are gone. They're off the library and the screen shown when no video is open.
- The two privacy settings are renamed. Settings › Privacy has "Enable VODscription annotations for VODs" and "Enable streamer avatars from img.vodscribe.com", which work as before.
- A short window shows the player alone. Show transcript and chat, at the right of the VOD's title, swaps them, and the transcript stays in its place as you open other VODs.
- Fixes:
  - An edge can't empty the selection. Moving a selection's edge with , and . onto the other edge stops a frame short, where it left a selection of nothing that Delete and C refused.
  - B twice quickly makes one cut. It made two on the same frame.
  - Splitting at a join leaves no sliver. In an edit, Split (B) on a selection that ends where two pieces meet left a sliver of the earlier piece cut off before the join.
  - The transcript keeps its scroll position. Switching the transcript's heading to Highlights (or Chapters) and back started the transcript again at the top.
  - The Transcribe menu lists each stretch once. Near a VOD's end, lengths that would run past the end are left out, as To the end covers them.
  - Cuts before playing use the right frames. Lines picked in the transcript of a video that hasn't started playing are cut on its frame grid, not on that of the video open before it.
  - The Marks toggles show your saved choice. Just after a VOD opened, the Timeline panel could show both kinds of marks on for a moment, and a click then was lost.
  - Arrows work after clicking anywhere on a row. A click anywhere on a row, not only on its title, lets ↑ and ↓ move to the row before or after.
  - Show in folder works during a download. While a VOD's download is in progress, paused or stopped by an error, it shows the partial file as it is.
  - A VOD reopens where it was left. A step or jump made just before opening something else could be forgotten, and the VOD came back a step earlier.
  - Reload brings back what was open. Reload, in the right-click menu, reopens what was open where it was, instead of leaving nothing open while the video's player carried on.
  - Undo undoes the latest change. Undo (Ctrl+Z) pressed right after a change, such as moving a mark, could undo the one before it instead.

## 0.7.0 — 2026-10-30

A VOD can now come with annotations: a transcript that reads like a chat, with who's speaking, and chapters and highlights in a column of their own, offered when you add the VOD. Picking and dragging clips, rows and stretches works as in a file manager, a video on this computer shows its picture the moment it opens, and the choices VODscribe remembers, such as the volume, now last after it restarts.

- Videos on this computer open on a frame. A file or a kept video shows its first frame, or the one where you left it, instead of its library picture, and Play starts at once, without a moment of black while the video gets going.
- The keyboard button moved right. It sits at the right of the player's controls, beside the captions and full screen buttons, just above the cheat sheet it opens.
- Downloaded VODs have a shorter note. A VOD from a site whose player VODscribe can't show says just "Press Play to download the video and watch it here." under its thumbnail; the note before downloading still explains why.
- A missing waveform is a thin line. Where the timeline has no waveform (a VOD not transcribed there yet, a stretch of an edit without one), a thin line replaces the thick bar, so the waveform stands out from what isn't there yet.
- Clicking a clip selects it. As in a file manager, clicking another selects that one instead, and clicking the selected one again lets it go; Ctrl-click (⌘-click on a Mac) and Shift-click still pick several.
- Each clip has a play button. It plays the VOD from the clip's start.
- Double-clicking a clip opens the editor. The editor's top names the VOD or edit the clip comes from before the clip's own name. Click that name, or Back to under the player, to go back to that clip bar with the clip selected.
- Long clip bars scroll sideways. The mouse wheel over a long clip bar scrolls it sideways, and so does dragging with the middle button held down.
- Clips are added and saved by dragging. A clip's Add to an edit (+) and Save a copy buttons are gone: drag the clip onto an edit in the library to add it there, with any cuts you made in it in the editor, as + put it, or out of the window into a folder or a video editor to save a copy.
- The clip bar's several-picked row is gone. The row at the start of the clip bar (how many, with Clip, Add to edit… and Remove) gave way to the same keys and gestures as for one: C, or Clip under the player, clips each stretch picked, Delete takes the picked clips out, and dragging any of them carries them all, in the order you picked them. Escape, or a click on empty space, still lets them go.
- Edits have selections as VODs do. Lines chosen in its transcript, or a stretch set with I and O, show both edges on the timeline, to drag or, once clicked, move a frame at a time with , and . (or as far as the arrows, J and L go). Split (B) cuts both ends, Escape lets it go, and Delete takes it out of the edit as before.
- Holding an edit's picture picks up a stretch. As on a VOD's player, holding the picture picks up the stretch at the playhead, or its piece, to carry onto an edit, the clip bar or out of the window.
- Edits have a Clip button. As C does, it clips the stretch at the playhead, or what's selected, to the edit's clip bar, and it can be dragged the same way as a VOD's.
- A adds picked clips to an edit. With clips picked in the clip bar, or stretches picked on the timeline, A picks the edit they go in, and where in it, without dragging them, as A does on a highlight.
- Clips can be any length. The three-hour limit is gone.
- An edit's source links moved. An edit's source links sit on the clip bar's row, as a VOD's edit links do, so the player gets the row back.
- The transcript follows from the start. From the moment a VOD or an edit opens, the line at the playhead is lit and in view.
- The transcript follows every playhead move. A click on the timeline, a highlight or chapter, a search result, the arrow keys and J and L, or a seek in the site's own player all move it, and a jump far away puts the line a third of the way down. Scrolling the transcript yourself still stops it following, and Follow along brings it back.
- Annotated transcripts read like a chat. A person's lines sit one after another under one heading with their initial in a coloured circle, their name in their colour and when they started; a new heading comes when someone else speaks, a new chapter starts or after a pause of more than 20 seconds.
- Unnamed lines sit under Unknown. In a transcript that names others, lines nobody is named for get a heading of their own with a plain outline of a person in its circle, so they never read as said by the person before them.
- Each chapter opens with a divider. The divider in the transcript shows the chapter's emoji and title.
- The speaker and chapter stay in view. As you scroll a long transcript, the chapter and the heading of the person speaking (Unknown over unnamed lines) stay pinned until the next heading pushes them up, a new chapter pushing up both, and a line a search, the timeline, the arrow keys or Follow along take you to shows below them. Click the name to open their channel, or anywhere else on it to go back to where their lines or the chapter start.
- Characters link to their streamer. When annotations name the streamer behind a character, the name in the transcript links to their channel and their picture replaces the initial, which stays when there's no picture; the heading reads the character's name with the streamer in parentheses, "Mr. C (Burn)", and so does Cast above the transcript, with a link to their channel.
- Cast and highlights show pictures. The same picture, or the initial in their colour, is beside each name in Cast and beside each person in an opened highlight.
- Opened highlights use short names. It's the short name the annotations give, the character's own, never cut off but wrapped in a narrow column, and a person whose streamer is named links to that streamer's channel (point at it to see who and where).
- Highlight rows show who's in them. Small overlapping circles at the end of the row's time and length leave the title the row's whole width.
- Names link to a stream's same moment. When annotations say another streamer was live at the same time, their name opens their stream at that moment instead of their channel: a transcript heading at when its lines start, a person in an opened highlight at the highlight's start, and Cast at the playhead. Outside their stream, or when VODscribe doesn't know when this VOD started, the link is their channel as before.
- A stream in your library opens there. Otherwise it opens on Twitch or YouTube at that moment, and a Kick video at its start, as Kick's links can't name a time; point at the name to see where it goes, and one that opens in your browser shows the small outward arrow after it, as other links that leave the app do.
- Text-to-speech voices read as TTS. A text-to-speech voice in the cast has a voice sign in its circle and links nowhere.
- A VOD can come with annotations. They bring its transcript, chapters, highlights, who's speaking, what the stream is doing, and its chat.
- Chapters show above the timeline. They're gray slices, each with its emoji when the file gives one (🎮 for a game, 💬 for a chat), the one playing is named beside the VOD's title, and a thin band under them shows when it's a game, just chatting or an event.
- Highlights show on the timeline. Each is its kind's emoji (😂 funny, 🔥 hype, 🍿 drama, 💛 wholesome and more; ⭐ for a kind the app doesn't know), several close together show as one with how many, pointing at one shows its title and why it stands out, and a click goes to it.
- Cast lists who's speaking. Transcript lines name their speaker in that person's colour, and Cast above the transcript lists everyone with their role and other names.
- Newer annotations keep your own changes. They replace what the last ones brought, but your own marks stay, and so does an imported chapter or highlight you renamed or moved, without the newer annotations adding their copy beside yours.
- Chapters and highlights get a column. It runs the full height of the window on the right, as the library does on the left, with the player and transcript between, in the app as on the website; it scrolls on its own and keeps the chapter and highlight playing lit and in view.
- The column and timeline light together. Point at a chapter or highlight in the column and it lights up on the timeline, and point at one on the timeline and its row lights up.
- Narrow windows switch transcript and highlights. Three columns need a window about 1440 px wide or wider with the library showing, 1000 px without it; narrower, or on a phone on the website, a Transcript | Highlights switch on the transcript's heading shows one or the other in the same place. It reads Chapters, or Chapters & highlights, when that's what the VOD has, and the one you leave open stays open.
- The column shows only with chapters or highlights. It shows for a VOD or an edit with chapters or highlights; otherwise the transcript has the room, and on a narrower window the switch's Highlights side is greyed.
- The side columns can be hidden. Click the edge between the library and the player, or between the player and the column, to hide that side, and click it again at the window's edge to bring it back; point at an edge and an arrow by the player shows which way it goes. Ctrl+Shift+B and Ctrl+Alt+B (⇧⌘B and ⌥⌘B on a Mac) do the same, the side stays hidden after VODscribe restarts, and with the library hidden the column fits beside the player on a narrower window.
- Highlights drag into edits. Drag a highlight onto an edit in the library, or onto Make new edit, and it goes in as one piece named for it. From the keyboard, press A on a highlight (or right-click it, or use the menu key) to pick the edit to add it to.
- Your and imported marks can be hidden. The Timeline button above the transcript has two toggles, Your marks and Imported marks, for the chapters and highlights you made and those from annotations, on the timeline, in the transcript and in the column; both start on and stay as you leave them.
- The column's heading has Copy chapters. As a chapter's title box does, it copies every chapter with its time, ready for a YouTube video's description.
- Adding a VOD offers its annotations. For a Twitch, YouTube or Kick VOD with annotations published, VODscribe offers "A transcript and annotations are available for this VOD", with Use, Not now and Don't ask again. Use brings in its transcript, chapters, highlights and who's speaking in place of an earlier import, keeping the chapters and highlights you made, and the dialog says when a transcript made on this computer will be replaced.
- A gift button marks annotated VODs. At the left of the row, it asks the file host again and offers them, or says you have the latest already, with Replace anyway; with asking turned off in Settings › Privacy, it goes by what the file host said last time and says so.
- Check for annotations asks again. Right-click a VOD from a site in the library and pick it; for a VOD from another site, it says annotations are only published for Twitch, YouTube and Kick VODs.
- Your library is checked for annotations privately. At every start VODscribe downloads one small list of every VOD with annotations, again only when it has changed, and checks your library against it on your computer, so the file host never learns which VODs you have and is asked about one only when the list has it. With asking turned off in Settings › Privacy, nothing is downloaded or asked.
- A dot marks newer annotations. A VOD found shows its gift button, with a dot on it when its annotations are newer than yours, and the dialog says a newer transcript and annotations are available.
- Check all VODs for annotations checks everything. Right-click any VOD from a site in the library and pick it when you hear annotations just went up.
- Picking works as in a file manager. In the library a click opens a row and picks it alone; Ctrl-click (⌘-click on a Mac) then adds another or takes one out, Shift-click picks the run from the last one clicked, and Ctrl+Shift-click adds that run, in the library, the clip bar and on the timeline alike.
- Arrow keys move through the library. Press ↑ or ↓ to open the row before or after, or with Shift held to pick the run to it; Escape, or a click below the list, lets go of all but the open row.
- Alt-click picks a timeline stretch. Alt-click (Option-click on a Mac) picks the stretch between cuts there by itself, Ctrl+Alt-click (⌘⌥-click) adds it to the others, and Alt-drag still carries it out.
- Dragging several shows a pile. The first thing's picture shows over its waveform, as dragging one clip always has, with up to two more stacked behind it and blank cards past three, up to ten, and a count such as 3 stretches; one thing picked by itself drags as if it weren't picked.
- Every library row has a menu. Right-click it (or use the menu key): a VOD's has Show in folder for a file on this computer, Open on Twitch (YouTube, Kick) and Copy link for a VOD from a site, with Show in folder too once its video is downloaded, then Add to edit…, Check for annotations (for a VOD from a site) and Remove…; an edit's has Show in folder once it's rendered, Add to edit… and Delete edit…. In a menu ↑ and ↓ move, Enter picks, and Escape closes it.
- Rows get a Show in folder button. A file's row, and that of a VOD whose video is downloaded, has a Show in folder button, as a rendered edit's row has.
- Removing one VOD asks first. As removing several does, it says its transcript and any downloaded audio or video are erased from this computer.
- VODs opened before open at once. Everything is in place as it opens, and the same goes for the VOD your pointer rests on in the library and the ones beside the open one, which are read ahead.
- Asking for annotations can be off. Settings › Privacy has "Ask the file host whether annotations exist for your VODs", on to begin with; asking sends only the VOD's site and its id on that site, and turned off, or with Don't ask again, nothing is asked.
- Streamers' pictures can be turned off. Under it, "Load streamers' pictures from img.vodscribe.com", also on to begin with, brings the pictures of the streamers annotations name from there, which gets the streamer's site and login and your IP address and passes you on to Twitch's or Kick's own picture; turned off, nothing is asked and their initials show instead.
- Ctrl+Q quits VODscribe. Ctrl+Q (⌘Q on a Mac) quits, as closing its window does.
- Fixes:
  - Playback survives the download finishing. A VOD from a site whose player VODscribe can't show, played while its video downloads, lost its picture when the download finished and refused to play on; it carries on from the video kept, where it was.
  - Opening a VOD moves less. The caption strip under a site's player stays put instead of vanishing for a moment and pushing the timeline and the transcript down, the transcript doesn't call the whole VOD "not transcribed" for an instant before its lines appear, and much less moves as the chat and the chapters and highlights arrive (the chat's minimap still shifts a little).
  - Opening videos back to back works. A video on this computer sometimes stopped at "The player was closed while opening." with no picture, its open cancelled by the close of the one before it.
  - The title's time follows the playhead. It shows and copies a link to where the playhead is, in the app and on the website, and Open on Twitch (or YouTube) beside it opens that moment too; both stayed on the moment you last jumped to, and a VOD played from its start never showed the time at all.
  - Twitch selections start where you clicked. Pressed right after a click on the timeline, I (or O) started the selection at 0:00 if Twitch's player was still loading there, so the whole VOD was selected.
  - Kick links copy without a time. The time beside a Kick VOD's title copies its link without the time Kick ignored, as Kick's links can't say one, and pointing at it says so.
  - The timeline's thumbnail sits left of the pointer. It shows just to the pointer's left (to its right only at the timeline's left end), the time still centred on the pointer; on the right, where what you drag also appears, it could look like a hover that didn't go away.
  - The time follows arrow keys at once. On a video on this computer, → or ← (or Shift with them) moves the time shown at once, as a click on the timeline does, where it waited for the new frame to be drawn.
  - Another video shows its own time. Opening a file, a kept video or an edit while another plays showed the previous one's time under the player for a moment, until the new one's first frame.
  - Untranscribed edits show the CC button. An edit whose videos aren't transcribed shows it, off, as a VOD does, so the player's buttons don't move once its captions come.
  - Errors show at the window's bottom. What goes wrong with a chapter, a highlight, a cut, an edit or a clip shows there wherever you are, and stays until you close it or it's put right. In an edit, a chapter or highlight that couldn't be added, moved or renamed said nothing, Show in folder on an edit's row said nothing with a VOD open, and a clip that couldn't be made said so only in the clip bar, which isn't there while the player is hidden.
  - C twice in an edit makes one clip. Each press on the same stretch made another clip, where on a VOD it makes one; pressed again on a clip that failed, C tries it again.
  - Clipping a selection twice makes one clip. A second press of C or click on Clip, before the first had cut the selection's ends, also clipped the stretch the playhead was in, between the cuts there were before.
  - Clips finish when their VOD is removed. A clip on an edit's clip bar stopped when the VOD it's cut from was removed from the library meanwhile, and showed as being made until VODscribe restarted.
  - Deleting an edit deletes its clips' files. The clips on its clip bar go with their files (unless you dragged one out of VODscribe), as with a VOD's clips; their files stayed in the clips folder.
  - A deleted clip closes in the editor. Deleted from its clip bar, or with the edit or VOD whose clip bar it's on, it stayed open though it was gone.
  - Changing a clip's chapters remakes its file. A clip changed in the editor, so that its file has its chapters, is made again when you add, rename or move a chapter in it, as on undo; its file kept the chapters it had before.
  - Alt-dragging inside a piece carries that stretch. Alt-dragging (Option-dragging on a Mac) the stretch between two cuts inside one of an edit's pieces, such as a clip put in it, carried the whole piece.
  - Choosing lines lets go of marks. In an edit, lines chosen in the transcript while a chapter or highlight was selected left it selected, so Delete took the chapter or highlight instead of the lines.
  - Undo in the message offers Redo. After removing pieces, lines, clips or stretches, Undo in the message offers Redo once used, as Ctrl+Z (⌘Z) does, and says when there's nothing left to undo; it offered nothing.
  - Letting go outside ends a drag. A clip, a library row, the Clip button or a transcript line pressed and let go outside the window started carrying, or selecting, when the pointer came back over the window with no button held.
  - A long drag back isn't a click. Let go where it began after more than half a second, it also opened the row or played from it.
  - Transcription stops when VODscribe crashes. When VODscribe crashed or was ended from outside (a task manager, a log-out), the transcription ran on for a while in the background; something started in the instant you quit, such as a download or an export, stops too.
  - Filter text set by search is clearer. When the search text sets the filter, such as with in:chat or within:30s, the descriptions under Audio and Chat and the units beside Within were faded twice over and hard to make out, in the dark theme most of all.
  - Chatters' names are always readable. In search results and chat they keep their own colour, but a dark one, such as Twitch's default blue, is lighter on the dark theme, and a pale one darker on the light theme.
  - Kept stretches keep subs, gifts and tips. Deleting a VOD with its video while keeping the stretches its clips use turned the subs, gifts and tips in their chat into plain messages; stretches kept that way while the VOD is still in your library get them back.
  - Copied chapters keep one per line. A chapter title with a line break in it, as one from annotations can have, added lines YouTube read as chapters of their own.
  - Click then Ctrl-click picks both. In the library, a click on a row and then a Ctrl-click (⌘-click on a Mac) on another picked only the second, and a click on a row left what was picked before it as it was.
  - Ctrl+A doesn't select the page's text. Over places such as the library page's main side, Ctrl+A (⌘A) selected all the text on the page.
  - Remembered choices last after a restart. The volume, the speed, captions on or off, whether chat shows and the other choices VODscribe remembers were forgotten at every restart of the app as installed, since 0.1.1.
  - Quitting with a video open is quick. With a video on this computer open, it took two seconds, the window left on screen meanwhile.
  - On Linux, the window closes at once. The window stayed up for up to three seconds while VODscribe erased the VODs you'd removed from the library.

## 0.6.0 — 2026-10-24

VODscribe no longer transcribes everything you add: transcribe a VOD, or just the stretch you picked, when you want its transcript. YouTube, Twitch and Kick VODs get captions in a strip under the player, search learns words in any order, minus, OR and from:, and you can pick several clips, stretches or VODs at once and drag them into an edit together.

- Site VODs get captions below the player. YouTube, Twitch and Kick VODs get captions from their transcript in a strip under the player, since nothing may be drawn over a site's player, turned on and off with the same captions button. With captions on, the strip is there as soon as the VOD opens, so the timeline and buttons below don't jump when you press Play.
- Player buttons show before Play. A video on this computer (a file, a kept or downloaded video) shows them under the player as soon as it opens, so set them first and it starts that way.
- The speed you pick is remembered. Like the volume, it holds for every video and after VODscribe restarts.
- Other sites' VODs play by downloading. A VOD from a site whose player VODscribe can't show (anything but YouTube, Twitch and Kick) shows its thumbnail; press Play to download its video, after the usual note about downloading, and it plays in VODscribe as it comes in, waiting where it hasn't yet, then stays on this computer. A site whose video can't come in parts plays it once it's all in, and Keep video on the row does the same.
- The timeline shows a picture of each moment. Pointing at the timeline of a video on this computer (a file, a kept video, an edit's pieces), or dragging the playhead, shows a small, slightly see-through picture of that moment in the waveform beside the line and the time, staying in place as you move.
- Dragging a selection's edge shows that frame. For the end, it's the last frame inside the selection, which the player shows too.
- The pictures are made in the background. They're made from when a video is added, so a long one takes a moment, and between two keyframes (far apart in a screen recording) they show the one before the pointer, the frame the picture there is made from. Once the video has played, rest the pointer and the picture becomes exactly the frame there, the one Play would start on, and Play there starts at once, without the wait while it finds the frame.
- Twitch VODs get volume and mute controls. They sit under the player, kept in step with Twitch's own; Twitch's player has no speed setting, so that one stays out.
- The keyboard button opens a cheat sheet. It opens under the player's controls, so the picture doesn't move down, with the keys in four short columns, Playing, Cutting, Marks and More (two by two in a narrow window, scrolling in a short one), each with a few words on what it does, an edit's own keys under Cutting, and All controls › to open Settings › Controls. Left open, it stays open for every VOD and edit, and after VODscribe restarts.
- Captions gray out until transcribed. It says to transcribe for captions, and a video file's captions come as soon as a part of it is transcribed, without opening it again.
- Site waveforms show only where transcribed. Elsewhere on the timeline of a VOD from a site it's a flat line.
- Pick several things at once. Ctrl-click (⌘-click on a Mac) clips in the clip bar, stretches between cuts on the timeline, or VODs and edits in the library to add each or take it out again, Shift-click to pick the whole run from there to here, or drag a box over them from empty space in the clip bar or below the library's list.
- Picked things are numbered in order. With two or more picked, each shows its number in the order you picked them, and the clip bar or the top of the list says how many, with what you can do with them; a plain click still plays, opens or moves the playhead as before.
- Escape lets go, and Ctrl+A picks everything. Escape, or a click on empty space, lets them go; Ctrl+A (⌘A) picks everything on the side of the window the pointer is on, the library's whole list or, over the player, timeline, transcript or clip bar, all the clips.
- One kind of thing is picked at once. Picking clips or stretches lets go of library rows and the other way round, and selecting a cut, chapter or highlight lets go of both, so Delete always acts on just what's selected.
- Clip and delete several at once. With stretches picked on the timeline, C (or Clip) makes a clip of each in the order you picked them; Delete takes the picked clips out of the clip bar, and in an edit the picked pieces too, in one go that a single undo brings back.
- Drag what you picked into an edit. Drag one of the clips or rows picked, Alt-drag a picked stretch, or pull a picked piece off an edit's timeline, and it all goes in, in the order you picked it, as one step a single undo takes back; a VOD picked in the library goes in whole, as one piece. Let go on an edit in the library to add them at the end of its timeline, on "Make new edit" to start a new one, or with the edit open, on its timeline where the marker shows, or on its clip bar for a clip each.
- Several clips drag out in one go. Picked clips dragged out of the window, into a video editor or a folder, take all their files in one drag, once each is made.
- Add to edit… works without a mouse. Above the clip bar's picked clips, or the library's picked rows, it lists your edits, then puts them on the edit's timeline, at the end, or on its clip bar.
- Adding a VOD doesn't transcribe it. A VOD or video file opens, ready to watch and clip, and its row says "Not transcribed" until you click Transcribe there. One partly transcribed says how much ("Transcribed 24:00 of 3:04:00"), and Transcribe the rest does what's left.
- Transcribe sits under the player. At the right beside the captions button (or T), it transcribes the selection, again if need be, replacing its lines, or else what isn't transcribed yet of the whole VOD, or all of it again, after asking, once it all is; pointing at it lights up on the timeline what it will transcribe, as Clip does.
- Hold Transcribe to start at the playhead. Pick 5 minutes, 15 minutes, an hour, to the end, or the whole VOD, and it remembers your last choice.
- The transcript marks untranscribed parts. Each says from when to when, with Transcribe for just that part, and a part waiting or being transcribed says how far it is, with Stop. A VOD with nothing transcribed yet offers Transcribe the whole VOD or Transcribe from here, and while a part is transcribed, the top of the transcript says which.
- Search finds phrases across lines. A phrase that runs from one line of the transcript into the next is found, and a result over several lines shows them all, each part found marked.
- Quoted words are found in any order. They need only all be said within 20 seconds of each other, as in `"all" "words" "any" "order"`, while unquoted words are still a phrase, in order, and the two mix: `nice "clutch"` finds "nice" with "clutch" anywhere near it.
- A minus leaves out matches. Put it before a word or a quoted phrase to leave out matches with it nearby, as in `i did it -frick`.
- OR finds either side. AND binds more tightly than OR, and brackets group, as in `well that was OR damn` or `(from:alice "hi") OR (from:bob "gg")`.
- Searches show how they were read. Under the search box, a search with more than one part shows as chips, OR between the alternatives, brackets round groups and a crossed-out chip for a minus, and a search VODscribe can't read, such as one with a quote left open, says what to fix there instead.
- Search learns from, in, within, after, before. Type `from:name` for what a channel said and wrote or a chatter wrote (`from:"Some One"` for a name with a space, `from:ada,marlowe` for several; alone it lists everything they said and wrote), `in:chat` or `in:audio`, `within:2m` for how close together the words must be (seconds, minutes or hours; 20 seconds unless you say), and `after:1:20:00` or `before:2h` for where in the VOD. Each one shows as a chip with an × to take it out of the search.
- The search filter gains Within and From whom. From whom suggests names as you type, channels first, then the people in chat, takes several separated by commas, and alone lists everything they said and wrote; for a YouTube channel, it says their name in chat can differ from the channel's. What you type wins over the filter, which then shows the typed value grayed out, set by the search text.
- Empty searches say what wasn't searched. In a VOD transcribed only in part, it says that only chat and the transcribed parts were searched, with Transcribe the rest; across all VODs, it says how many aren't transcribed, so only their chat was searched.
- Delete removes picked library rows. So does Remove… above the list, which asks once, saying how many of each.
- Transcribe works on picked VODs. With several VODs picked in the library, pointing at Transcribe on one lights up all the picked ones it would transcribe, and a click asks for them all, transcribed one after another in the order you picked them.
- A file's library picture comes later. It's a frame an eighth of the way in, not a tenth, from the small frames VODscribe keeps of each video on this computer for the timeline.
- Settings opens as its own window. It has a section list instead of a dropdown, and in its Help section, Reset settings and Repair VODscribe sit apart under Reset and repair, marked in red as the other buttons that delete things are, each with a line on what it does.
- Every keyboard control can be changed. In Settings › Controls, give an action several keys, modifiers allowed, and reset one or all.
- Fixes:
  - A started selection survives a timeline click. A selection started with I (or O) stays when you click the timeline to move the playhead, so O (or I) there ends it where you clicked; Escape still lets it go.
  - Seeking in .ts files shows the right frame. In an MPEG-TS video file, moving the playhead or stepping a frame showed one from up to a few seconds later, the next keyframe, and stepping there stood still on it.
  - On a Mac, folder permission waits are explained. The first time you play a file from your Downloads, Desktop or Documents folder, it says so while macOS asks, rather than sitting black.
  - Media controls show Twitch titles again. The desktop's media controls show a Twitch VOD's own title and artwork again, as before 0.5.0; turning WebKit's Media Session off then, for a desktop freeze seen once on GNOME, didn't reduce what WebKit tells the desktop and left the controls a bare title.
  - Early seeks in an edit hold. Moving the playhead in an edit or a clip just opened in the editor, before it was ready to play, put it back a moment later, so a cut (B) missed where the time showed and Delete after it took the whole piece, not the stretch at the playhead.
  - A scrolling clip bar takes clicks again. On some Linux systems, with more clips than fit, a click on the lower half of a clip, its name or its buttons did nothing.
  - Edit timelines show site VOD pictures. Pointing at an edit's timeline shows the small picture over pieces of a VOD from a site once that part of its video is downloaded, not only over pieces of video files.
  - Stopping a transcription keeps its lines. The lines transcribed so far were thrown away.
  - A chat that stops partway is kept. When a chat stops downloading partway, as a long Twitch VOD's sometimes does with "service error", the chat column and the strip beside the transcript keep what came in, and the header says why, with Try again to fetch the rest from where it stopped; before, it disappeared and came back only while Try again ran.
  - Chat downloads retry a refused part. A part of the chat that Twitch or Kick refuses for a moment is asked for again, for about half a minute, before the download stops.
  - Twitch chats finish at the VOD's end. Their last messages can run a second or two past the video, and Twitch refused every request past the end with "service error", so the download stops where the video does.
  - The window shows promptly at start. It sometimes stayed empty for about two minutes when VODscribe started.

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
