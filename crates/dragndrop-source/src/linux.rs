//! Linux (GTK 3): the data of a drag of files out of a widget, made or not.
//!
//! You start the drag (`gtk_drag_begin_with_coordinates`) with
//! [`Drag::targets`], and pass on its signals, on GTK's thread:
//! `drag-data-get` to [`Drag::data_get`], the context's `drop-performed` to
//! [`Drag::drop_performed`], `drag-failed` to [`Drag::failed`] and
//! `drag-end` to [`Drag::end`]; and say when the drag is over a window of
//! your own ([`Drag::over_own`]), which X11 tells by itself and Wayland
//! doesn't. Where X11 names the app asking ([`names_receivers`]), you may
//! have some apps given none of the files ([`Drag::refuse`]): one known to
//! misread them, say.
//!
//! Each file is offered two ways, both answered as they're asked for, so the
//! app it's let go on chooses:
//!
//! - its address (text/uri-list), which any app that takes files reads. An
//!   app asks for it as the drag goes over it (a file manager) or once it's
//!   let go there. Answered at once when all are made, and always to your
//!   own window (with those made so far: it needs no files, and its ask
//!   comes inside its own handling of the drag). Otherwise the answer waits
//!   for them (the hold, below), unless the app asking gives up on an answer
//!   that waits, or asks again at the drop and isn't there yet
//!   ([`holds_ask`]): it's answered at once with none.
//! - KDE's Ark extraction (`application/x-kde-ark-dndextract-service` and
//!   `-path`), which Dolphin takes: it asks a D-Bus object of yours to put
//!   the files in its folder, its drop done at once, and each is copied
//!   there as it's made. Offered in every drag where there's a session bus:
//!   apps that don't know it never ask for it.
//!
//! # The hold
//!
//! An answer that waits runs GTK's main loop inside the `drag-data-get`
//! call until the files are all made, one won't be, or the drag is over. It
//! has to be a loop nested inside the call: GTK sends the selection the
//! moment the handler returns, so the files must be there by then, and the
//! receiver (a file manager) waits for that answer as long as it takes,
//! while your window goes on drawing and handling input from inside the
//! loop. GTK 3 has no answer given later: returning with none is answering
//! none.
//!
//! The drag's end comes only after the hold ends (GTK waits for the answer),
//! so [`Event::Ended`] comes once the files went or didn't; for Dolphin,
//! once its copies are written. While a hold runs, begin no other drag
//! ([`holding`]): it would run inside it.
//!
//! What runs meanwhile is what the main loop dispatches. A toolkit that
//! takes its own work only between turns of its own event loop (Tauri's,
//! tao's) waits for the hold's end: tell the window what it must hear from
//! inside the loop (a GLib source), not through that event loop.

use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gtk::glib;
use gtk::prelude::*;

use crate::{Ending, Event, Failure, Item, Outcome, State, settled, trace};

/// How often a hold looks at the files.
pub(crate) const TICK: Duration = Duration::from_millis(100);

thread_local! {
    /// Asks for a drag's files being held, one inside another.
    static HOLDING: Cell<u32> = const { Cell::new(0) };
}

/// An ask for a drag's files is being held on this thread (GTK's): a drag
/// begun now would run inside it, so don't begin one.
pub fn holding() -> bool {
    HOLDING.with(Cell::get) > 0
}

/// Whether the first mouse button is down on `window`'s display (None if it
/// has no pointer to ask). A drag begun with no button held never ends:
/// nothing releases it. See [`crate::ReleasedEarly`].
pub fn button_held(window: &gtk::gdk::Window) -> Option<bool> {
    let pointer = window.display().default_seat()?.pointer()?;
    let (_, _, _, state) = window.device_position(&pointer);
    Some(state.contains(gtk::gdk::ModifierType::BUTTON1_MASK))
}

/// One drag's files and what's known of it as it goes. Cloned handles are
/// the same drag. GTK's thread only.
#[derive(Clone)]
pub struct Drag(Rc<Inner>);

