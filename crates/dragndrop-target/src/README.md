# dragndrop-target's code

- `lib.rs`: the types every system shares, and what you give and get on each.
- `windows.rs`: Windows' `IDataObject`.
- `macos.rs`: macOS's `NSDraggingInfo` and its file promises.
- `linux.rs`: GTK 3's drags: file addresses, the document portal, and sources that save files for you.
- `descriptor.rs`: FileGroupDescriptorW read from its bytes.
- `folder.rs`: the folder destination and safe file names.
