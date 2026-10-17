//! The native surface the picture is drawn on, over the page's box in the
//! app's window, which the page reports (Screen.svelte). On Linux it's a
//! Wayland subsurface or a child X window drawn with the compositor
//! (present_linux.rs); on Windows a child window drawn with Direct3D or GDI
//! (present_windows.rs); on macOS a view over the page whose layer shows
//! each frame as an image (present_macos.rs).

#[cfg(target_os = "linux")]
#[path = "present_linux.rs"]
mod linux;
#[cfg(target_os = "linux")]
pub use linux::Screen;

#[cfg(windows)]
#[path = "present_windows.rs"]
mod windows;
#[cfg(windows)]
pub use windows::Screen;

#[cfg(target_os = "macos")]
#[path = "present_macos.rs"]
mod macos;
#[cfg(target_os = "macos")]
pub use macos::Screen;