struct Inner {
    items: Arc<Vec<Box<dyn Item>>>,
    on: Arc<dyn Fn(Event) + Send + Sync>,
    ark: Option<Arc<crate::ark::Offer>>,
    /// Asks for the files so far, for the trace.
    asks: Cell<u32>,
    /// Another app was given all the files' addresses.
    given: Cell<bool>,
    /// Called off, or dropped where nothing took it.
    failed: Cell<bool>,
    /// Called off (Escape).
    called_off: Cell<bool>,
    /// Over (GTK's drag-end), its end put off while an answer waits.
    ended: Cell<bool>,
    /// Answers waiting (the hold), one inside another.
    holding: Cell<u32>,
    /// Why the files never came, if one couldn't be made.
    failure: RefCell<Option<String>>,
    /// The drag is over your own window.
    over_own: Cell<bool>,
    /// The class of the other app's window it was let go on, where X11 says.
    receiver: RefCell<Option<String>>,
    /// The files were all made as it ended.
    made: Cell<bool>,
    /// Which other apps asking are given none of the files (`Drag::refuse`).
    refuse: RefCell<Option<Refuse>>,
    /// Let go (drop-performed), anywhere.
    dropped: Cell<bool>,
}

/// Says of another app asking for a drag's files whether it gets none.
type Refuse = Box<dyn Fn(&Asker) -> bool>;

/// The `info` of the targets, as `drag-data-get` hears it.
const URIS: u32 = 0;

impl Drag {
    /// A drag of `items`, in their order; `on` hears how it goes. Puts Ark's
    /// object on the session bus, if there is one, until the drag is over
    /// ([`Drag::end`], or [`Drag::withdraw`] if it never began).
    pub fn new(items: Vec<Box<dyn Item>>, on: impl Fn(Event) + Send + Sync + 'static) -> Drag {
        let items = Arc::new(items);
        let ark = crate::ark::Offer::new(items.clone());
        Drag(Rc::new(Inner {
            items,
            on: Arc::new(on),
            ark,
            asks: Cell::new(0),
            given: Cell::new(false),
            failed: Cell::new(false),
            called_off: Cell::new(false),
            ended: Cell::new(false),
            holding: Cell::new(0),
            failure: RefCell::new(None),
            over_own: Cell::new(false),
            receiver: RefCell::new(None),
            made: Cell::new(false),
            dropped: Cell::new(false),
            refuse: RefCell::new(None),
        }))
    }

    /// What the drag offers, for `gtk_drag_begin_with_coordinates`: the
    /// files' addresses (`info` 0), and Ark's two targets (1 and 2), for
    /// other apps only.
    pub fn targets(&self) -> gtk::TargetList {
        let targets = gtk::TargetList::new(&[]);
        targets.add_uri_targets(URIS);
        if self.0.ark.is_some() {
            let other = gtk::TargetFlags::OTHER_APP.bits();
            targets.add(&gtk::gdk::Atom::intern(crate::ark::SERVICE_TARGET), other, crate::ark::SERVICE);
            targets.add(&gtk::gdk::Atom::intern(crate::ark::OBJECT_TARGET), other, crate::ark::OBJECT);
        }
        targets
    }

    /// Answers an ask for the drag's data: call it from your source
    /// widget's `drag-data-get`, with what it was given. It may hold the
    /// ask until the files are made (the module's "The hold").
    pub fn data_get(&self, context: &gtk::gdk::DragContext, data: &gtk::SelectionData, info: u32) {
        let drag = &self.0;
        drag.asks.set(drag.asks.get() + 1);
        let n = drag.asks.get();
        if drag.failed.get() || drag.ended.get() {
            return;
        }
        if self.refuses(context) {
            trace(&format!("ask {n} ({}) refused: an app told to get none", data.target().name()));
            if info == URIS {
                answer(data, &[]);
            }
            return;
        }
        if let Some(ark) = drag.ark.as_ref().filter(|_| info == crate::ark::SERVICE || info == crate::ark::OBJECT) {
            let named = if info == crate::ark::SERVICE { &ark.service } else { &ark.path };
            data.set(&data.target(), 8, named.as_bytes());
            return;
        }
        // Your own window: what's made so far, at once.
        if drag.over_own.get() || asked_by_own_window(context) {
            let ready: Vec<_> =
                drag.states().filter_map(|s| if let State::Ready(p) = s { Some(p) } else { None }).collect();
            trace(&format!(
                "ask {n} ({}) by the app's own window: answered with the {} made",
                data.target().name(),
                ready.len()
            ));
            answer(data, &ready);
            return;
        }
        let mut now = settled(drag.states());
        if now.is_none() && !holds_ask(context, drag.dropped.get()) {
            trace(&format!("ask {n} ({}) answered at once with none: not one to hold", data.target().name()));
            answer(data, &[]);
            return;
        }
        if now.is_none() {
            trace(&format!("ask {n} ({}): held until the files are made", data.target().name()));
            // Asked once let go there (GTK 3's): it waits for them.
            if drag.dropped.get() {
                drag.waited();
            }
            self.hold();
            now = if drag.failed.get() || drag.ended.get() { None } else { settled(drag.states()) };
        }
        match now {
            Some(Ok(paths)) => {
                trace(&format!("ask {n} ({}): answered with {} files", data.target().name(), paths.len()));
                answer(data, &paths);
                drag.given.set(true);
            }
            Some(Err(why)) => {
                trace(&format!("ask {n}: not answered, a file couldn't be made: {why}"));
                *drag.failure.borrow_mut() = Some(why);
            }
            None => trace(&format!("ask {n}: not answered, the drag is over")),
        }
    }

