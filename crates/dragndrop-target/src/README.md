# dragndrop-target's code

- `lib.rs`: the types every system shares, and what you give and get on each.
- `windows.rs`: Windows' `IDataObject`.
- `macos.rs`: macOS's `NSDraggingInfo` and its file promises.
- `descriptor.rs`: FileGroupDescriptorW read from its bytes.
- `folder.rs`: the folder destination and safe file names.
