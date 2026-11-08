# Drop target

The window's own drop target, which also takes virtual files: a video inside a zip folder on Windows, a file promise on macOS, a file dragged out of an archive manager on Linux. On Windows and macOS it takes links from other apps too.

- `mod.rs`: the drop target's interface, kept free of the app's types.
- `windows.rs`: OLE's `IDropTarget`, taking paths, virtual files or links.
- `macos.rs`: wry's web view's dragging methods, hooked to take file promises and links.
- `linux.rs`: GTK's drag signals on the page, taking the drops the web view can't.
