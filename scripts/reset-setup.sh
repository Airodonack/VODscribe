#!/bin/sh
# Removes the files the first-run setup downloads (speech model, yt-dlp) so the
# setup screen appears again. The library, transcripts, settings and GPU pack stay.
set -eu
case "$(uname -s)" in
  Linux) data="${XDG_DATA_HOME:-$HOME/.local/share}/app.vodscribe" ;;
  Darwin) data="$HOME/Library/Application Support/app.vodscribe" ;;
  *) echo "Unsupported OS; delete models/ and bin/ in the app data folder by hand." >&2; exit 1 ;;
esac
if pgrep -f 'target/(debug|release)/vodscribe$|/vodscribe-worker' >/dev/null 2>&1; then
  echo "vodscribe is running. Quit it first." >&2
  exit 1
fi
for dir in models bin; do
  if [ -d "$data/$dir" ]; then
    du -sh "$data/$dir"
    rm -rf "$data/$dir"
  fi
done
echo "Setup files removed from $data. The next launch shows the setup screen."
