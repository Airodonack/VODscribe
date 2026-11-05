# Drop target

The window's own drop target, which also takes virtual files such as a video inside a zip folder on Windows, and file promises on macOS.

- `mod.rs`: the drop target's interface, kept free of the app's types.
- `windows.rs`: OLE's `IDropTarget`, taking paths or virtual files.
- `macos.rs`: wry's web view's dragging methods, hooked to take file promises.
