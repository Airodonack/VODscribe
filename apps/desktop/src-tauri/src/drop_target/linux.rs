//! The window's drops on Linux (GTK 3), beside the web view's own. Drags with
//! file addresses (text/uri-list: a file manager's, the app's own) are left
//! to the web view, whose drops reach the page as Tauri's drag-drop events,
//! as ever. Taken here are the drags with none, whose files come another
//! way (`dragndrop_target::linux`):
//!
//! - from a sandboxed app, through the document portal: the files it gives
//!   are on disk, so they're told to the target as a drop with paths;
//! - from an archive manager that saves its files where it's told (XDS:
//!   File Roller 3, Engrampa; Ark's own way on KDE): saved into the target's
//!   stage folder once let go, then each one it takes received as a virtual
//!   file is on Windows. The fetches are kept here until they end.
//!
//! GTK sends such a drag's motion, leave and drop to the page's widget
//! (WebKit's), whose handlers would refuse it: these run first and stop it.

use std::cell::RefCell;
use std::rc::Rc;

use dragndrop_target::Fetch;
use dragndrop_target::linux::{self, PORTAL, PORTAL_FILES, URI_LIST};
use gtk::gdk;
use gtk::glib;
use gtk::prelude::*;

use super::{Event, Target};

/// Takes the drops on `page` (the web view's widget) that it can't, for
/// `target`; `trace` hears what's going on (the targets each drag offers,
/// each file received). Called on GTK's thread.
pub(crate) fn register(page: &gtk::Widget, target: Box<dyn Target>, trace: fn(&str)) {
    let drops = Rc::new(Drops {
        target,
        trace,
        seen: RefCell::default(),
        over: RefCell::new(Over::No),
        fetches: RefCell::default(),
    });
    let on = drops.clone();
    page.connect_drag_motion(move |_, context, x, y, time| on.motion(context, (f64::from(x), f64::from(y)), time));
    let on = drops.clone();
    page.connect_drag_leave(move |_, context, _| on.leave(context));
    page.connect_drag_drop(move |_, context, x, y, time| drops.drop(context, (f64::from(x), f64::from(y)), time));
}

/// A drag over the page that's taken here.
#[derive(Clone, PartialEq)]
enum Over {
    No,
    /// Over it: its files come through the portal (`portal`) or are saved.
    Taken {
        context: gdk::DragContext,
        portal: bool,
    },
    /// It left (or is about to be let go: GTK leaves first).
    Leaving {
        context: gdk::DragContext,
        portal: bool,
    },
}

struct Drops {
    target: Box<dyn Target>,
    trace: fn(&str),
    /// The last drag whose targets were traced.
    seen: RefCell<Option<gdk::DragContext>>,
    over: RefCell<Over>,
    /// The saved files let go on the page being received, kept until they
    /// end (dropped, they'd stop).
    fetches: RefCell<Vec<Fetch>>,
}

impl Drops {
    /// Over the page: taken here when its files have no addresses.
    fn motion(&self, context: &gdk::DragContext, at: (f64, f64), time: u32) -> bool {
        if self.seen.borrow().as_ref() != Some(context) {
            (self.trace)(&format!("a drag over the page offers {}", linux::formats(context).join(", ")));
            *self.seen.borrow_mut() = Some(context.clone());
        }
        let now = self.over.borrow().clone();
        match now {
            Over::Taken { context: ref c, .. } | Over::Leaving { context: ref c, .. } if c == context => {
                // Back over it after leaving (not let go after all).
                if let Over::Leaving { context, portal } = now {
                    *self.over.borrow_mut() = Over::Taken { context, portal };
                }
                self.target.heard(Event::Over { at });
            }
            _ => {
                if linux::offers(context, URI_LIST) {
                    return false;
                }
                let portal = [PORTAL, PORTAL_FILES].iter().any(|t| linux::offers(context, t));
                if !portal && !linux::saves(context) {
                    return false;
                }
                // The name the source suggests (an archive manager may give
                // any): one for the page to see a drag of files come.
                let names = linux::items(context).iter().map(|i| i.name().to_string()).collect::<Vec<_>>();
                let names = if names.is_empty() { vec!["file".into()] } else { names };
                self.target.heard(Event::Enter { paths: vec![], names, links: vec![], at });
                *self.over.borrow_mut() = Over::Taken { context: context.clone(), portal };
            }
        }
        context.drag_status(gdk::DragAction::COPY, time);
        true
    }

    /// Left, unless it's let go now (GTK leaves just before a drop).
    fn leave(self: &Rc<Self>, context: &gdk::DragContext) {
        let now = self.over.borrow().clone();
        let Over::Taken { context: c, portal } = now else { return };
        if c != *context {
            return;
        }
        *self.over.borrow_mut() = Over::Leaving { context: c, portal };
        let drops = self.clone();
        glib::idle_add_local_once(move || {
            if matches!(*drops.over.borrow(), Over::Leaving { .. }) {
                *drops.over.borrow_mut() = Over::No;
                drops.target.heard(Event::Leave);
            }
        });
    }

    /// Let go: its files asked for, then received.
    fn drop(self: &Rc<Self>, context: &gdk::DragContext, at: (f64, f64), time: u32) -> bool {
        let now = self.over.replace(Over::No);
        let (Over::Taken { context: c, portal } | Over::Leaving { context: c, portal }) = now else { return false };
        if c != *context {
            return false;
        }
        let Some(stage) = self.target.stage() else {
            context.drag_finish(false, false, time);
            self.target.heard(Event::Leave);
            return true;
        };
        // Saved files come in as jobs: the page hears a drop with none now.
        if !portal {
            self.target.heard(Event::Drop { paths: vec![], links: vec![], at });
        }
        let drops = self.clone();
        linux::receive(context, time, stage, move |received| {
            if let Some(why) = received.failure() {
                (drops.trace)(&format!("nothing received: {why}"));
                drops.target.nothing_given(why);
            }
            let paths = received.offer().paths.clone();
            if portal {
                (drops.trace)(&format!("let go with {} file(s) through the portal", paths.len()));
                drops.target.heard(Event::Drop { paths, links: vec![], at });
                return;
            }
            let items = received.offer().items.clone();
            (drops.trace)(&format!(
                "let go: {} file(s) saved for the app ({})",
                items.len(),
                items.iter().map(|i| i.name()).collect::<Vec<_>>().join(", ")
            ));
            let requests = drops.target.incoming(items.into_iter().filter(|f| drops.target.takes(f.name())).collect());
            let reading = linux::fetch(received, requests);
            let mut fetches = drops.fetches.borrow_mut();
            fetches.retain(|f| !f.is_finished());
            fetches.push(reading);
        });
        true
    }
}
