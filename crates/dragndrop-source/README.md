# dragndrop-source

Drag files out of your window before they exist. The drag begins at once; each file is made after the drop and handed over once it's there. Explorer and Finder show their own copy progress while they wait. An app that takes only finished files gets nothing, and you hear that it didn't.

It builds the drag's data only. You start your own drag (`DoDragDrop`, `beginDraggingSession`, `gtk_drag_begin`) with what it gives you, and pass on the drag's events. It owns no event loop and starts nothing.

Windows (virtual files), macOS (file promises) and Linux (GTK 3, with KDE's Ark extraction for Dolphin) work.

```rust
struct Clip { name: String, file: Arc<Mutex<Option<PathBuf>>> }

impl dragndrop_source::Item for Clip {
    fn name(&self) -> String { self.name.clone() }
    fn state(&self) -> dragndrop_source::State {
        match &*self.file.lock().unwrap() {
            Some(path) => dragndrop_source::State::Ready(path.clone()),
            None => dragndrop_source::State::Making(None),
        }
    }
}

// Windows, on the UI thread, the button down:
let drag = dragndrop_source::windows::Drag::new(hwnd, vec![Box::new(clip)], |event| println!("{event:?}"))?;
let mut effect = DROPEFFECT_NONE;
let hr = DoDragDrop(drag.data_object(), &drag.drop_source(), DROPEFFECT_COPY, &mut effect);
drag.returned(hr, effect);
```

```rust
// Linux: the drag's targets, and its signals passed on.
let drag = dragndrop_source::linux::Drag::new(items, on_event);
let asked = drag.clone();
widget.connect_drag_data_get(move |_, context, data, info, _| asked.data_get(context, data, info));
// ...drag-failed, drag-end and drop-performed likewise, then:
widget.drag_begin_with_coordinates(&drag.targets(), gdk::DragAction::COPY, 1, None, -1, -1);
```

- [`src`](src): the code.
- `Cargo.toml`: the crate, its licence and its dependencies: the windows crate, objc2's on macOS, and gtk on Linux.
- `LICENSE-MIT`, `LICENSE-APACHE`: the two licences.

Licensed under either of [Apache License 2.0](LICENSE-APACHE) or [MIT](LICENSE-MIT), at your option.
