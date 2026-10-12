#!/usr/bin/env bash
set -euo pipefail
: "${HOME:?HOME is required}"
source_dir=$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)
for name in KONTRA.clap KONTRA.vst3 kontakto-standalone LICENSES.txt; do
  test -e "$source_dir/$name" || { echo "Missing package file: $name" >&2; exit 1; }
done
mkdir -p "$HOME/.clap" "$HOME/.vst3" "$HOME/.local/bin" "$HOME/.local/share/kontra"
tmp=$(mktemp -d "$HOME/.vst3/.KONTRA-install.XXXXXX")
trap 'rm -rf -- "$tmp"' EXIT
cp -R -- "$source_dir/KONTRA.vst3" "$tmp/KONTRA.vst3"
if test -e "$HOME/.vst3/KONTRA.vst3" || test -L "$HOME/.vst3/KONTRA.vst3"; then
  mv -- "$HOME/.vst3/KONTRA.vst3" "$tmp/previous"
fi
if ! mv -- "$tmp/KONTRA.vst3" "$HOME/.vst3/KONTRA.vst3"; then
  test ! -e "$tmp/previous" || mv -- "$tmp/previous" "$HOME/.vst3/KONTRA.vst3"
  exit 1
fi
install -m 755 -- "$source_dir/KONTRA.clap" "$HOME/.clap/KONTRA.clap"
install -m 755 -- "$source_dir/kontakto-standalone" "$HOME/.local/bin/kontakto-standalone"
install -m 644 -- "$source_dir/LICENSES.txt" "$HOME/.local/share/kontra/LICENSES.txt"
printf '%s\n' 'Installed KONTRA. Rescan plugins in your DAW; run ~/.local/bin/kontakto-standalone for the standalone app.'
