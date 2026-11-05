# Drag source

Dragging files out of the window on Windows before they're made. Linux's lives in `clips.rs`, macOS's in `file_drag`.

- `mod.rs`: the drag's interface, kept free of the app's types.
- `windows.rs`: OLE's drag, offering each file as a virtual file and as the file itself.
