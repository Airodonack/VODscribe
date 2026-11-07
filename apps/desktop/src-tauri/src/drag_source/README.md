# Drag source

Dragging clips out of the window before they're made. Each system's drag starts here; its data comes from the [`dragndrop-source`](../../../../../crates/dragndrop-source) crate.

- `mod.rs`: what clips.rs calls, and the drag's trace in test builds.
- `linux.rs`: GTK's drag, its picture, and the release the page never sees.
- `macos.rs`: AppKit's drag session from the web view's press.
- `windows.rs`: OLE's drag, with its picture.
