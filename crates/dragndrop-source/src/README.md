# dragndrop-source's code

- `lib.rs`: what every system shares: the files you give, and what you hear back.
- `windows.rs`: OLE's `IDataObject` and `IDropSource`, each file a virtual file beside the file itself.
- `macos.rs`: AppKit's dragging items, each file a file promise beside its address.
- `placeholder.rs`: the empty file at a promised file's place while it's made, and the copy that takes its place.
- `linux.rs`: GTK 3's drag data, the hold while files are made, and apps refused the files.
- `ark.rs`: KDE's Ark extraction, which Dolphin takes.