    /// Has every other app asking for the files that `refuse` says yes to
    /// answered with none, as it asks, by what X11 says of its window
    /// ([`Asker`]). Where X11 doesn't name the app asking (Wayland:
    /// [`names_receivers`]), it's never asked. Call it before the drag
    /// begins.
    pub fn refuse(&self, refuse: impl Fn(&Asker) -> bool + 'static) {
        *self.0.refuse.borrow_mut() = Some(Box::new(refuse));
    }

    /// Whether the app asking (`context`'s destination) is one to refuse.
    fn refuses(&self, context: &gtk::gdk::DragContext) -> bool {
        let refuse = self.0.refuse.borrow();
        let (Some(refuse), Some(window)) = (refuse.as_ref(), other_window(context)) else { return false };
        let asker = Asker { class: window_class(&window), program: window_pid(&window).and_then(program_of) };
        refuse(&asker)
    }

    /// Waits, running GTK's main loop, until the files are all made or one
    /// won't be, or the drag is over.
    fn hold(&self) {
        let drag = &self.0;
        HOLDING.with(|h| h.set(h.get() + 1));
        drag.holding.set(drag.holding.get() + 1);
        let main = glib::MainLoop::new(None, false);
        let (quit, watched) = (main.clone(), self.0.clone());
        // (The tick ends itself as it ends the wait.)
        glib::timeout_add_local(TICK, move || {
            if watched.failed.get() || watched.ended.get() || settled(watched.states()).is_some() {
                quit.quit();
                return glib::ControlFlow::Break;
            }
            glib::ControlFlow::Continue
        });
        main.run();
        drag.holding.set(drag.holding.get() - 1);
        HOLDING.with(|h| h.set(h.get() - 1));
    }

    /// The drag is over a window of yours (its `drag-motion` and
    /// `drag-drop`: GTK leaves a widget just before a drop on it, so say so
    /// again at the drop), or has left it (`drag-leave`). An ask then is your
    /// own: X11 tells by the window asking; Wayland doesn't.
    pub fn over_own(&self, over: bool) {
        self.0.over_own.set(over);
    }

    /// It was let go: call it from the drag context's `drop-performed`
    /// (connected in your `drag-begin`). Let go on another app, `on` hears
    /// [`Event::LetGo`]; one whose ask is held already (asked as the drag
    /// went over it, Nautilus 43's) waits for the files.
    pub fn drop_performed(&self, context: &gtk::gdk::DragContext) {
        let drag = &self.0;
        drag.dropped.set(true);
        if !drag.over_own.get() && !asked_by_own_window(context) {
            (drag.on)(Event::LetGo);
            if drag.holding.get() > 0 {
                drag.waited();
            }
        }
    }

    /// It failed: dropped where nothing took it, or called off. Call it from
    /// `drag-failed`.
    pub fn failed(&self, result: gtk::DragResult) {
        trace(&format!("failed: {result:?}"));
        self.0.failed.set(true);
        self.0.called_off.set(result == gtk::DragResult::UserCancelled);
    }

