use super::*;

/// Resolve is told by its windows' class, or by its program where the
/// class doesn't say (`is_resolve`: "by its windows' class or the program its
/// process runs"; `Watch::refuses`: "Resolve, while this applies").
#[test]
fn resolve_is_told_by_its_window_or_its_process() {
    assert!(is_resolve(Some("resolve"), None));
    assert!(is_resolve(Some("Resolve"), Some(Path::new("/usr/bin/python3"))), "the test's stand-in");
    assert!(is_resolve(None, Some(Path::new(PROGRAM))));
    assert!(is_resolve(Some("unknown"), Some(Path::new("/opt/resolve/bin/resolve"))));
    assert!(!is_resolve(Some("org.gnome.Nautilus"), Some(Path::new("/usr/bin/nautilus"))));
    assert!(!is_resolve(Some("kdenlive"), None));
    assert!(!is_resolve(None, None));
    let watch = Watch::new(true);
    assert!(!watch.refuses(Some("dolphin"), None));
    assert!(watch.refuses(Some("resolve"), None));
    assert!(!Watch::new(false).refuses(Some("resolve"), None), "not watched: never refused");
}

/// Installed is its program where its installer puts it (`installed_at`:
/// "Whether Resolve's program is at `program`"; `PROGRAM`).
#[test]
fn resolve_is_installed_where_its_program_is() {
    let dir = std::env::temp_dir().join(format!("vodscribe-resolve-{}", std::process::id()));
    let program = dir.join("bin/resolve");
    assert!(!installed_at(&program));
    std::fs::create_dir_all(program.parent().unwrap()).unwrap();
    std::fs::write(&program, b"").unwrap();
    assert!(installed_at(&program));
    assert!(!installed_at(program.parent().unwrap()), "a folder of that name isn't it");
    std::fs::remove_dir_all(&dir).unwrap();
}

/// Watched while asked to and an MP4 (or M4A) is dragged, on Linux only
/// (`applies`: "on Linux, while `settings` say to ask, and one of them is MP4
/// (or its sound alone, M4A: AAC too)"; 0.7.2: "Dropping on DaVinci Resolve
/// offers MOV. On Linux, where Resolve can't open MP4 ...").
#[test]
fn a_drag_of_mp4_is_watched_while_asked_to() {
    let on = Settings::default();
    let off = Settings { suggest_mov_for_resolve: false, ..Settings::default() };
    let linux = cfg!(target_os = "linux");
    assert_eq!(applies(&on, &["a.mp4"]), linux);
    assert_eq!(applies(&on, &["a.mov", "b.M4A"]), linux);
    assert!(!applies(&on, &["a.mov", "b.wav"]));
    assert!(!applies(&off, &["a.mp4"]));
    assert!(!applies(&on, &[]));
}

/// Refused by Resolve: said, wherever it ended but called off or back in
/// the window. Let go on an app not named: said where Resolve is
/// installed. Else nothing (`heard`'s doc: "Resolve `refused`, or let go on
/// another app the system didn't name (`unnamed`) where Resolve is
/// `installed`").
#[test]
fn a_drop_on_resolve_is_said() {
    use Ending::*;
    let yes = || true;
    let no = || false;
    for ending in [Elsewhere, Nowhere] {
        assert_eq!(heard(true, true, false, ending, false, no), Some(ResolveDrop::Refused), "{ending:?}");
    }
    assert_eq!(heard(true, true, false, Elsewhere, true, no), None, "went over Resolve, given to another");
    assert_eq!(heard(true, true, false, CalledOff, false, yes), None);
    assert_eq!(heard(true, true, false, Back, false, yes), None);
    assert_eq!(heard(false, true, false, Elsewhere, false, yes), None, "not watched");
    assert_eq!(heard(true, false, true, Elsewhere, true, yes), Some(ResolveDrop::Maybe));
    assert_eq!(heard(true, false, true, Elsewhere, false, yes), Some(ResolveDrop::Maybe));
    assert_eq!(heard(true, false, true, Elsewhere, true, no), None, "not installed");
    assert_eq!(heard(true, false, true, Nowhere, false, yes), None, "dropped on nothing");
    assert_eq!(heard(true, false, false, Elsewhere, true, yes), None, "named, not Resolve");
}

