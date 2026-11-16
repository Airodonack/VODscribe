//! The specs of the timeline's layers at their seam: `load` and `save` on
//! `layers.json`, and `defaults`. The files are the ones each version wrote.

use super::*;

use crate::TempDir;

/// The timeline a new library starts with.
fn fresh() -> Timeline {
    Timeline { layers: defaults(), events: Vec::new(), marks: MarkSources::default() }
}

/// A timeline of just `layers`, with no events and both kinds of marks shown.
fn of(layers: Vec<Layer>) -> Timeline {
    Timeline { layers, events: Vec::new(), marks: MarkSources::default() }
}

/// 0.2.0: "Timeline layers mark the moments you care about: laughter, hype
/// and "clip it" to start, or any words and emotes".
#[test]
fn a_new_library_starts_with_laughter_hype_and_clip_it_layers() {
    let names: Vec<_> = defaults().into_iter().map(|l| l.name).collect();
    assert_eq!(names, ["Laughter", "Hype", "Clip it"]);
    for layer in defaults() {
        assert!(!layer.terms.is_empty(), "{} has no words", layer.name);
        assert_eq!(layer.channel, None, "{} is for one channel", layer.name);
    }
}

/// `load`: "The saved timeline, or the default layers (and no events) if
/// there's none yet".
#[test]
fn with_nothing_saved_the_starting_layers_show_and_no_events() {
    let dir = TempDir::new("layers");
    assert_eq!(load(&dir.join("layers.json")), fresh());
}

/// 0.2.0: "any words and emotes, for every channel or one"; `Term::emote`:
/// "The emote's image, when the term was added as one".
#[test]
fn a_layer_keeps_its_words_emotes_and_channel() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    let mut mine = fresh();
    mine.layers[0].channel = Some(Channel { platform: "kick".into(), channel: "Barky".into() });
    let emote = Some("https://files.kick.com/emotes/1/fullsize".into());
    mine.layers[0].terms.push(Term { text: "barkylol".into(), emote });
    mine.layers[1].visible = true;
    mine.layers[2].name = "Renamed".into();
    save(&path, &mine).unwrap();
    assert_eq!(load(&path), mine);
}

/// The chat events drawn (`Timeline::events`: subs, gifts, tips) are saved
/// with the layers.
#[test]
fn the_chat_events_drawn_are_kept() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    let mut mine = fresh();
    mine.events = vec![Kind::Sub, Kind::Gift, Kind::Tip];
    save(&path, &mine).unwrap();
    assert_eq!(load(&path).events, [Kind::Sub, Kind::Gift, Kind::Tip]);
    mine.events = Vec::new();
    save(&path, &mine).unwrap();
    assert!(load(&path).events.is_empty(), "turning every event off sticks");
}

/// `MarkSources`: "Whose chapters and highlights are shown: the user's own,
/// those an annotations import brought. Both, at first."
#[test]
fn whose_marks_are_shown_is_kept_and_both_at_first() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    assert_eq!(load(&path).marks, MarkSources { user: true, imported: true });
    let mut mine = fresh();
    for marks in [
        MarkSources { user: false, imported: true },
        MarkSources { user: true, imported: false },
        MarkSources { user: false, imported: false },
    ] {
        mine.marks = marks;
        save(&path, &mine).unwrap();
        assert_eq!(load(&path).marks, marks);
    }
}

/// `MarkSources` (`#[serde(default)]`): a kind the file says nothing of is
/// shown, as at first.
#[test]
fn a_kind_of_marks_the_file_says_nothing_of_is_shown() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    fs::write(&path, r#"{"layers": [], "marks": {"user": false}}"#).unwrap();
    assert_eq!(load(&path).marks, MarkSources { user: false, imported: true });
    fs::write(&path, r#"{"layers": [], "marks": {"imported": false}}"#).unwrap();
    assert_eq!(load(&path).marks, MarkSources { user: true, imported: false });
}

/// 0.6.0's file had layers and events and no word of marks: both are shown
/// (`load`: "Reads the plain list of layers the file held before event
/// toggles too"; `Timeline::marks`).
#[test]
fn a_file_from_before_marks_could_be_hidden_shows_both() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    fs::write(&path, r#"{"layers": [], "events": ["sub"]}"#).unwrap();
    let timeline = load(&path);
    assert_eq!(timeline.events, [Kind::Sub]);
    assert_eq!(timeline.marks, MarkSources { user: true, imported: true });
}

/// `load`: "Reads the plain list of layers the file held before event
/// toggles too."
#[test]
fn a_file_that_was_just_the_list_of_layers_is_read() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    fs::write(&path, serde_json::to_string(&defaults()[..1]).unwrap()).unwrap();
    assert_eq!(load(&path), of(defaults()[..1].to_vec()));
}

/// 0.3.0 Fixes: "Timeline layers that failed to load could be overwritten."
/// `load`'s answer for a file it can't read is the starting layers; the file
/// itself is left as it is, until a save replaces it.
#[test]
fn a_file_that_cant_be_read_gives_the_starting_layers_and_is_left_alone() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    fs::write(&path, "{").unwrap();
    assert_eq!(load(&path), fresh());
    assert_eq!(fs::read_to_string(&path).unwrap(), "{", "reading changed the file");
}

/// 0.2.0: layers can be edited and deleted (`defaults`: "to edit or
/// delete"): deleting all of them is kept, and doesn't bring the starting
/// ones back.
#[test]
fn deleting_every_layer_sticks() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    save(&path, &fresh()).unwrap();
    save(&path, &of(Vec::new())).unwrap();
    assert!(load(&path).layers.is_empty());
}

/// A save replaces the whole file (`save`: written beside it, then moved
/// over), leaving nothing else in the folder.
#[test]
fn a_save_leaves_only_the_file() {
    let dir = TempDir::new("layers");
    let path = dir.join("layers.json");
    save(&path, &fresh()).unwrap();
    let mut mine = fresh();
    mine.layers.truncate(1);
    save(&path, &mine).unwrap();
    let files: Vec<_> = fs::read_dir(&*dir).unwrap().map(|e| e.unwrap().file_name()).collect();
    assert_eq!(files, ["layers.json"]);
}

/// `save` in a folder that isn't there says so rather than saying it saved.
#[test]
fn a_save_that_cant_be_made_says_so() {
    let dir = TempDir::new("layers");
    assert!(save(&dir.join("missing").join("layers.json"), &fresh()).is_err());
}