    /// It's over: call it from `drag-end`. `then` runs on GTK's thread once
    /// no ask is held any more (at once, unless it was called off or given
    /// up on during a hold): disconnect your handlers there. `on` hears
    /// [`Event::Ended`] after that, from a thread of its own; for Dolphin,
    /// once its copies are written.
    pub fn end(&self, context: &gtk::gdk::DragContext, then: impl FnOnce() + 'static) {
        let drag = &self.0;
        drag.ended.set(true);
        // The app it was let go on, while the drag still says; and whether
        // the files were made by then (taken, or not, as they were).
        if !drag.failed.get() && !drag.over_own.get() {
            *drag.receiver.borrow_mut() = other_window(context).and_then(|w| window_class(&w));
            drag.made.set(settled(drag.states()).is_some_and(|r| r.is_ok()));
        }
        if drag.holding.get() == 0 {
            self.finish();
            then();
            return;
        }
        // Over while an answer waits (called off during it, or the app it was
        // let go on gave up): finished once that wait is, outside it.
        let (me, then) = (self.clone(), Cell::new(Some(then)));
        glib::timeout_add_local(TICK, move || {
            if me.0.holding.get() > 0 {
                return glib::ControlFlow::Continue;
            }
            me.finish();
            if let Some(then) = then.take() {
                then();
            }
            glib::ControlFlow::Break
        });
    }

    /// Tells `on` how it went, from a thread of its own.
    fn finish(&self) {
        let drag = &self.0;
        let ending = match () {
            _ if drag.called_off.get() => Ending::CalledOff,
            _ if drag.over_own.get() => Ending::Back,
            _ if drag.failed.get() => Ending::Nowhere,
            _ => Ending::Elsewhere,
        };
        // Dolphin asked for them (Ark's): told once its copies are written.
        // They're its own copies.
        let copying = drag.ark.as_ref().is_some_and(|ark| {
            let on = drag.on.clone();
            ark.ended(move |failed| {
                trace(&format!("over: copied for Dolphin, failed {failed:?}"));
                let outcome = Outcome {
                    ending: Ending::Elsewhere,
                    delivered: failed.is_none(),
                    copied: true,
                    failure: failed.map(Failure::Failed),
                    made: true,
                    receiver: None,
                };
                // (Perhaps on GTK's thread, if they were written already.)
                std::thread::spawn(move || on(Event::Ended(outcome)));
            })
        });
        if copying {
            return;
        }
        let (given, failure) = (drag.given.get(), drag.failure.borrow_mut().take());
        let (receiver, made) = (drag.receiver.borrow_mut().take(), drag.made.get());
        let on = drag.on.clone();
        // Off GTK's thread: the receiver's name is read from the desktop
        // entries on disk.
        std::thread::spawn(move || {
            let outcome = Outcome {
                ending,
                delivered: given,
                copied: false,
                failure: failure.map(Failure::Failed),
                made,
                receiver: receiver.as_deref().map(app_name),
            };
            trace(&format!("over: {outcome:?}"));
            on(Event::Ended(outcome));
        });
    }

    /// It never began (`gtk_drag_begin` refused, or the button was up):
    /// Ark's object comes off the bus, and `on` hears nothing.
    pub fn withdraw(&self) {
        if let Some(ark) = &self.0.ark {
            ark.ended(|_| {});
        }
    }
}

impl Inner {
    fn states(&self) -> impl Iterator<Item = State> + '_ {
        self.items.iter().map(|i| i.state())
    }

    /// The app it was let go on waits for those still being made.
    fn waited(&self) {
        waited(&self.items);
    }
}

/// Each of `items` still being made is told it's waited for.
pub(crate) fn waited(items: &[Box<dyn Item>]) {
    for item in items {
        if matches!(item.state(), State::Making(_)) {
            item.waited_for();
        }
    }
}

/// `paths` as the answer: one address each, escaped, so names with spaces
/// and other characters arrive whole.
fn answer(data: &gtk::SelectionData, paths: &[PathBuf]) {
    let uris: Vec<_> = paths.iter().filter_map(|p| glib::filename_to_uri(p, None).ok()).collect();
    data.set_uris(&uris.iter().map(|u| u.as_str()).collect::<Vec<_>>());
}

/// Another app asking for a drag's files, as X11 tells of its window.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Asker {
    /// Its window's class (WM_CLASS's second part: "dolphin", "Resolve").
    pub class: Option<String>,
    /// The program its process runs, if it can be read (not another user's).
    pub program: Option<PathBuf>,
}

