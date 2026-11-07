//! A GTK 3 window that takes what's dropped on it, on Linux: it prints what
//! each drag offers and gets, and moves the files a source saves into
//! folder `received` of the current one (or the first argument).
//!
//! cargo run -p dragndrop-target --example linux_receiver [folder]

#[cfg(target_os = "linux")]
fn main() {
    use std::cell::RefCell;
    use std::path::PathBuf;

    use dragndrop_target::linux::{self, DIRECT_SAVE, PORTAL, PORTAL_FILES, URI_LIST};
    use dragndrop_target::{Destination, Fetch};
    use gtk::gdk;
    use gtk::prelude::*;

    let into = std::env::args().nth(1).map(PathBuf::from).unwrap_or_else(|| "received".into());
    gtk::init().expect("GTK can't start");
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    window.set_title("dragndrop-target receiver");
    window.set_default_size(400, 300);
    window.add(&gtk::Label::new(Some("Drop files here")));
    // A drop site with no targets of GTK's choosing: the handlers decide.
    window.drag_dest_set(gtk::DestDefaults::empty(), &[], gdk::DragAction::COPY);
    let seen: RefCell<Option<gdk::DragContext>> = RefCell::default();
    window.connect_drag_motion(move |_, context, _, _, time| {
        if seen.borrow().as_ref() != Some(context) {
            println!("a drag offers: {}", linux::formats(context).join(", "));
            *seen.borrow_mut() = Some(context.clone());
        }
        let takes = [URI_LIST, PORTAL, PORTAL_FILES].iter().any(|t| linux::offers(context, t)) || linux::saves(context);
        context.drag_status(if takes { gdk::DragAction::COPY } else { gdk::DragAction::empty() }, time);
        true
    });
    let fetches: &'static RefCell<Vec<Fetch>> = Box::leak(Box::default());
    window.connect_drag_drop(move |_, context, _, _, time| {
        println!("let go");
        if linux::offers(context, DIRECT_SAVE) {
            println!("suggests: {:?}", linux::items(context).iter().map(|i| i.name().to_string()).collect::<Vec<_>>());
        }
        let into = into.clone();
        linux::receive(context, time, into.join(".staged"), move |received| {
            match received.failure() {
                Some(why) => println!("nothing: {why}"),
                None if received.offer().paths.is_empty() && received.offer().items.is_empty() => println!("nothing"),
                None => {}
            }
            for path in &received.offer().paths {
                println!("path: {}", path.display());
            }
            let requests = received
                .offer()
                .items
                .iter()
                .map(|item| {
                    let name = item.name().to_string();
                    let to = Destination::folder(&into, move |got| println!("item {name}: {got:?}"));
                    (item.clone(), to)
                })
                .collect();
            let mut kept = fetches.borrow_mut();
            kept.retain(|f| !f.is_finished());
            kept.push(linux::fetch(received, requests));
        });
        true
    });
    window.connect_destroy(|_| gtk::main_quit());
    window.show_all();
    gtk::main();
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("This example is for Linux.");
}
