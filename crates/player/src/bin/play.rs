//! Plays a file in a bare window, without the app: the player as the app
//! has it (decode, clock, present), to run and look at on its own.
//!
//!   vodscribe-play <file> [ffmpeg]
//!
//! ffmpeg is the one on PATH unless given. Space plays and pauses, the
//! arrows seek 5 s, J and L 10 s, comma and period step a frame, Home goes
//! to the start, Q quits. The title shows the time and frame.

#[cfg(target_os = "linux")]
fn main() -> anyhow::Result<()> {
    use std::path::PathBuf;
    use std::sync::Arc;

    use gtk::prelude::*;
    use vodscribe_player::clock::Player;
    use vodscribe_player::present::{Canvas, Place};

    let mut args = std::env::args_os().skip(1);
    let file = PathBuf::from(args.next().ok_or_else(|| anyhow::anyhow!("usage: vodscribe-play <file> [ffmpeg]"))?);
    let ffmpeg = args.next().map(PathBuf::from).unwrap_or_else(|| "ffmpeg".into());

    gtk::init()?;
    let player = Arc::new(Player::open(&ffmpeg, &file)?);
    let window = gtk::Window::new(gtk::WindowType::Toplevel);
    let picture = player.info().picture.unwrap();
    window.set_default_size(
        picture.width.min(1280) as i32,
        (picture.height * picture.width.min(1280) / picture.width) as i32,
    );
    let area = gtk::GLArea::new();
    let canvas = Canvas::new(&area, Place::Over);
    window.add(&area);

    // Each frame due is drawn, and the title brought up to date.
    let (shown, drawn) = (canvas.clone(), player.clone());
    let title = glib::SendWeakRef::from(window.downgrade());
    let name = file.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
    player.on_frame(Box::new(move |_| {
        shown.show(drawn.frame());
        let (time, fps, playing) = (drawn.time(), drawn.fps(), drawn.playing());
        let title = title.clone();
        let name = name.clone();
        glib::MainContext::default().invoke(move || {
            if let Some(window) = title.upgrade() {
                let frame = (time * fps + 1e-6).floor() as u64;
                let state = if playing { "" } else { " (paused)" };
                window.set_title(&format!("{name} - {time:.3} s, frame {frame}{state}"));
            }
        });
    }));

    let keys = player.clone();
    window.connect_key_press_event(move |_, key| {
        use gtk::gdk::keys::constants as k;
        let at = keys.time();
        match key.keyval() {
            k::space => keys.toggle(),
            k::Left => keys.seek(at - 5.0),
            k::Right => keys.seek(at + 5.0),
            k::j | k::J => keys.seek(at - 10.0),
            k::l | k::L => keys.seek(at + 10.0),
            k::comma => keys.step(-1),
            k::period => keys.step(1),
            k::Home => keys.seek(0.0),
            k::q | k::Q | k::Escape => gtk::main_quit(),
            _ => return glib::Propagation::Proceed,
        }
        glib::Propagation::Stop
    });
    window.connect_destroy(|_| gtk::main_quit());
    window.show_all();
    gtk::main();
    Ok(())
}

#[cfg(not(target_os = "linux"))]
fn main() {
    eprintln!("The player's window is built for Linux only so far.");
}
