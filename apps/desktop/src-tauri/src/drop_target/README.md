# Drop target

The window's own drop target on Windows, which also takes virtual files such as a video inside a zip folder.

- `mod.rs`: the drop target's interface, kept free of the app's types.
- `windows.rs`: OLE's `IDropTarget`, taking paths or virtual files.
