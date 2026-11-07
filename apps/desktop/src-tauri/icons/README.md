# Icons

The app icon in every size, generated from the two SVGs. Keep the two SVGs' glyphs in sync.

- `app-icon.svg`: the source of every icon but macOS's.
- `app-icon-macos.svg`: the source of `icon.icns`, on Apple's grid with a smaller rounded square and a shadow.
- `icon.icns`, `icon.ico`, `icon.png`, the `*.png` sizes, `android/` and `ios/`: generated; Tauri picks what each system needs.

To change the icon, from `apps/desktop`:

```sh
pnpm tauri icon src-tauri/icons/app-icon.svg -o src-tauri/icons
pnpm tauri icon src-tauri/icons/app-icon-macos.svg -o /tmp/mac-icons   # then copy only its icon.icns here
```

Then copy the 32 px PNG to `apps/desktop/static/favicon.png`, and `icon.ico` to `apps/site/static/favicon.ico`.
