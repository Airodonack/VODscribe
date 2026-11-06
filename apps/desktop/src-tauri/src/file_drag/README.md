# File drag

The system's drag of files out of a window, begun before the files are made, on macOS. It stands alone, with no Tauri and no library types.

- `mod.rs`: the drag's interface: a window handle and one `Item` per file.
- `macos.rs`: AppKit's drag, each file a file promise beside its address.
- `placeholder.rs`: the empty file standing at a promised file's place while it's made, and the copy that takes its place.