/// Whether the apps a drag on `display` goes to are named as they ask for
/// its files and as it ends: on X11, by their windows; on Wayland, never.
pub fn names_receivers(display: &gtk::gdk::Display) -> bool {
    display.type_().name().starts_with("GdkX11")
}

/// The window a drag is over, asking for its data or let go on (`context`'s
/// destination), if it's another app's: X11 says; Wayland doesn't.
fn other_window(context: &gtk::gdk::DragContext) -> Option<gtk::gdk::Window> {
    use gtk::glib::translate::{FromGlibPtrNone, ToGlibPtr};
    // SAFETY: the context is alive for the call; the window it gives may be
    // null (none known), checked before it's taken.
    let window = unsafe {
        let window = gtk::gdk::ffi::gdk_drag_context_get_dest_window(context.to_glib_none().0);
        if window.is_null() {
            return None;
        }
        gtk::gdk::Window::from_glib_none(window)
    };
    (window.window_type() == gtk::gdk::WindowType::Foreign).then_some(window)
}

/// Whether the window asking a drag for its data (`context`'s destination)
/// is one of this process's own (X11 says; elsewhere there may be none).
fn asked_by_own_window(context: &gtk::gdk::DragContext) -> bool {
    use gtk::glib::translate::ToGlibPtr;
    // SAFETY: the context is alive for the call; the window it gives may be
    // null (none known), checked before its type is read.
    unsafe {
        let window = gtk::gdk::ffi::gdk_drag_context_get_dest_window(context.to_glib_none().0);
        !window.is_null() && gtk::gdk::ffi::gdk_window_get_window_type(window) != gtk::gdk::ffi::GDK_WINDOW_FOREIGN
    }
}

/// Whether an ask for a drag's files (from `context`'s destination, before
/// the drop if not `dropped`) waits for them, while they're still being made:
/// held until they are, or answered at once with none. Apps ask in three
/// ways. Nautilus 43 and later (GTK 4) asks as the drag goes over it and
/// keeps that answer for the drop, asking no more: that ask is held. Qt's
/// apps give up on an answer that waits (after 5 s on X11, 1 on Wayland), and
/// Dolphin, which asks as the drag goes over it, then refuses the drop (so
/// never takes Ark's extraction): never held. The others (GTK 3's, as
/// Nautilus 42, which asks at each move and takes the drop only once
/// answered) ask again once it's let go there: held only then. Where X11
/// names the window asking, by the toolkit its process runs; where it
/// doesn't (Wayland), as Nautilus 43 is answered, unless the session is
/// KDE's, whose apps are Qt's.
fn holds_ask(context: &gtk::gdk::DragContext, dropped: bool) -> bool {
    let toolkit = match other_window(context) {
        Some(window) => window_pid(&window).and_then(toolkit_of),
        None if kde_session() => Some(Toolkit::Qt),
        None => Some(Toolkit::Gtk4),
    };
    holds(toolkit, dropped)
}

/// Whether an ask from an app made with `toolkit` (None: not known) is held,
/// before the drop if not `dropped` (`holds_ask`).
fn holds(toolkit: Option<Toolkit>, dropped: bool) -> bool {
    match toolkit {
        Some(Toolkit::Qt) => false,
        Some(Toolkit::Gtk4) | None => true,
        Some(Toolkit::Other) => dropped,
    }
}

/// The toolkit an app asking for a drag's files is made with, as it asks
/// (`holds_ask`).
#[derive(Clone, Copy, PartialEq, Debug)]
enum Toolkit {
    Qt,
    Gtk4,
    Other,
}

/// Whether this is a KDE session.
fn kde_session() -> bool {
    std::env::var("XDG_CURRENT_DESKTOP").is_ok_and(|d| d.split(':').any(|p| p.eq_ignore_ascii_case("kde")))
}