/// A drag's watch refuses an app that is Resolve the files, and says so
/// once the drag is over (`Watch::refuses`, `Watch::heard`; 0.7.2: "a clip or
/// edit dropped on it is held back").
#[test]
fn a_watch_that_refused_resolve_the_files_says_so_when_the_drop_ends() {
    let watch = Watch::new(true);
    assert!(watch.applies());
    assert!(!watch.refuses(Some("dolphin"), None));
    assert_eq!(watch.heard(Ending::Nowhere, false), None, "nothing was refused yet");
    assert!(watch.refuses(Some("resolve"), None));
    assert_eq!(watch.heard(Ending::Nowhere, false), Some(ResolveDrop::Refused));
    assert_eq!(watch.heard(Ending::Elsewhere, false), Some(ResolveDrop::Refused));
    assert_eq!(watch.heard(Ending::CalledOff, false), None, "called off");
    assert_eq!(watch.heard(Ending::Back, false), None, "back on the window");
}

/// A drag not watched (not Linux, an MOV, or the note turned off) never
/// refuses or says anything of Resolve (`Watch::new`: "else one that never
/// refuses or asks").
#[test]
fn a_drag_not_watched_never_refuses_or_says_anything() {
    let watch = Watch::new(false);
    assert!(!watch.applies());
    assert!(!watch.refuses(Some("resolve"), None));
    watch.unnamed();
    for ending in [Ending::CalledOff, Ending::Back, Ending::Nowhere, Ending::Elsewhere] {
        assert_eq!(watch.heard(ending, false), None, "{ending:?}");
    }
}

/// What the page hears of a drag's end, as a person would from the
/// outside: one let go on `receiver`. (On Linux only: elsewhere a drag never
/// asks.)
#[cfg(target_os = "linux")]
mod on_a_drag {
    use std::path::Path;

    use super::PROGRAM;

    use crate::testkit::{LetGo, Receiver, Takes, TestApp};
    use serde_json::{Value, json};

    /// Settings as the page saves them: `change` applied to what's there.
    fn settings(app: &TestApp, change: impl FnOnce(&mut Value)) {
        let mut settings = app.invoke("get_settings", json!({})).unwrap();
        change(&mut settings);
        app.invoke("set_settings", json!({ "settings": settings })).unwrap();
    }

    /// A clip of a 60 second video, made, and dragged out and let go on
    /// the app named `on`; what the page hears of the end, and what that app
    /// was given.
    fn dragged_to(app: &TestApp, on: &str) -> (Value, Vec<std::path::PathBuf>) {
        let vod = app.add_video_file("Day 3.mp4", 60.0);
        let clip = app.make_clip(vod.id, "0:10", "0:20");
        app.drag_out(&[clip.id]).unwrap();
        let drag = app.drags().last();
        drag.let_go(LetGo::On(Receiver::taking_files(on)));
        (app.drag_ended(), drag.given())
    }

    /// An MP4 clip let go on Resolve is refused it, and the page hears that
    /// and nothing else of the drag: it isn't "failed", nor "finished only",
    /// and no app is named (`start_drag`: "Refused to Resolve: nothing else to
    /// say of it"; 0.7.2: "a clip or edit dropped on it is held back and
    /// VODscribe offers to switch to MOV").
    #[test]
    fn an_mp4_clip_let_go_on_resolve_is_held_back_and_the_page_hears_only_that() {
        let app = TestApp::new();
        let (ended, given) = dragged_to(&app, "resolve");
        assert!(given.is_empty(), "Resolve was given {given:?}");
        assert_eq!(
            ended,
            json!({ "drag": 1, "delivered": false, "failed": null, "finished_only": false, "receiver": null, "resolve": "refused" })
        );
    }

    /// An MP4 clip let go on another app is given it, and nothing asks.
    #[test]
    fn an_mp4_clip_let_go_on_another_app_is_given_and_nothing_asks() {
        let app = TestApp::new();
        let (ended, given) = dragged_to(&app, "kdenlive");
        assert_eq!((ended["delivered"].clone(), ended["resolve"].clone()), (json!(true), Value::Null), "{ended}");
        assert_eq!(given.len(), 1);
    }

