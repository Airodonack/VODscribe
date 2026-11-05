# dragndrop-target

Read what a drag dropped on your window offers: the plain paths, and the virtual files that have no path yet. A video dragged out of a zip folder in Explorer arrives as nothing in most apps; with this crate it arrives as a file.

It reads data only. Your drop target stays yours: from its drop handler you pass the native drag object, and you get the paths, the virtual items (a name, a size if known), and a fetch of each item into a folder or a writer, with progress and cancel.

Windows works. macOS (file promises) and Linux (portal file transfer, XdndDirectSave) are to come.

```rust
// Inside your IDropTarget::Drop, with the IDataObject it was given:
let items = dragndrop_target::windows::items(data);
let requests = items
    .into_iter()
    .map(|item| (item, dragndrop_target::Destination::folder(&downloads, |got| println!("{got:?}"))))
    .collect();
let fetch = dragndrop_target::windows::fetch(data, requests);
// Keep `fetch` until fetch.is_finished(): dropping it cancels.
```

- [`src`](src): the code.
- `Cargo.toml`: the crate, its licence and its one dependency.
- `LICENSE-MIT`, `LICENSE-APACHE`: the two licences.

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
