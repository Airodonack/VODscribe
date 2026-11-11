# The library's tests

- `tests.rs`: the helpers the specs of `db.rs` share, the specs not yet in a file of their own, and the files in `tests/` that hold the rest.
- `tests/migrations_early.rs`: upgrades of libraries from before the clip model (V1 to V21), and the upgrade itself: the copy kept before it, a library from a newer version.
- `tests/migrations_late.rs`: upgrades from the clip model on (V22 on).
- `tests/search.rs`: search over transcripts and chat.
- `tests/chat_store.rs`: chat kept in the library.
- `tests/library.rs`: the library file itself, a broken one set aside.
