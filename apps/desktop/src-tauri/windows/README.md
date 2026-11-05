# Windows bundle files

- `installer.nsi`: a copy of Tauri's NSIS installer template, changed where marked "VODscribe:".

When the Tauri CLI is updated, take its template again and reapply those changes; `scripts/check-nsis-template.py` fails until then.
