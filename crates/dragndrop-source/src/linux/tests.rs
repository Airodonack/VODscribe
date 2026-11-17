use super::*;

use std::sync::Mutex;

/// Held by each test that reads or sets the environment (the desktop entries'
/// folders, the session's desktop): they share the process's.
static ENV: Mutex<()> = Mutex::new(());

/// Runs `f` with the variables set (None: unset), put back after.
fn with_env<T>(vars: &[(&str, Option<&str>)], f: impl FnOnce() -> T) -> T {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let before: Vec<_> = vars.iter().map(|(name, _)| (*name, std::env::var_os(name))).collect();
    let set = |name: &str, value: Option<&std::ffi::OsStr>| {
        // SAFETY: nobody else reads the environment while ENV is held.
        unsafe {
            match value {
                Some(value) => std::env::set_var(name, value),
                None => std::env::remove_var(name),
            }
        }
    };
    for (name, value) in vars {
        set(name, value.map(std::ffi::OsStr::new));
    }
    let result = f();
    for (name, value) in before {
        set(name, value.as_deref());
    }
    result
}

/// The receiving app is named as its desktop entry names it: the entry
/// naming its window's class, else the class.
#[test]
fn an_app_is_named_by_its_desktop_entry() {
    let _env = ENV.lock().unwrap_or_else(|e| e.into_inner());
    let entry = "[Desktop Entry]\nName=Kdenlive\nStartupWMClass=kdenlive\n[Desktop Action new]\nName=New\n";
    assert_eq!(desktop_entry(entry), (Some("Kdenlive".into()), Some("kdenlive")));
    assert_eq!(desktop_entry("[Desktop Entry]\nExec=x\n"), (None, None));
    assert_eq!(desktop_entry("[Desktop Action new]\nName=New\n"), (None, None), "another group's name");
    assert_eq!(app_name("org.example.NoSuchApp"), "NoSuchApp", "a reverse domain name's last part");
    assert_eq!(app_name("No-such-app"), "No-such-app");
}

/// The app asking is told by the program its process runs.
#[test]
fn an_apps_program_is_read_from_its_process() {
    assert_eq!(program_of(std::process::id()), std::env::current_exe().ok());
    assert_eq!(program_of(u32::MAX), None);
}

/// Qt's apps are answered at once, GTK 4's held, the others held only
/// once it's let go there; one not known is held, as GTK 4's.
#[test]
fn an_ask_is_held_by_the_toolkit_asking() {
    assert_eq!(toolkit_in("7f0 r-xp /usr/lib/libQt6Core.so.6\n"), Toolkit::Qt);
    assert_eq!(toolkit_in("7f0 r-xp /usr/lib/x86_64-linux-gnu/libgtk-4.so.1\n"), Toolkit::Gtk4);
    assert_eq!(toolkit_in("7f0 r-xp /usr/lib/libgtk-3.so.0\n"), Toolkit::Other);
    for dropped in [false, true] {
        assert!(!holds(Some(Toolkit::Qt), dropped));
        assert!(holds(Some(Toolkit::Gtk4), dropped));
        assert!(holds(None, dropped));
        assert_eq!(holds(Some(Toolkit::Other), dropped), dropped);
    }
}

/// An app is named as its desktop entry names it: the entry naming its
/// windows' class (StartupWMClass), else the entry named for the class
/// ("Files" for org.gnome.Nautilus), else the class (its last part, if it's a
/// reverse domain name) (`app_name`'s doc).
#[test]
fn an_app_is_named_by_the_entry_that_names_its_class_or_is_named_for_it() {
    let dir = std::env::temp_dir().join(format!("dragndrop-source-linux-entries-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    let apps = dir.join("share/applications");
    std::fs::create_dir_all(&apps).unwrap();
    std::fs::create_dir_all(dir.join("empty")).unwrap();
    let entry = |file: &str, text: &str| std::fs::write(apps.join(file), text).unwrap();
    entry("org.kde.dolphin.desktop", "[Desktop Entry]\nName=Dolphin\nStartupWMClass=dolphin\n");
    entry("org.gnome.Nautilus.desktop", "[Desktop Entry]\nName=Files\n");
    entry("kdenlive.desktop", "[Desktop Entry]\nName=Kdenlive Video Editor\n");
    entry("foo.desktop", "[Desktop Entry]\nName=Named for it\n");
    entry("bar.desktop", "[Desktop Entry]\nName=Names its class\nStartupWMClass=foo\n");
    entry("nameless.desktop", "[Desktop Entry]\nStartupWMClass=nameless\n");
    let share = dir.join("share");
    let named = with_env(&[("XDG_DATA_HOME", share.to_str()), ("XDG_DATA_DIRS", dir.join("empty").to_str())], || {
        ["Dolphin", "org.gnome.Nautilus", "nautilus", "kdenlive", "foo", "nameless", "org.example.NoSuchApp", "a.b"]
            .map(app_name)
    });
    assert_eq!(
        named,
        ["Dolphin", "Files", "Files", "Kdenlive Video Editor", "Names its class", "nameless", "NoSuchApp", "a.b"]
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// Where X11 doesn't name the window asking, the session is told by its
/// desktop (`holds_ask`: "unless the session is KDE's, whose apps are Qt's").
#[test]
fn a_session_is_kdes_by_its_desktop() {
    let kde = |desktop: Option<&str>| with_env(&[("XDG_CURRENT_DESKTOP", desktop)], kde_session);
    assert!(kde(Some("KDE")));
    assert!(kde(Some("ubuntu:KDE")), "one of several");
    assert!(kde(Some("kde")), "in any case");
    assert!(!kde(Some("GNOME")));
    assert!(!kde(Some("X-KDEish")), "a name that only has it in it");
    assert!(!kde(None));
}

/// The toolkit of an app is read from the process, and none where it can't
/// be (`toolkit_of`: "None if that can't be read").
#[test]
fn a_toolkit_of_a_process_that_cannot_be_read_is_none() {
    assert_eq!(toolkit_of(u32::MAX), None);
    assert!(toolkit_of(std::process::id()).is_some(), "this process's own map wasn't read");
}