    /// Switched to MOV, a clip is made so, which Resolve takes: it's given the
    /// file and nothing asks (drag-resolve.mjs: "Switch to MOV makes clips as
    /// MOV DNxHR and the pill again as MOV, which then lands in Resolve with
    /// nothing asked").
    #[test]
    fn a_mov_clip_let_go_on_resolve_is_given_and_nothing_asks() {
        let app = TestApp::new();
        settings(&app, |s| {
            s["clip_codec"] = json!("dnxhr");
            s["clip_format"] = json!("mov");
        });
        let (ended, given) = dragged_to(&app, "resolve");
        assert_eq!((ended["delivered"].clone(), ended["resolve"].clone()), (json!(true), Value::Null), "{ended}");
        assert_eq!(given.len(), 1);
        assert_eq!(given[0].extension().and_then(|e| e.to_str()), Some("mov"));
    }

    /// With "Don't ask again" ticked (the setting `suggest_mov_for_resolve`
    /// off), an MP4 clip let go on Resolve is given it, as to any app (0.7.2:
    /// "Notes all say Don't ask again ... the Resolve note ... matches their
    /// Ask switches in Settings").
    #[test]
    fn with_dont_ask_again_ticked_an_mp4_clip_let_go_on_resolve_is_given() {
        let app = TestApp::new();
        settings(&app, |s| s["suggest_mov_for_resolve"] = json!(false));
        let (ended, given) = dragged_to(&app, "resolve");
        assert_eq!((ended["delivered"].clone(), ended["resolve"].clone()), (json!(true), Value::Null), "{ended}");
        assert_eq!(given.len(), 1);
    }

    /// Where the system doesn't name the app a drag is let go on (Wayland), it
    /// is given the files as any app is, and the page is told it may have been
    /// Resolve only where Resolve is installed (`Watch::unnamed`: "a drop on
    /// any other app asks, if Resolve is installed, and nothing is
    /// refused"; resolve.rs: "where it doesn't (Wayland), a drop on any other
    /// app asks, if Resolve is installed, and nothing is refused").
    #[test]
    fn an_app_that_is_not_named_is_given_the_files_and_asks_only_where_resolve_is_installed() {
        let app = TestApp::new();
        let vod = app.add_video_file("Day 3.mp4", 60.0);
        let clip = app.make_clip(vod.id, "0:10", "0:20");
        app.drag_out(&[clip.id]).unwrap();
        let drag = app.drags().last();
        drag.let_go(LetGo::On(Receiver { name: None, takes: Takes::Files }));
        let ended = app.drag_ended();
        assert_eq!(ended["delivered"], json!(true), "{ended}");
        assert_eq!(drag.given().len(), 1);
        let installed = Path::new(PROGRAM).is_file();
        assert_eq!(ended["resolve"], if installed { json!("maybe") } else { Value::Null }, "{ended}");
    }

    /// An edit's MP4 video let go on Resolve is held back too (0.7.2: "a clip
    /// or edit dropped on it"; drag-resolve.mjs: "an edit's MP4 video dragged
    /// onto Resolve: refused").
    #[test]
    fn an_edit_s_mp4_video_let_go_on_resolve_is_held_back() {
        let app = TestApp::new();
        let vod = app.add_video_file("Day 3.mp4", 60.0);
        let edit = app.db().add_edit("Best of").unwrap().id;
        app.db().add_piece(edit, vod.id, (0.0, 30.0), "a", None).unwrap();
        app.invoke("render_edit_now", json!({ "id": edit })).unwrap();
        vodscribe_core::testkit::eventually("the edit rendered", || {
            let now = crate::edits::made_now(app.handle());
            app.db().edit(edit, now).unwrap().is_some_and(|e| e.status == vodscribe_core::db::ClipStatus::Ready)
        });
        app.invoke("drag_edit", json!({ "id": edit, "drag": 1 })).unwrap();
        let drag = app.drags().last();
        drag.let_go(LetGo::On(Receiver::taking_files("resolve")));
        let ended = app.drag_ended();
        assert_eq!(ended["resolve"], json!("refused"), "{ended}");
        assert!(drag.given().is_empty(), "Resolve was given {:?}", drag.given());
    }
}
