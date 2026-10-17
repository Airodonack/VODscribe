//! The native surface the picture is drawn on, over the page's box in the
//! app's window, which the page reports (Screen.svelte). On Linux it's a
//! Wayland subsurface or a child X window with its own EGL context
//! (present_linux.rs); on Windows a child window drawn with Direct3D or GDI
//! (present_windows.rs). macOS has no surface yet.

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
