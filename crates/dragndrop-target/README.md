# dragndrop-target

Read what a drag dropped on your window offers: the plain paths, the virtual files that have no path yet, and on Windows and macOS the web links of a link dragged from a browser. A video dragged out of a zip folder in Explorer arrives as nothing in most apps; with this crate it arrives as a file.

It reads data only. Your drop target stays yours: from its drop handler you pass the native drag object, and you get the paths, the virtual items (a name, a size if known), and a fetch of each item into a folder or a writer, with progress and cancel.

Windows, macOS (file promises) and Linux work. On Linux (GTK 3) it reads the files of sandboxed apps (the document portal's file transfer) as paths, and has archive managers save theirs for you (XdndDirectSave from File Roller and Engrampa, Ark's own way on KDE).

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

```rust
// Inside your GTK widget's drag-drop handler (return true from it):
dragndrop_target::linux::receive(context, time, &staging, move |received| {
    let requests = received.offer().items.iter()
        .map(|item| (item.clone(), dragndrop_target::Destination::folder(&downloads, |got| println!("{got:?}"))))
        .collect();
    let fetch = dragndrop_target::linux::fetch(received, requests);
    // Keep `fetch` until fetch.is_finished(), as above.
});
```

- [`src`](src): the code.
- [`examples`](examples): a window that takes drops, on Linux.
- `Cargo.toml`: the crate, its licence and its dependencies: the windows crate, objc2's on macOS, and gtk on Linux.
- `LICENSE-MIT`, `LICENSE-APACHE`: the two licences.

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