/// The process another app's window `window` is of, as it says (_NET_WM_PID).
fn window_pid(window: &gtk::gdk::Window) -> Option<u32> {
    let atom = gtk::gdk::Atom::intern;
    let (_, _, bytes) = gtk::gdk::property_get(window, &atom("_NET_WM_PID"), &atom("CARDINAL"), 0, 1, 0)?;
    // A 32-bit property comes as the machine's longs.
    match bytes.len() {
        8.. => u64::from_ne_bytes(bytes[..8].try_into().ok()?).try_into().ok(),
        4.. => Some(u32::from_ne_bytes(bytes[..4].try_into().ok()?)),
        _ => None,
    }
}

/// The program process `pid` runs; None if that can't be read.
fn program_of(pid: u32) -> Option<PathBuf> {
    std::fs::read_link(format!("/proc/{pid}/exe")).ok()
}

/// The toolkit process `pid` runs, by the libraries it has loaded; None if
/// that can't be read (another user's, or in a sandbox of its own).
fn toolkit_of(pid: u32) -> Option<Toolkit> {
    let maps = std::fs::read_to_string(format!("/proc/{pid}/maps")).ok()?;
    Some(toolkit_in(&maps))
}

/// The toolkit a process's memory map `maps` shows it runs.
fn toolkit_in(maps: &str) -> Toolkit {
    if maps.contains("/libQt") {
        Toolkit::Qt
    } else if maps.contains("/libgtk-4") {
        Toolkit::Gtk4
    } else {
        Toolkit::Other
    }
}

/// Another app's window `window`'s class (WM_CLASS's second part: "dolphin",
/// "org.gnome.Nautilus").
fn window_class(window: &gtk::gdk::Window) -> Option<String> {
    let atom = gtk::gdk::Atom::intern;
    let (_, _, bytes) = gtk::gdk::property_get(window, &atom("WM_CLASS"), &atom("STRING"), 0, 1024, 0)?;
    let class = bytes.split(|&b| b == 0).nth(1).filter(|c| !c.is_empty())?;
    Some(String::from_utf8_lossy(class).into_owned())
}

/// The name an app whose windows are of class `class` goes by: its desktop
/// entry's (the one naming that class, or named for it: "Files" for
/// org.gnome.Nautilus), else the class (its last part, if it's a reverse
/// domain name).
fn app_name(class: &str) -> String {
    let home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".local/share")));
    let dirs = std::env::var("XDG_DATA_DIRS").unwrap_or_else(|_| "/usr/local/share:/usr/share".into());
    let dirs = home.into_iter().chain(dirs.split(':').filter(|d| !d.is_empty()).map(PathBuf::from));
    let entries = dirs.flat_map(|d| std::fs::read_dir(d.join("applications")).into_iter().flatten().flatten());
    let mut named = None;
    for entry in entries {
        let path = entry.path();
        let Some(stem) =
            path.file_stem().and_then(|s| s.to_str()).filter(|_| path.extension().is_some_and(|e| e == "desktop"))
        else {
            continue;
        };
        let Ok(text) = std::fs::read_to_string(&path) else { continue };
        let (name, wm_class) = desktop_entry(&text);
        let Some(name) = name else { continue };
        if wm_class.is_some_and(|c| c.eq_ignore_ascii_case(class)) {
            return name;
        }
        let stem = stem.to_ascii_lowercase();
        let class = class.to_ascii_lowercase();
        if named.is_none() && (stem == class || stem.ends_with(&format!(".{class}"))) {
            named = Some(name);
        }
    }
    named.unwrap_or_else(|| match class.matches('.').count() {
        2.. => class.rsplit('.').next().unwrap_or(class).to_string(),
        _ => class.to_string(),
    })
}

/// A desktop entry's name and the window class it says its windows have (its
/// [Desktop Entry] group's Name and StartupWMClass).
fn desktop_entry(text: &str) -> (Option<String>, Option<&str>) {
    let (mut name, mut class) = (None, None);
    let mut main = false;
    for line in text.lines().map(str::trim) {
        if line.starts_with('[') {
            main = line == "[Desktop Entry]";
        } else if !main {
            continue;
        } else if let Some(v) = line.strip_prefix("Name=") {
            name = Some(v.trim().to_string());
        } else if let Some(v) = line.strip_prefix("StartupWMClass=") {
            class = Some(v.trim());
        }
    }
    (name.filter(|n| !n.is_empty()), class)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The receiving app is named as its desktop entry names it: the entry
    /// naming its window's class, else the class.
    #[test]
    fn an_app_is_named_by_its_desktop_entry() {
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
}
